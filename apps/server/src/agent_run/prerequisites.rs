//! What a real run needs and the status that follows from it.

use cockpit_i18n::{ArgValue, Msg, msg};

use super::*;

/// What has to be there for a real run.
pub struct Prerequisites<'a> {
    pub container: &'a ContainerStatus,
    pub image: ImageState,
    /// Why the image state is `unknown`.
    pub image_hint: Option<Msg>,
    pub model: bool,
    pub key: bool,
    pub github_access: bool,
}

/// The sentence when the image may not be used for a run, `None` if it is current.
pub fn image_problem(state: ImageState, hint: Option<Msg>) -> Option<Msg> {
    match state {
        ImageState::Current => None,
        ImageState::Missing => Some(msg!(ServerAgentImageMissing)),
        ImageState::Outdated => Some(msg!(ServerAgentImageOutdated)),
        ImageState::Unknown => Some(msg!(
            ServerAgentImageUnknown,
            hint = hint.unwrap_or_else(|| msg!(ServerAgentImageUnknownHint))
        )),
    }
}

/// One sentence per missing prerequisite, with the place in the assistant ("Set up").
pub fn missing(v: &Prerequisites) -> Vec<Msg> {
    let mut f = Vec::new();
    match (&v.container.runtime, v.container.engine_running) {
        (None, _) => f.push(msg!(ServerAgentNoRuntime)),
        (Some(l), false) => f.push(msg!(ServerAgentRuntimeStopped, runtime = l)),
        (Some(_), true) => f.extend(image_problem(v.image, v.image_hint.clone())),
    }
    if !v.model || !v.key {
        let mut was = Vec::new();
        if !v.model {
            was.push(msg!(ServerAgentWhatModelName));
        }
        if !v.key {
            was.push(msg!(ServerAgentWhatKey));
        }
        f.push(msg!(
            ServerAgentModelAccessMissing,
            what = ArgValue::joined(was, ", ")
        ));
    }
    if !v.github_access {
        f.push(msg!(ServerAgentGithubMissingForAgent));
    }
    f
}

/// The repos of the agent and the default, from the configuration:
///
/// * `[[agent.repos]]` (`listed`) are the repos; `[agent] repo` (`default`) is added if it is
///   not listed.
/// * Without a list the default is the only repo; without a default too, the repos of
///   `[github] repos` are the choices. Without any of them the list is empty: the user picks
///   from every repo GitHub shows at the start, and the first use stores the repo here.
/// * Without a default, exactly one repo is the default.
pub fn repos_effective(
    listed: &[AgentRepo],
    default: Option<&str>,
    github: &[String],
) -> AgentRepos {
    let mut repos: Vec<AgentRepo> = listed.to_vec();
    if repos.is_empty()
        && let Some(d) = default
    {
        repos.push(AgentRepo::new(d));
    }
    if repos.is_empty() {
        repos = github.iter().map(|r| AgentRepo::new(r)).collect();
    }
    if let Some(d) = default
        && !repos.iter().any(|r| r.repo == d)
    {
        repos.push(AgentRepo::new(d));
    }
    let default = match default {
        Some(d) => Some(d.to_string()),
        None if repos.len() == 1 => Some(repos[0].repo.clone()),
        None => None,
    };
    AgentRepos { repos, default }
}

/// The status for `/api/health`.
pub fn status(v: &Prerequisites) -> AgentStatus {
    let missing = missing(v);
    AgentStatus {
        ready: missing.is_empty(),
        image: v.image != ImageState::Missing,
        image_state: v.image,
        image_hint: v.image_hint.clone(),
        image_name: IMAGE.into(),
        repo: None,
        repos: AgentRepos::default(),
        missing,
    }
}
