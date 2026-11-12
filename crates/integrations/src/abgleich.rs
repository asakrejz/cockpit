//! Der Abruf für den Abgleich: holt Tickets, Pull Requests, Kommentare, Checks und Diffs über
//! die Clients und packt sie als [`Remote`] für `cockpit_core::sync::merge`.
//!
//! Scheitert eine Quelle, bleibt ihr Stand im Workspace unverändert; der Fehler wird zum
//! [`Problem`] der Quelle. Der Abruf selbst schreibt nichts.

use cockpit_core::{Loaded, Outcome, PrLink, Remote, Review, Source, SourceData, Workspace};

use crate::Fehler;
use crate::ausgehend::{Ausgehend, Ziele};
use crate::github::{
    GitHubClient, Relation, aenderungen_angefordert, check_runs_to_core, comments_mit_reviews,
    pull_to_item, pull_to_review,
};
use crate::jira::{self, JiraClient};

pub struct GitHubDienst {
    pub client: GitHubClient,
    /// `owner/name`
    pub repos: Vec<String>,
}

pub struct JiraDienst {
    pub client: JiraClient,
    pub jql: String,
    /// Die Abfrage holt alle Tickets eines Projekts ([`jira::jql_fuer_projekt`]): Was weder
    /// zugewiesen noch beobachtet ist, steht dann mit dem Grund „Im Projekt“ da.
    pub im_projekt: bool,
}

/// Die eingerichteten Dienste. Eine Quelle, die fehlt, ist aus.
#[derive(Default)]
pub struct Dienste {
    pub github: Option<GitHubDienst>,
    pub jira: Option<JiraDienst>,
}

impl Dienste {
    pub fn ist_leer(&self) -> bool {
        self.github.is_none() && self.jira.is_none()
    }

    pub fn ziele(&self) -> Ziele {
        Ziele {
            github: self.github.is_some(),
            jira: self.jira.is_some(),
            repos: self
                .github
                .as_ref()
                .map(|g| g.repos.clone())
                .unwrap_or_default(),
        }
    }

    /// Führt den Aufruf beim zuständigen Dienst aus.
    pub async fn ausfuehren(&self, a: &Ausgehend) -> Result<(), Fehler> {
        let jira = matches!(
            a,
            Ausgehend::JiraKommentar { .. } | Ausgehend::JiraStatus { .. }
        );
        if jira {
            match &self.jira {
                Some(j) => j.client.ausfuehren(a).await,
                None => Err(Fehler::Einstellung("Jira ist nicht eingerichtet".into())),
            }
        } else {
            match &self.github {
                Some(g) => g.client.ausfuehren(a).await,
                None => Err(Fehler::Einstellung("GitHub ist nicht eingerichtet".into())),
            }
        }
    }

    /// Fragt alle Quellen ab, die nicht in `pausiert` stehen. `bekannt` ist der Stand des
    /// Workspace (für Diffs, die sich nicht geändert haben), `jetzt_ms` die Uhrzeit.
    pub async fn holen(&self, bekannt: &Workspace, jetzt_ms: u64, pausiert: &[Source]) -> Remote {
        let jetzt_s = (jetzt_ms / 1000) as i64;
        let mut sources = Vec::new();
        if let Some(g) = self
            .github
            .as_ref()
            .filter(|_| !pausiert.contains(&Source::GitHub))
        {
            let res = github_laden(&g.client, &g.repos, bekannt, jetzt_s).await;
            sources.push(als_quelle(Source::GitHub, res, jetzt_ms));
        }
        if let Some(j) = self
            .jira
            .as_ref()
            .filter(|_| !pausiert.contains(&Source::Jira))
        {
            let res = jira_laden(&j.client, &j.jql, j.im_projekt, jetzt_s).await;
            sources.push(als_quelle(Source::Jira, res, jetzt_ms));
        }
        Remote { sources }
    }
}

fn als_quelle(source: Source, res: Result<Loaded, Fehler>, jetzt_ms: u64) -> SourceData {
    SourceData {
        source,
        outcome: match res {
            Ok(loaded) => Outcome::Loaded(loaded),
            Err(e) => Outcome::Failed(e.problem(jetzt_ms)),
        },
    }
}

/// Ein fehlendes oder verbotenes Repo ist ein Hinweis, kein Ausfall der ganzen Quelle.
fn nur_hinweis(e: &Fehler) -> bool {
    matches!(
        e,
        Fehler::NichtGefunden | Fehler::KeineBerechtigung | Fehler::Abgelehnt(_)
    )
}

