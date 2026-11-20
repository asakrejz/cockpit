//! Which right a GitHub credential lacks – and how to say it in plain words.
//!
//! GitHub Projects need rights that the issue and pull request features do not: a classic token
//! needs `read:project` (read) or `project` (write), a fine-grained token the "Projects"
//! permission of the organization or account plus "Issues" of the repo, a GitHub App the same
//! two permissions. Classic scopes are reported by GitHub in the header `X-OAuth-Scopes`; for
//! the other two kinds the only sign is a refusal (403 / `FORBIDDEN`), so the right that was
//! needed for the call that failed is the one that is named.

use cockpit_i18n::{Msg, msg};
use serde::{Deserialize, Serialize};

/// What kind of credential is in use.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TokenKind {
    /// A personal access token (classic): the scopes can be read.
    Classic,
    /// A fine-grained personal access token.
    FineGrained,
    /// A sign-in through a GitHub App (user token) or an installation token.
    App,
    #[default]
    Unknown,
}

/// A right that GitHub Projects need.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Right {
    ProjectsRead,
    ProjectsWrite,
    IssuesRead,
    IssuesWrite,
}

impl Right {
    pub const ALL: [Right; 4] = [
        Right::ProjectsRead,
        Right::ProjectsWrite,
        Right::IssuesRead,
        Right::IssuesWrite,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Right::ProjectsRead => "projects_read",
            Right::ProjectsWrite => "projects_write",
            Right::IssuesRead => "issues_read",
            Right::IssuesWrite => "issues_write",
        }
    }

    /// The scope of a classic token that gives the right.
    pub fn classic_scope(self) -> &'static str {
        match self {
            Right::ProjectsRead => "read:project",
            Right::ProjectsWrite => "project",
            Right::IssuesRead | Right::IssuesWrite => "repo",
        }
    }

    pub fn is_write(self) -> bool {
        matches!(self, Right::ProjectsWrite | Right::IssuesWrite)
    }

    fn is_projects(self) -> bool {
        matches!(self, Right::ProjectsRead | Right::ProjectsWrite)
    }

    /// Do these classic scopes give the right? `project` includes `read:project`; `repo` includes
    /// issues (`public_repo` only for public repos, which is accepted here).
    pub fn granted_by(self, scopes: &[String]) -> bool {
        let has = |s: &str| scopes.iter().any(|x| x == s);
        match self {
            Right::ProjectsRead => has("read:project") || has("project"),
            Right::ProjectsWrite => has("project"),
            Right::IssuesRead | Right::IssuesWrite => has("repo") || has("public_repo"),
        }
    }

    /// The sentence for people: what is missing and exactly how to fix it for this kind of token.
    pub fn msg(self, kind: TokenKind) -> Msg {
        let what = match self {
            Right::ProjectsRead => msg!(CoreRightWhatProjectsRead),
            Right::ProjectsWrite => msg!(CoreRightWhatProjectsWrite),
            Right::IssuesRead => msg!(CoreRightWhatIssuesRead),
            Right::IssuesWrite => msg!(CoreRightWhatIssuesWrite),
        };
        let level = if self.is_write() {
            "Read and write"
        } else {
            "Read-only"
        };
        let scope = self.classic_scope();
        let fix = match (kind, self.is_projects()) {
            (TokenKind::Classic, _) => msg!(CoreRightFixClassic, scope = scope),
            (TokenKind::FineGrained, true) => msg!(CoreRightFixFineProjects, level = level),
            (TokenKind::FineGrained, false) => msg!(CoreRightFixFineIssues, level = level),
            (TokenKind::App, true) => msg!(CoreRightFixAppProjects, level = level),
            (TokenKind::App, false) => msg!(CoreRightFixAppIssues, level = level),
            (TokenKind::Unknown, projects) => msg!(
                CoreRightFixUnknown,
                scope = scope,
                permission = if projects { "Projects" } else { "Issues" },
                level = level
            ),
        };
        msg!(CoreRightMissing, what = what, fix = fix)
    }
}

