//! What a real run needs and the status that follows from it.

use super::*;

/// What has to be there for a real run.
pub struct Prerequisites<'a> {
    pub container: &'a ContainerStatus,
    pub image: ImageState,
    /// Why the image state is `unknown`.
    pub image_hint: Option<String>,
    pub model: bool,
    pub key: bool,
    /// The repo in which the agent works; `Err` with the explanation if it is not unambiguous.
    pub repo: Result<&'a str, String>,
    pub github_access: bool,
}

/// The sentence when the image may not be used for a run, `None` if it is current.
pub fn image_problem(state: ImageState, hint: Option<&str>) -> Option<String> {
    match state {
        ImageState::Current => None,
        ImageState::Missing => Some(
            "Das Agent-Image fehlt. Öffne „Einrichten“ › „Container“ und klicke auf „Image bauen“."
                .to_string(),
        ),
        ImageState::Outdated => Some(
            "Das Agent-Image ist veraltet. Bitte unter „Einrichten“ › „Container“ neu bauen."
                .to_string(),
        ),
        ImageState::Unknown => Some(format!(
            "Ob das Agent-Image aktuell ist, lässt sich nicht prüfen. {}",
            hint.unwrap_or("Prüfe den Schritt „Container“ unter „Einrichten“.")
        )),
    }
}

/// One sentence in plain text per missing prerequisite, with the place in the assistant ("Einrichten").
pub fn missing(v: &Prerequisites) -> Vec<String> {
    let mut f = Vec::new();
    match (&v.container.runtime, v.container.engine_running) {
        (None, _) => f.push(
            "Es gibt keine Container-Laufzeit. Öffne „Einrichten“ › „Container“ und installiere Rancher Desktop (dockerd) oder Podman."
                .to_string(),
        ),
        (Some(l), false) => f.push(format!(
            "{l} läuft nicht. Starte es (Rancher Desktop bzw. „podman machine start“) und prüfe es unter „Einrichten“ › „Container“."
        )),
        (Some(_), true) => f.extend(image_problem(v.image, v.image_hint.as_deref())),
    }
    if !v.model || !v.key {
        let mut was = Vec::new();
        if !v.model {
            was.push("der Modellname");
        }
        if !v.key {
            was.push("der Key");
        }
        f.push(format!(
            "Beim Modellzugang fehlt etwas ({}). Trage es unter „Einrichten“ › „Modell“ ein.",
            was.join(", ")
        ));
    }
    if let Err(m) = &v.repo {
        f.push(m.clone());
    }
    if !v.github_access {
        f.push(
            "GitHub ist nicht verbunden (der Agent braucht es zum Klonen und Pushen). Melde dich unter „Einrichten“ › „GitHub“ an."
                .to_string(),
        );
    }
    f
}

/// The sentence when no repo is set up for the agent.
pub const NO_REPO: &str = "Es ist kein Repo eingetragen, in dem der Agent arbeiten soll. Trage es unter „Einrichten“ › „GitHub“ ein.";

/// The repos of the agent and the default, from the configuration:
///
/// * `[[agent.repos]]` (`listed`) are the repos; `[agent] repo` (`default`) is added if it is
///   not listed.
/// * Without a list the default is the only repo; without a default too, the repos of
///   `[github] repos` are the choices (the user picks at the start).
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
pub fn status(real: bool, v: &Prerequisites) -> AgentStatus {
    if !real {
        return AgentStatus {
            mode: "simulated".into(),
            ready: true,
            image: v.image != ImageState::Missing,
            image_state: v.image,
            image_hint: v.image_hint.clone(),
            image_name: IMAGE.into(),
            repo: None,
            repos: AgentRepos::default(),
            missing: Vec::new(),
        };
    }
    let missing = missing(v);
    AgentStatus {
        mode: "real".into(),
        ready: missing.is_empty(),
        image: v.image != ImageState::Missing,
        image_state: v.image,
        image_hint: v.image_hint.clone(),
        image_name: IMAGE.into(),
        repo: v.repo.as_ref().ok().map(|r| (*r).to_string()),
        repos: AgentRepos::default(),
        missing,
    }
}
