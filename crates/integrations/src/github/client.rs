//! Lesender Client für die GitHub-REST-API.

use cockpit_core::Item;

use serde::de::DeserializeOwned;

use super::dto::{
    CheckRun, CheckRuns, GitRefResponse, IssueComment, Pull, PullFile, PullReview, Repo,
    ReviewComment, SearchItem, SearchResult, User,
};
use super::map;
use crate::outbound::{Request, Outbound, branch_request, check_again_request};
use crate::http::{base_url, client, send};
use crate::{Error, Secret};

/// Warum ein Pull Request in der Inbox steht.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Relation {
    /// Ein Review von mir ist angefragt.
    ReviewRequested,
    /// Der Pull Request gehört mir.
    Own,
}

impl Relation {
    fn search(self) -> &'static str {
        match self {
            Relation::ReviewRequested => "is:pr is:open archived:false review-requested:@me",
            Relation::Own => "is:pr is:open archived:false author:@me",
        }
    }
}

/// Höchstens so viele Pull Requests je Relation werden pro Abruf geladen.
const MAX_PRS: usize = 30;
/// Seiten à 100 bei Dateien und Kommentaren (GitHub liefert höchstens 3000 Dateien).
const MAX_PAGES: u32 = 3;

pub struct GitHubClient {
    http: reqwest::Client,
    base: String,
    token: Secret,
}

impl std::fmt::Debug for GitHubClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GitHubClient")
            .field("basis", &self.base)
            .field("token", &self.token)
            .finish()
    }
}

impl GitHubClient {
    /// `api_url` ist üblicherweise `https://api.github.com` (bei GitHub Enterprise
    /// `https://host/api/v3`).
    pub fn new(api_url: &str, token: Secret) -> Result<Self, Error> {
        Ok(Self {
            http: client()?,
            base: base_url(api_url)?,
            token,
        })
    }

    fn request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        self.http
            .request(method, format!("{}{path}", self.base))
            .bearer_auth(self.token.expose())
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
    }

    fn get(&self, path: &str) -> reqwest::RequestBuilder {
        self.request(reqwest::Method::GET, path)
    }

    /// Schickt die Anfrage; ein Schlüssel, den der Dienst in einer Meldung zurückspielt,
    /// wird unkenntlich gemacht.
    async fn text(&self, req: reqwest::RequestBuilder) -> Result<String, Error> {
        send(req)
            .await
            .map_err(|e| e.without_key(self.token.expose()))
    }

    async fn lies_json<T: DeserializeOwned>(
        &self,
        req: reqwest::RequestBuilder,
    ) -> Result<T, Error> {
        let text = self.text(req).await?;
        serde_json::from_str(&text).map_err(|e| Error::Response(e.to_string()))
    }

    async fn send(&self, a: &Request) -> Result<String, Error> {
        let method = reqwest::Method::from_bytes(a.method.as_bytes())
            .map_err(|e| Error::Config(e.to_string()))?;
        let mut req = self.request(method, &a.path);
        if let Some(body) = &a.body {
            req = req.json(body);
        }
        self.text(req).await
    }

    /// Login des Schlüsselinhabers.
    pub async fn me(&self) -> Result<String, Error> {
        Ok(self.lies_json::<User>(self.get("/user")).await?.login)
    }

    /// Offene Pull Requests, bei denen ein Review von mir angefragt ist oder die mir gehören.
    /// Liefert `(owner/name, Nummer)`.
    pub async fn search(&self, relation: Relation) -> Result<Vec<(String, u32)>, Error> {
        self.search_with(relation.search()).await
    }

    /// Wie [`GitHubClient::suche`], beschränkt auf ein Repo (`owner/name`).
    pub async fn search_in(
        &self,
        relation: Relation,
        repo: &str,
    ) -> Result<Vec<(String, u32)>, Error> {
        self.search_with(&format!("{} repo:{repo}", relation.search()))
            .await
    }

    /// Die Suche, seitenweise (je 100), bis es keine volle Seite mehr gibt oder
    /// [`MAX_PRS`] erreicht sind.
    async fn search_with(&self, q: &str) -> Result<Vec<(String, u32)>, Error> {
        let per_page = MAX_PRS.min(100);
        let mut all = Vec::new();
        for page in 1..=MAX_PAGES {
            let res: SearchResult = self
                .lies_json(self.get("/search/issues").query(&[
                    ("q", q),
                    ("sort", "updated"),
                    ("per_page", &per_page.to_string()),
                    ("page", &page.to_string()),
                ]))
                .await?;
            let full = res.items.len() >= per_page;
            all.extend(
                res.items
                    .iter()
                    .filter_map(|i: &SearchItem| Some((i.repo()?, i.number))),
            );
            if !full || all.len() >= MAX_PRS {
                break;
            }
        }
        all.truncate(MAX_PRS);
        Ok(all)
    }

    pub async fn pull(&self, repo: &str, nr: u32) -> Result<Pull, Error> {
        self.lies_json(self.get(&format!("/repos/{repo}/pulls/{nr}")))
            .await
    }

    /// Geänderte Dateien mit Patch.
    pub async fn files(&self, repo: &str, nr: u32) -> Result<Vec<PullFile>, Error> {
        self.pages(&format!("/repos/{repo}/pulls/{nr}/files"))
            .await
    }

    pub async fn comments(&self, repo: &str, nr: u32) -> Result<Vec<IssueComment>, Error> {
        self.pages(&format!("/repos/{repo}/issues/{nr}/comments"))
            .await
    }

    pub async fn line_comments(
        &self,
        repo: &str,
        nr: u32,
    ) -> Result<Vec<ReviewComment>, Error> {
        self.pages(&format!("/repos/{repo}/pulls/{nr}/comments"))
            .await
    }

    /// Status der Checks für einen Commit.
    pub async fn checks(&self, repo: &str, sha: &str) -> Result<cockpit_core::Checks, Error> {
        Ok(map::aggregate_checks(&self.check_runs(repo, sha).await?))
    }

    /// Die einzelnen Check-Läufe eines Commits.
    pub async fn check_runs(&self, repo: &str, sha: &str) -> Result<Vec<CheckRun>, Error> {
        let runs: CheckRuns = self
            .lies_json(
                self.get(&format!("/repos/{repo}/commits/{sha}/check-runs"))
                    .query(&[("per_page", "100")]),
            )
            .await?;
        Ok(runs.check_runs)
    }

    /// Urteile der Reviewer.
    pub async fn reviews(&self, repo: &str, nr: u32) -> Result<Vec<PullReview>, Error> {
        self.pages(&format!("/repos/{repo}/pulls/{nr}/reviews"))
            .await
    }

    async fn pages<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<Vec<T>, Error> {
        let mut all = Vec::new();
        for page in 1..=MAX_PAGES {
            let part: Vec<T> = self
                .lies_json(
                    self.get(path)
                        .query(&[("per_page", "100"), ("page", &page.to_string())]),
                )
                .await?;
            let full = part.len() == 100;
            all.extend(part);
            if !full {
                break;
            }
        }
        Ok(all)
    }

    /// Inbox-Einträge: erst Review-Anfragen, dann eigene Pull Requests (ohne Doppelte).
    /// Zu jedem Pull Request wird er selbst und der Check-Status nachgeladen.
    pub async fn load_inbox(&self, now: i64) -> Result<Vec<Item>, Error> {
        let mut items: Vec<Item> = Vec::new();
        let mut seen: Vec<(String, u32)> = Vec::new();
        for relation in [Relation::ReviewRequested, Relation::Own] {
            for (repo, nr) in self.search(relation).await? {
                if seen.contains(&(repo.clone(), nr)) {
                    continue;
                }
                let pull = self.pull(&repo, nr).await?;
                let checks = self.checks(&repo, &pull.head.sha).await?;
                items.push(map::pull_to_item(&repo, &pull, checks, relation, now));
                seen.push((repo, nr));
            }
        }
        Ok(items)
    }
}

