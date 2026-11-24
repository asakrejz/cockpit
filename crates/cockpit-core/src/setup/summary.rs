//! The plain-text summary of the whole setup.

use cockpit_i18n::{ArgValue, Msg, msg};

use super::*;

/// The plain text for the result of the GitHub connection test. Never names the token.
pub fn github_check_text(cause: GitHubCause, user: Option<&str>) -> Msg {
    match cause {
        GitHubCause::Ok => match user.filter(|n| !n.is_empty()) {
            Some(n) => msg!(CoreSetupGithubOkUser, name = n),
            None => msg!(CoreSetupGithubOk),
        },
        GitHubCause::NotConnected => msg!(CoreSetupGithubNotConnected),
        GitHubCause::Token => msg!(CoreSetupGithubToken),
        GitHubCause::Permission => msg!(CoreSetupGithubPermission),
        GitHubCause::RateLimit => msg!(CoreSetupGithubRateLimit),
        GitHubCause::Network => msg!(CoreSetupGithubNetwork),
        GitHubCause::Other => msg!(CoreSetupGithubOther),
    }
}

/// The plain text for the result of the Jira connection test. Never names the token.
pub fn jira_check_text(cause: JiraCause, name: Option<&str>) -> Msg {
    match cause {
        JiraCause::Ok => match name.filter(|n| !n.is_empty()) {
            Some(n) => msg!(CoreSetupJiraOkName, name = n),
            None => msg!(CoreSetupJiraOk),
        },
        JiraCause::Details => msg!(CoreSetupJiraDetails),
        JiraCause::Address => msg!(CoreSetupJiraAddress),
        JiraCause::Token => msg!(CoreSetupJiraToken),
        JiraCause::Permission => msg!(CoreSetupJiraPermission),
        JiraCause::Reachable => msg!(CoreSetupJiraReachable),
        JiraCause::Other => msg!(CoreSetupJiraOther),
    }
}

/// How well an item of the summary stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    /// Works (green checkmark).
    Good,
    /// Not set up yet or skipped – no error.
    Open,
    /// Something went wrong or is incomplete.
    Problem,
}

/// A row of the summary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    pub name: Msg,
    pub tone: Tone,
    /// Plain text of what is going on.
    pub text: Msg,
    /// The step in which this can be fixed.
    pub step: Step,
}

/// What the summary needs to know.
#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub setup: Setup,
    pub jira: ServiceStatus,
    pub github: ServiceStatus,
    pub endpoint: EndpointStatus,
    pub container: ContainerStatus,
    /// Whether the real agent can start (image built, everything set up).
    pub agent: AgentStatus,
    /// Result of the last connection test to the endpoint, if it has run already.
    pub check: Option<EndpointCheck>,
    /// What GitHub shows of the configured repos, if it was asked (`GET /api/github/access`).
    pub github_access: Option<crate::api::GitHubAccess>,
}

pub(super) fn service_line(
    name: Msg,
    provider: &str,
    step: Step,
    status: &ServiceStatus,
    client_id: bool,
    secret: bool,
    token_way: Option<Msg>,
) -> Line {
    let (tone, text) = match (status.connected, status.kind) {
        // The sign-in ran out and no token takes over: say why, not "not set up".
        (false, _) if status.expired => (Tone::Problem, msg!(CoreSetupExpired)),
        (true, AuthKind::OAuth) => (Tone::Good, msg!(CoreSetupConnectedOauth, name = provider)),
        (true, _) => (Tone::Good, msg!(CoreSetupConnectedToken)),
        (false, _) if !client_id && !secret => (
            Tone::Open,
            match token_way {
                Some(hint) => msg!(CoreSetupNotSetUpHint, hint = hint),
                None => msg!(CoreSetupNotSetUp),
            },
        ),
        (false, _) if !client_id => (Tone::Problem, msg!(CoreSetupClientIdMissing)),
        (false, _) if !secret => (Tone::Problem, msg!(CoreSetupSecretMissing)),
        (false, _) => (Tone::Open, msg!(CoreSetupAlmostDone, name = provider)),
    };
    Line {
        name,
        tone,
        text,
        step,
    }
}

/// The Jira row: token (default) or OAuth, with the kind of sign-in in the text.
pub(super) fn jira_line(s: &Snapshot) -> Line {
    let e = &s.setup;
    let client_id = e.atlassian_client_id.is_some();
    let secret = e.has_key("atlassian-oauth");
    let (tone, text) = match (s.jira.connected, s.jira.kind) {
        (false, _) if s.jira.expired => (Tone::Problem, msg!(CoreSetupExpired)),
        (true, AuthKind::OAuth) => (Tone::Good, msg!(CoreSetupJiraConnectedOauth)),
        (true, _) => (Tone::Good, msg!(CoreSetupConnectedToken)),
        // The way via the Atlassian app is started: say what is missing, as before.
        (false, _) if client_id || secret => {
            let z = service_line(
                msg!(CoreSetupNameJira),
                "Atlassian",
                Step::Jira,
                &s.jira,
                client_id,
                secret,
                None,
            );
            (z.tone, z.text)
        }
        (false, _) => {
            let missing: Vec<Msg> = [
                (e.jira_site.is_empty(), msg!(CoreSetupMissingSite)),
                (e.jira_email.is_empty(), msg!(CoreSetupMissingEmail)),
                (!e.has_key("jira"), msg!(CoreSetupMissingToken)),
            ]
            .into_iter()
            .filter_map(|(missing, what)| missing.then_some(what))
            .collect();
            if missing.len() == 3 {
                (Tone::Open, msg!(CoreSetupJiraNotSetUp))
            } else {
                (
                    Tone::Open,
                    msg!(
                        CoreSetupJiraTokenMissing,
                        what = ArgValue::joined(missing, ", ")
                    ),
                )
            }
        }
    };
    Line {
        name: msg!(CoreSetupNameJira),
        tone,
        text,
        step: Step::Jira,
    }
}

