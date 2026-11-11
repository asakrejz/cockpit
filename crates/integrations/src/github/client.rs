//! Lesender Client für die GitHub-REST-API.

use cockpit_core::Item;

use serde::de::DeserializeOwned;

use super::dto::{
    CheckRun, CheckRuns, GitRefAntwort, IssueComment, Pull, PullFile, PullReview, Repo,
    ReviewComment, SearchItem, SearchResult, User,
};
use super::map;
use crate::ausgehend::{Anfrage, Ausgehend, branch_anfrage, check_neu_anfrage};
use crate::http::{basis_url, client, sende};
use crate::{Fehler, Secret};

/// Warum ein Pull Request in der Inbox steht.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Relation {
    /// Ein Review von mir ist angefragt.
    ReviewAngefragt,
    /// Der Pull Request gehört mir.
    Eigener,
}

impl Relation {
    fn suche(self) -> &'static str {
        match self {
            Relation::ReviewAngefragt => "is:pr is:open archived:false review-requested:@me",
            Relation::Eigener => "is:pr is:open archived:false author:@me",
        }
    }
}

/// Höchstens so viele Pull Requests je Relation werden pro Abruf geladen.
const MAX_PRS: usize = 30;
/// Seiten à 100 bei Dateien und Kommentaren (GitHub liefert höchstens 3000 Dateien).
const MAX_SEITEN: u32 = 3;

pub struct GitHubClient {
    http: reqwest::Client,
    basis: String,
    token: Secret,
}

impl std::fmt::Debug for GitHubClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GitHubClient")
            .field("basis", &self.basis)
            .field("token", &self.token)
            .finish()
    }
}

impl GitHubClient {
    /// `api_url` ist üblicherweise `https://api.github.com` (bei GitHub Enterprise
    /// `https://host/api/v3`).
    pub fn new(api_url: &str, token: Secret) -> Result<Self, Fehler> {
        Ok(Self {
            http: client()?,
            basis: basis_url(api_url)?,
            token,
        })
    }

    fn anfrage(&self, methode: reqwest::Method, pfad: &str) -> reqwest::RequestBuilder {
        self.http
            .request(methode, format!("{}{pfad}", self.basis))
            .bearer_auth(self.token.expose())
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
    }

    fn get(&self, pfad: &str) -> reqwest::RequestBuilder {
        self.anfrage(reqwest::Method::GET, pfad)
    }

    /// Schickt die Anfrage; ein Schlüssel, den der Dienst in einer Meldung zurückspielt,
    /// wird unkenntlich gemacht.
    async fn text(&self, req: reqwest::RequestBuilder) -> Result<String, Fehler> {
        sende(req)
            .await
            .map_err(|e| e.ohne_schluessel(self.token.expose()))
    }

    async fn lies_json<T: DeserializeOwned>(
        &self,
        req: reqwest::RequestBuilder,
    ) -> Result<T, Fehler> {
        let text = self.text(req).await?;
        serde_json::from_str(&text).map_err(|e| Fehler::Antwort(e.to_string()))
    }

    async fn schicke(&self, a: &Anfrage) -> Result<String, Fehler> {
        let methode = reqwest::Method::from_bytes(a.methode.as_bytes())
            .map_err(|e| Fehler::Einstellung(e.to_string()))?;
        let mut req = self.anfrage(methode, &a.pfad);
        if let Some(body) = &a.body {
            req = req.json(body);
        }
        self.text(req).await
    }

    /// Login des Schlüsselinhabers.
    pub async fn ich(&self) -> Result<String, Fehler> {
        Ok(self.lies_json::<User>(self.get("/user")).await?.login)
    }

    /// Offene Pull Requests, bei denen ein Review von mir angefragt ist oder die mir gehören.
    /// Liefert `(owner/name, Nummer)`.
    pub async fn suche(&self, relation: Relation) -> Result<Vec<(String, u32)>, Fehler> {
        self.suche_mit(relation.suche()).await
    }

    /// Wie [`GitHubClient::suche`], beschränkt auf ein Repo (`owner/name`).
    pub async fn suche_in(
        &self,
        relation: Relation,
        repo: &str,
    ) -> Result<Vec<(String, u32)>, Fehler> {
        self.suche_mit(&format!("{} repo:{repo}", relation.suche()))
            .await
    }

    /// Die Suche, seitenweise (je 100), bis es keine volle Seite mehr gibt oder
    /// [`MAX_PRS`] erreicht sind.
    async fn suche_mit(&self, q: &str) -> Result<Vec<(String, u32)>, Fehler> {
        let je_seite = MAX_PRS.min(100);
        let mut alle = Vec::new();
        for seite in 1..=MAX_SEITEN {
            let res: SearchResult = self
                .lies_json(self.get("/search/issues").query(&[
                    ("q", q),
                    ("sort", "updated"),
                    ("per_page", &je_seite.to_string()),
                    ("page", &seite.to_string()),
                ]))
                .await?;
            let voll = res.items.len() >= je_seite;
            alle.extend(
                res.items
                    .iter()
                    .filter_map(|i: &SearchItem| Some((i.repo()?, i.number))),
            );
            if !voll || alle.len() >= MAX_PRS {
                break;
            }
        }
        alle.truncate(MAX_PRS);
        Ok(alle)
    }