/// The scopes in the header `X-OAuth-Scopes` (`repo, read:project`).
///
/// ```
/// use cockpit_core::permissions::parse_scopes;
/// assert_eq!(parse_scopes("repo, read:project"), ["repo", "read:project"]);
/// assert!(parse_scopes("").is_empty());
/// ```
pub fn parse_scopes(header: &str) -> Vec<String> {
    header
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

/// Which of the wanted rights a classic token lacks, judged by its scopes.
pub fn missing_for_scopes(scopes: &[String], wanted: &[Right]) -> Vec<Right> {
    wanted
        .iter()
        .copied()
        .filter(|r| !r.granted_by(scopes))
        .collect()
}

/// What the client learned about the credential: its kind and, for classic tokens, its scopes.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct Access {
    #[serde(default)]
    pub kind: TokenKind,
    /// `None` = GitHub did not say (fine-grained tokens and apps never do).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scopes: Option<Vec<String>>,
}

impl Access {
    /// The rights from the wanted ones that are known to be missing. Only classic tokens can
    /// be judged ahead of time; for all others the answer is empty until a call is refused.
    pub fn missing(&self, wanted: &[Right]) -> Vec<Right> {
        match &self.scopes {
            Some(s) => missing_for_scopes(s, wanted),
            None => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scopes(s: &str) -> Vec<String> {
        parse_scopes(s)
    }

    #[test]
    fn project_scope_includes_read_project_but_not_the_other_way() {
        assert!(Right::ProjectsRead.granted_by(&scopes("project")));
        assert!(Right::ProjectsRead.granted_by(&scopes("read:project")));
        assert!(!Right::ProjectsWrite.granted_by(&scopes("read:project")));
        assert!(Right::ProjectsWrite.granted_by(&scopes("repo, project")));
    }

    #[test]
    fn issues_need_repo() {
        assert!(!Right::IssuesWrite.granted_by(&scopes("read:project")));
        assert!(Right::IssuesWrite.granted_by(&scopes("repo")));
    }

    #[test]
    fn missing_lists_only_what_is_lacking() {
        let missing = missing_for_scopes(
            &scopes("repo, read:project"),
            &[Right::ProjectsRead, Right::ProjectsWrite, Right::IssuesWrite],
        );
        assert_eq!(missing, [Right::ProjectsWrite]);
    }

    #[test]
    fn an_access_without_scopes_cannot_be_judged() {
        let a = Access {
            kind: TokenKind::FineGrained,
            scopes: None,
        };
        assert!(a.missing(&Right::ALL).is_empty());
        let c = Access {
            kind: TokenKind::Classic,
            scopes: Some(scopes("")),
        };
        assert_eq!(c.missing(&Right::ALL).len(), 4);
    }

    #[test]
    fn the_message_names_the_exact_right_for_each_kind() {
        let text = |r: Right, k: TokenKind| r.msg(k).render(cockpit_i18n::Lang::En);
        assert!(text(Right::ProjectsRead, TokenKind::Classic).contains("read:project"));
        assert!(text(Right::ProjectsWrite, TokenKind::Classic).contains("`project`"));
        assert!(text(Right::IssuesWrite, TokenKind::Classic).contains("`repo`"));
        let fine = text(Right::ProjectsWrite, TokenKind::FineGrained);
        assert!(fine.contains("Projects") && fine.contains("Read and write"));
        let issues = text(Right::IssuesWrite, TokenKind::FineGrained);
        assert!(issues.contains("Issues") && issues.contains("Read and write"));
        assert!(text(Right::ProjectsRead, TokenKind::App).contains("GitHub App"));
        assert!(text(Right::ProjectsRead, TokenKind::Unknown).contains("read:project"));
        // German too.
        assert!(
            Right::ProjectsRead
                .msg(TokenKind::Classic)
                .render(cockpit_i18n::Lang::De)
                .contains("read:project")
        );
    }
}