pub async fn github_laden(
    client: &GitHubClient,
    repos: &[String],
    bekannt: &Workspace,
    jetzt_s: i64,
) -> Result<Loaded, Fehler> {
    let ich = client.ich().await?;
    let mut out = Loaded::default();
    let mut gesehen: Vec<(String, u32)> = Vec::new();

    for repo in repos {
        let mut treffer: Vec<(u32, Relation)> = Vec::new();
        for relation in [Relation::ReviewAngefragt, Relation::Eigener] {
            match client.suche_in(relation, repo).await {
                Ok(liste) => treffer.extend(liste.into_iter().map(|(_, nr)| (nr, relation))),
                Err(e) if nur_hinweis(&e) => {
                    out.warnings.push(format!("{repo}: {e}"));
                    treffer.clear();
                    break;
                }
                Err(e) => return Err(e),
            }
        }
        for (nr, relation) in treffer {
            if gesehen.iter().any(|(r, n)| r == repo && *n == nr) {
                continue;
            }
            // Das Modell kennt Reviews nur über die Nummer: Gleiche Nummern aus anderen Repos
            // lassen sich nicht auseinanderhalten.
            if let Some((anderes, _)) = gesehen.iter().find(|(_, n)| *n == nr) {
                out.warnings.push(format!(
                    "#{nr} aus {repo} ausgelassen: die Nummer gehört schon zu {anderes}"
                ));
                continue;
            }
            gesehen.push((repo.clone(), nr));
            match pull_laden(client, repo, nr, relation, &ich, bekannt, jetzt_s).await {
                Ok((item, review, link)) => {
                    out.items.push(item);
                    out.reviews.push(review);
                    out.prs.push(link);
                }
                // Zwischen Suche und Abruf geschlossen oder gelöscht.
                Err(Fehler::NichtGefunden) => {}
                Err(e) => return Err(e),
            }
        }
    }
    Ok(out)
}

async fn pull_laden(
    client: &GitHubClient,
    repo: &str,
    nr: u32,
    relation: Relation,
    ich: &str,
    bekannt: &Workspace,
    jetzt_s: i64,
) -> Result<(cockpit_core::Item, Review, PrLink), Fehler> {
    let pull = client.pull(repo, nr).await?;
    let laeufe = client.check_runs(repo, &pull.head.sha).await?;
    let checks = crate::github::aggregate_checks(&laeufe);
    let urteile = client.reviews(repo, nr).await?;
    let kommentare = client.kommentare(repo, nr).await?;
    let zeilen = client.zeilen_kommentare(repo, nr).await?;

    // Der Diff ändert sich nur mit einem neuen Commit.
    let unveraendert = bekannt
        .sync
        .pr(nr)
        .filter(|l| l.repo == repo && l.head_sha == pull.head.sha)
        .and_then(|_| bekannt.review(nr));
    let dateien = match unveraendert {
        Some(_) => Vec::new(),
        None => client.dateien(repo, nr).await?,
    };

    let mut item = pull_to_item(repo, &pull, checks, relation, jetzt_s);
    item.comments = comments_mit_reviews(&kommentare, &zeilen, &urteile, ich, jetzt_s);
    if relation == Relation::Eigener
        && let Some(pr) = item.pr.as_mut()
    {
        pr.changes_requested = aenderungen_angefordert(&urteile, ich);
    }
    let mut review = pull_to_review(&pull, &dateien, checks);
    if let Some(alt) = unveraendert {
        review.files = alt.files.clone();
    }
    review.check_runs = check_runs_to_core(&laeufe);
    let link = PrLink {
        number: nr,
        repo: repo.to_string(),
        head_sha: pull.head.sha.clone(),
    };
    Ok((item, review, link))
}

pub async fn jira_laden(
    client: &JiraClient,
    jql: &str,
    im_projekt: bool,
    jetzt_s: i64,
) -> Result<Loaded, Fehler> {
    let ich = client.ich().await?;
    let mut out = Loaded::default();
    for issue in client.suche(jql).await? {
        let mut item = jira::issue_to_item(&issue, &ich, jetzt_s);
        if im_projekt && item.scope != cockpit_core::Scope::Mine {
            item.reason_text = "Im Projekt".into();
        }
        match client.kommentare(&issue.key).await {
            Ok(k) => item.comments = jira::comments_to_core(&k, &ich, jetzt_s),
            Err(Fehler::NichtGefunden) => continue,
            Err(e) => return Err(e),
        }
        out.items.push(item);
    }
    Ok(out)
}