    pub async fn pull(&self, repo: &str, nr: u32) -> Result<Pull, Fehler> {
        self.lies_json(self.get(&format!("/repos/{repo}/pulls/{nr}")))
            .await
    }

    /// Geänderte Dateien mit Patch.
    pub async fn dateien(&self, repo: &str, nr: u32) -> Result<Vec<PullFile>, Fehler> {
        self.seiten(&format!("/repos/{repo}/pulls/{nr}/files"))
            .await
    }

    pub async fn kommentare(&self, repo: &str, nr: u32) -> Result<Vec<IssueComment>, Fehler> {
        self.seiten(&format!("/repos/{repo}/issues/{nr}/comments"))
            .await
    }

    pub async fn zeilen_kommentare(
        &self,
        repo: &str,
        nr: u32,
    ) -> Result<Vec<ReviewComment>, Fehler> {
        self.seiten(&format!("/repos/{repo}/pulls/{nr}/comments"))
            .await
    }

    /// Status der Checks für einen Commit.
    pub async fn checks(&self, repo: &str, sha: &str) -> Result<cockpit_core::Checks, Fehler> {
        Ok(map::aggregate_checks(&self.check_runs(repo, sha).await?))
    }

    /// Die einzelnen Check-Läufe eines Commits.
    pub async fn check_runs(&self, repo: &str, sha: &str) -> Result<Vec<CheckRun>, Fehler> {
        let runs: CheckRuns = self
            .lies_json(
                self.get(&format!("/repos/{repo}/commits/{sha}/check-runs"))
                    .query(&[("per_page", "100")]),
            )
            .await?;
        Ok(runs.check_runs)
    }

    /// Urteile der Reviewer.
    pub async fn reviews(&self, repo: &str, nr: u32) -> Result<Vec<PullReview>, Fehler> {
        self.seiten(&format!("/repos/{repo}/pulls/{nr}/reviews"))
            .await
    }

    async fn seiten<T: serde::de::DeserializeOwned>(&self, pfad: &str) -> Result<Vec<T>, Fehler> {
        let mut alle = Vec::new();
        for seite in 1..=MAX_SEITEN {
            let teil: Vec<T> = self
                .lies_json(
                    self.get(pfad)
                        .query(&[("per_page", "100"), ("page", &seite.to_string())]),
                )
                .await?;
            let voll = teil.len() == 100;
            alle.extend(teil);
            if !voll {
                break;
            }
        }
        Ok(alle)
    }

    /// Inbox-Einträge: erst Review-Anfragen, dann eigene Pull Requests (ohne Doppelte).
    /// Zu jedem Pull Request wird er selbst und der Check-Status nachgeladen.
    pub async fn lade_inbox(&self, jetzt: i64) -> Result<Vec<Item>, Fehler> {
        let mut items: Vec<Item> = Vec::new();
        let mut gesehen: Vec<(String, u32)> = Vec::new();
        for relation in [Relation::ReviewAngefragt, Relation::Eigener] {
            for (repo, nr) in self.suche(relation).await? {
                if gesehen.contains(&(repo.clone(), nr)) {
                    continue;
                }
                let pull = self.pull(&repo, nr).await?;
                let checks = self.checks(&repo, &pull.head.sha).await?;
                items.push(map::pull_to_item(&repo, &pull, checks, relation, jetzt));
                gesehen.push((repo, nr));
            }
        }
        Ok(items)
    }
}

// ---------- Schreiben ----------

impl GitHubClient {
    /// Führt einen Aufruf nach außen aus. Nur GitHub-Varianten; Jira ist Sache des Jira-Clients.
    pub async fn ausfuehren(&self, a: &Ausgehend) -> Result<(), Fehler> {
        match a {
            Ausgehend::GitHubBranch { repo, name } => self.branch_anlegen(repo, name).await,
            Ausgehend::GitHubCheckNeu {
                repo,
                nummer,
                check,
            } => self.check_neu_starten(repo, *nummer, check).await,
            andere => match andere.anfrage() {
                Some(anfrage) => self.schicke(&anfrage).await.map(|_| ()),
                None => Err(Fehler::Einstellung("Kein GitHub-Aufruf".into())),
            },
        }
    }

    /// Legt `name` als Ref vom Standard-Branch des Repos an.
    pub async fn branch_anlegen(&self, repo: &str, name: &str) -> Result<(), Fehler> {
        let r: Repo = self.lies_json(self.get(&format!("/repos/{repo}"))).await?;
        let r#ref: GitRefAntwort = self
            .lies_json(self.get(&format!("/repos/{repo}/git/ref/heads/{}", r.default_branch)))
            .await?;
        self.schicke(&branch_anfrage(repo, name, &r#ref.object.sha))
            .await
            .map(|_| ())
    }

    /// Fordert den Check mit diesem Namen am aktuellen Commit des Pull Requests neu an.
    pub async fn check_neu_starten(&self, repo: &str, nr: u32, check: &str) -> Result<(), Fehler> {
        let pull = self.pull(repo, nr).await?;
        let laeufe = self.check_runs(repo, &pull.head.sha).await?;
        let id = laeufe
            .iter()
            .find(|c| c.name == check)
            .map(|c| c.id)
            .ok_or_else(|| {
                Fehler::NichtMoeglich(format!("Check „{check}“ gibt es bei GitHub nicht (mehr)"))
            })?;
        self.schicke(&check_neu_anfrage(repo, id)).await.map(|_| ())
    }
}
