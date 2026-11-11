//! Lesender Client für die GitHub-REST-API.

use cockpit_core::Item;

use super::dto::{
    CheckRuns, IssueComment, Pull, PullFile, ReviewComment, SearchItem, SearchResult, User,
};
use super::map;
use crate::http::{basis_url, client, lies_json};
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

    fn get(&self, pfad: &str) -> reqwest::RequestBuilder {
        self.http
            .get(format!("{}{pfad}", self.basis))
            .bearer_auth(self.token.expose())
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
    }

    /// Login des Schlüsselinhabers.
    pub async fn ich(&self) -> Result<String, Fehler> {
        Ok(lies_json::<User>(self.get("/user")).await?.login)
    }

    /// Offene Pull Requests, bei denen ein Review von mir angefragt ist oder die mir gehören.
    /// Liefert `(owner/name, Nummer)`.
    pub async fn suche(&self, relation: Relation) -> Result<Vec<(String, u32)>, Fehler> {
        let res: SearchResult = lies_json(self.get("/search/issues").query(&[
            ("q", relation.suche()),
            ("sort", "updated"),
            ("per_page", &MAX_PRS.to_string()),
        ]))
        .await?;
        Ok(res
            .items
            .iter()
            .filter_map(|i: &SearchItem| Some((i.repo()?, i.number)))
            .collect())
    }

    pub async fn pull(&self, repo: &str, nr: u32) -> Result<Pull, Fehler> {
        lies_json(self.get(&format!("/repos/{repo}/pulls/{nr}"))).await
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
        let runs: CheckRuns = lies_json(
            self.get(&format!("/repos/{repo}/commits/{sha}/check-runs"))
                .query(&[("per_page", "100")]),
        )
        .await?;
        Ok(map::aggregate_checks(&runs.check_runs))
    }

    async fn seiten<T: serde::de::DeserializeOwned>(&self, pfad: &str) -> Result<Vec<T>, Fehler> {
        let mut alle = Vec::new();
        for seite in 1..=MAX_SEITEN {
            let teil: Vec<T> = lies_json(
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
