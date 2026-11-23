//! The container command of a run: arguments, environment and names.

use super::*;

/// How the folders of a run get into the agent container.
///
/// * `Bind`: `-v <host folder>:/work` and `/job` (native start; the engine resolves the paths on the same machine).
/// * `Copy`: no mounts. The container is created, the clone and the job are copied in with `cp`, it is
///   started, and the clone is copied out again afterwards. Needed when Cockpit itself runs in a container:
///   the engine would resolve a path of the Cockpit container on the host, where it does not exist.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AgentIo {
    #[default]
    Bind,
    Copy,
}

impl AgentIo {
    /// `COCKPIT_AGENT_IO=bind|copy` decides; without it a run inside the Cockpit container copies.
    pub fn choose(env: Option<&str>, in_container: bool) -> Self {
        match env.map(|e| e.trim().to_ascii_lowercase()).as_deref() {
            Some("bind") => Self::Bind,
            Some("copy") => Self::Copy,
            _ if in_container => Self::Copy,
            _ => Self::Bind,
        }
    }
}

/// Where the clone and the job live inside the agent container.
pub const WORK_DIR: &str = "/work";
pub const JOB_DIR: &str = "/job";

/// Everything `docker run` / `podman run` (or `create`) needs for a run.
pub struct RunCommand<'a> {
    pub name: &'a str,
    /// Value of the label `cockpit.run` (the key of the run).
    pub run: &'a str,
    pub clone: &'a Path,
    /// Read only (plan mode): the container does not change the clone.
    pub clone_only_read: bool,
    pub job: &'a Path,
    /// With open stdin (`-i`) for answers and hints.
    pub stdin_open: bool,
    /// The tool of the agent: selects the runner's adapter and which endpoint names `-e` passes on.
    pub provider: Provider,
    pub io: AgentIo,
}

/// The arguments after `docker`/`podman`. Never contains a secret: the runner gets address,
/// key and model as `-e NAME` and reads the values from the environment of the child process
/// ([`env_for_kind`]). `Bind`: `run --rm` with mounts. `Copy`: `create` without mounts and without
/// `--rm` (the clone is copied out after the end; the backend removes the container itself).
pub fn run_args(b: &RunCommand) -> Vec<String> {
    let s = |x: &str| x.to_string();
    let mut a = match b.io {
        AgentIo::Bind => vec![s("run"), s("--rm")],
        AgentIo::Copy => vec![s("create")],
    };
    if b.stdin_open {
        a.push(s("-i"));
    }
    a.extend([
        s("--name"),
        s(b.name),
        s("--label"),
        format!("{LABEL}={}", b.run),
        s("--add-host"),
        format!("{HOST_NAME}:host-gateway"),
        s("--cap-drop"),
        s("ALL"),
        s("--security-opt"),
        s("no-new-privileges"),
        s("--pids-limit"),
        s("1024"),
        s("--memory"),
        s("4g"),
    ]);
    if b.io == AgentIo::Bind {
        a.extend([
            s("-v"),
            format!(
                "{}:{WORK_DIR}{}",
                b.clone.display(),
                if b.clone_only_read { ":ro" } else { "" }
            ),
            s("-v"),
            format!("{}:{JOB_DIR}:ro", b.job.display()),
        ]);
    }
    for name in kind_env_names(b.provider) {
        a.extend([s("-e"), s(name)]);
    }
    a.push(s(IMAGE));
    a
}

/// `docker start` for a container made with `create`: attached (output of the runner), with stdin if the
/// container was created with `-i`.
pub fn start_args(name: &str, stdin_open: bool) -> Vec<String> {
    let mut a = vec!["start".to_string(), "-a".to_string()];
    if stdin_open {
        a.push("-i".into());
    }
    a.push(name.into());
    a
}

/// `docker cp <folder>/. <name>:<target>`: the contents of the folder into the (existing) target folder.
pub fn copy_in_args(folder: &Path, name: &str, target: &str) -> Vec<String> {
    vec![
        "cp".into(),
        format!("{}/.", folder.display()),
        format!("{name}:{target}"),
    ]
}

/// `docker cp <name>:/work/. <folder>`: everything the agent left in `/work` into the folder.
pub fn copy_out_args(name: &str, folder: &Path) -> Vec<String> {
    vec![
        "cp".into(),
        format!("{name}:{WORK_DIR}/."),
        folder.display().to_string(),
    ]
}