// ---------- Schreiben ----------

impl GitHubClient {
    /// Führt einen Aufruf nach außen aus. Nur GitHub-Varianten; Jira ist Sache des Jira-Clients.
    pub async fn execute(&self, a: &Outbound) -> Result<(), Error> {
        match a {
            Outbound::GitHubBranch { repo, name } => self.branch_create(repo, name).await,
            Outbound::GitHubRecheck {
                repo,
                number,
                check,
            } => self.check_again_start(repo, *number, check).await,
            other => match other.request() {
                Some(request) => self.send(&request).await.map(|_| ()),
                None => Err(Error::Config("Kein GitHub-Aufruf".into())),
            },
        }
    }

    /// Legt `name` als Ref vom Standard-Branch des Repos an.
    pub async fn branch_create(&self, repo: &str, name: &str) -> Result<(), Error> {
        let r: Repo = self.lies_json(self.get(&format!("/repos/{repo}"))).await?;
        let r#ref: GitRefResponse = self
            .lies_json(self.get(&format!("/repos/{repo}/git/ref/heads/{}", r.default_branch)))
            .await?;
        self.send(&branch_request(repo, name, &r#ref.object.sha))
            .await
            .map(|_| ())
    }

    /// Fordert den Check mit diesem Namen am aktuellen Commit des Pull Requests neu an.
    pub async fn check_again_start(&self, repo: &str, nr: u32, check: &str) -> Result<(), Error> {
        let pull = self.pull(repo, nr).await?;
        let runs = self.check_runs(repo, &pull.head.sha).await?;
        let id = runs
            .iter()
            .find(|c| c.name == check)
            .map(|c| c.id)
            .ok_or_else(|| {
                Error::NotPossible(format!("Check „{check}“ gibt es bei GitHub nicht (mehr)"))
            })?;
        self.send(&check_again_request(repo, id)).await.map(|_| ())
    }
}