/// The summary at the end: per item a checkmark or a sentence saying what is missing.
pub fn summary(s: &Snapshot) -> Vec<Line> {
    let e = &s.setup;
    let container = match (&s.container.runtime, s.container.engine_running) {
        // One step is still missing without an image of the agent.
        (Some(l), true) if !s.agent.image => {
            (Tone::Open, msg!(CoreSetupContainerNoImage, runtime = l))
        }
        (Some(l), true) => (Tone::Good, msg!(CoreSetupContainerOk, runtime = l)),
        (Some(l), false) => (Tone::Problem, msg!(CoreSetupContainerStopped, runtime = l)),
        (None, _) => (Tone::Open, msg!(CoreSetupContainerNone)),
    };
    let jira = jira_line(s);
    let mut github = service_line(
        msg!(CoreSetupNameGithub),
        "GitHub",
        Step::GitHub,
        &s.github,
        e.github_client_id.is_some(),
        e.has_key("github-oauth"),
        Some(msg!(CoreSetupGithubAppNeeded)),
    );
    // Connected is enough: the pull requests are found without a list of repos. Only repos that
    // are watched on purpose have to be visible, or their lists stay empty.
    if github.tone == Tone::Good
        && let Some(a) = s.github_access.as_ref().filter(|a| a.problem())
    {
        let hidden: Vec<Msg> = a
            .repos
            .iter()
            .filter(|r| r.visibility != crate::api::RepoVisibility::Visible)
            .map(|r| Msg::raw(r.repo.clone()))
            .collect();
        github.tone = Tone::Problem;
        github.text = msg!(
            CoreSetupGithubHidden,
            repos = ArgValue::joined(hidden, ", ")
        );
    }

    let model = {
        let (tone, text) = match (&s.check, s.endpoint.configured || s.endpoint.key) {
            (Some(p), _) => check_plain_text(p),
            (None, false) => (Tone::Open, msg!(CoreSetupModelNotSetUp)),
            (None, true) if !s.endpoint.key => (Tone::Problem, msg!(CoreSetupModelKeyMissing)),
            (None, true) if s.endpoint.model.is_none() => {
                (Tone::Problem, msg!(CoreSetupModelNameMissing))
            }
            (None, true) => (Tone::Open, msg!(CoreSetupModelUntested)),
        };
        // Name the chosen tool as soon as something is entered.
        let text = if s.endpoint.configured || s.endpoint.key {
            msg!(
                CoreSetupModelWithTool,
                tool = s.endpoint.provider.label(),
                text = text
            )
        } else {
            text
        };
        Line {
            name: msg!(CoreSetupNameModel),
            tone,
            text,
            step: Step::Model,
        }
    };

    let from = e.github_projects.tickets_from;
    let mut lines = vec![Line {
        name: msg!(CoreSetupNameContainer),
        tone: container.0,
        text: container.1,
        step: Step::Container,
    }];
    // Which source the tickets come from is a choice with a default (Jira), so it is never open.
    lines.push(Line {
        name: msg!(CoreSetupNameSource),
        tone: Tone::Good,
        text: match from {
            crate::TicketsFrom::Jira => msg!(CoreSetupSourceJira),
            crate::TicketsFrom::GitHubProjects => msg!(CoreSetupSourceProjects),
            crate::TicketsFrom::Both => msg!(CoreSetupSourceBoth),
        },
        step: Step::Source,
    });
    // Only the steps of the chosen source follow.
    if from.uses_jira() {
        lines.push(jira);
    }
    lines.push(github);
    if from.uses_projects() {
        lines.push(projects_line(s));
    }
    lines.push(model);
    lines
}

/// The GitHub Projects row: which projects the tickets come from.
fn projects_line(s: &Snapshot) -> Line {
    let p = &s.setup.github_projects;
    let (tone, text) = if !s.github.connected {
        (Tone::Open, msg!(CoreSetupProjectsNeedGithub))
    } else if p.projects.is_empty() {
        (Tone::Open, msg!(CoreSetupProjectsNone))
    } else {
        (
            Tone::Good,
            msg!(
                CoreSetupProjectsChosen,
                projects = ArgValue::joined(
                    p.projects
                        .iter()
                        .map(|n| Msg::raw(n.clone()))
                        .collect::<Vec<_>>(),
                    ", "
                )
            ),
        )
    };
    Line {
        name: msg!(CoreSetupNameProjects),
        tone,
        text,
        step: Step::Projects,
    }
}