/// The names that `docker run` passes on via `-e NAME` (only the name, never a value) for a tool:
/// the Anthropic names for the Claude Agent SDK and Claude Code, the OpenAI names (and the model) for Codex.
pub fn kind_env_names(provider: Provider) -> Vec<&'static str> {
    let mut names = vec!["COCKPIT_PROVIDER"];
    names.extend(if provider.openai_compatible() {
        ["OPENAI_BASE_URL", "OPENAI_API_KEY", "COCKPIT_MODEL"].as_slice()
    } else {
        [
            "ANTHROPIC_BASE_URL",
            "ANTHROPIC_AUTH_TOKEN",
            "ANTHROPIC_MODEL",
            "ANTHROPIC_API_KEY",
        ]
        .as_slice()
    });
    names
}

/// The environment of the `docker run` process: the values that `-e NAME` passes on are there.
/// `Some` sets the value, `None` removes it from the inherited environment so that no key from the
/// person's shell ever travels into the container. Names of the other tool family are always removed.
///
/// * Claude Agent SDK and Claude Code, with an address (proxy such as LiteLLM, another compatible endpoint):
///   `ANTHROPIC_BASE_URL` and `ANTHROPIC_AUTH_TOKEN` (Bearer). Without an address (directly at Anthropic):
///   `ANTHROPIC_API_KEY` (`x-api-key`), no base address.
/// * Codex: `OPENAI_API_KEY` (Bearer) always, `OPENAI_BASE_URL` with an address (without one directly at OpenAI),
///   the model in `COCKPIT_MODEL`.
pub fn env_for_kind(
    provider: Provider,
    endpoint_url: Option<&str>,
    key: &str,
    model: &str,
) -> Vec<(&'static str, Option<String>)> {
    let base = endpoint_url
        .map(str::trim)
        .filter(|u| !u.is_empty())
        .map(for_container);
    let mut out: Vec<(&'static str, Option<String>)> =
        vec![("COCKPIT_PROVIDER", Some(provider.id().to_string()))];
    if provider.openai_compatible() {
        out.extend([
            ("ANTHROPIC_BASE_URL", None),
            ("ANTHROPIC_AUTH_TOKEN", None),
            ("ANTHROPIC_MODEL", None),
            ("ANTHROPIC_API_KEY", None),
            ("OPENAI_BASE_URL", base),
            ("OPENAI_API_KEY", Some(key.to_string())),
            ("COCKPIT_MODEL", Some(model.to_string())),
        ]);
    } else {
        let proxy = base.is_some();
        out.extend([
            ("OPENAI_BASE_URL", None),
            ("OPENAI_API_KEY", None),
            ("COCKPIT_MODEL", None),
            ("ANTHROPIC_BASE_URL", base),
            ("ANTHROPIC_AUTH_TOKEN", proxy.then(|| key.to_string())),
            ("ANTHROPIC_MODEL", Some(model.to_string())),
            ("ANTHROPIC_API_KEY", (!proxy).then(|| key.to_string())),
        ]);
    }
    out
}

/// A proxy on the own machine (`localhost`, `127.0.0.1`) is called `cockpit-host` in the container.
pub fn for_container(url: &str) -> String {
    let url = url.trim().trim_end_matches('/');
    let Some((schema, rest)) = url.split_once("://") else {
        return url.to_string();
    };
    let (host_port, path) = match rest.find('/') {
        Some(i) => rest.split_at(i),
        None => (rest, ""),
    };
    let (host, port) = match host_port.rsplit_once(':') {
        Some((h, p)) if p.chars().all(|c| c.is_ascii_digit()) && !h.ends_with(']') => (h, Some(p)),
        Some((h, p)) if h.ends_with(']') => (h, Some(p)),
        _ => (host_port, None),
    };
    if matches!(host, "localhost" | "127.0.0.1" | "[::1]" | "0.0.0.0") {
        let port = port.map(|p| format!(":{p}")).unwrap_or_default();
        return format!("{schema}://{HOST_NAME}{port}{path}");
    }
    url.to_string()
}

/// The part of the container name that belongs to the run (without the time).
pub fn container_name_prefix(run: &str) -> String {
    let clean: String = run
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '-'
            }
        })
        .take(40)
        .collect();
    format!("cockpit-run-{clean}-")
}

/// A name for the container: only allowed characters, unique through the time.
pub fn container_name(run: &str, now_ms: u64) -> String {
    format!("{}{now_ms}", container_name_prefix(run))
}

/// A folder name for the run (the key contains characters such as `#`).
pub fn run_dir(run: &str) -> String {
    run.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect()
}
