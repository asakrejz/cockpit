//! Detect the container runtime (Docker or Podman), only via the command line.
//!
//! Rules: the backend calls `docker` or `podman` as a process, never the socket. The choice is made
//! in this order: `COCKPIT_CONTAINER`, `[agent] container` in `config.toml`, otherwise first `docker`,
//! then `podman` in the `PATH`. Whether the engine runs is shown by `<cli> info`. Nothing is built or
//! started here.

use std::ffi::OsStr;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use cockpit_core::api::{ContainerStatus, SocketState};
use cockpit_i18n::{Msg, msg};

/// This long `<cli> info` may take before it counts as "not running".
const INFO_TIME: Duration = Duration::from_secs(8);
/// This long a result applies before `/api/health` asks again (`info` can take seconds).
const VALID: Duration = Duration::from_secs(10);
/// Order of the search without an explicit choice.
const SEARCH: [&str; 2] = ["docker", "podman"];

/// The choice from environment and file; the environment goes first.
fn chosen(config: Option<&str>, env: Option<&str>) -> Option<(String, &'static str)> {
    env.map(|l| (l.to_string(), "env"))
        .or_else(|| config.map(|l| (l.to_string(), "config")))
}

fn not_found(cli: &str) -> Msg {
    msg!(ServerRunCliNotInPath, cli = cli)
}

fn not_started(cli: &str) -> Msg {
    match cli {
        "podman" => msg!(ServerRunPodmanNotRunning),
        _ => msg!(ServerRunDockerNotRunning),
    }
}

/// The decision as a pure function: `present(cli)` says whether the program is in the `PATH`,
/// `running(cli)` whether `<cli> info` succeeds.
pub fn choose(
    config: Option<&str>,
    env: Option<&str>,
    present: impl Fn(&str) -> bool,
    running: impl Fn(&str) -> bool,
) -> ContainerStatus {
    if let Some((cli, source)) = chosen(config, env) {
        if !present(&cli) {
            return ContainerStatus {
                hint: Some(not_found(&cli)),
                runtime: Some(cli),
                engine_running: false,
                source: source.into(),
                ..Default::default()
            };
        }
        let engine_running = running(&cli);
        return ContainerStatus {
            hint: (!engine_running).then(|| not_started(&cli)),
            runtime: Some(cli),
            engine_running,
            source: source.into(),
            ..Default::default()
        };
    }
    let mut first = None;
    for cli in SEARCH {
        if !present(cli) {
            continue;
        }
        if running(cli) {
            return ContainerStatus {
                runtime: Some(cli.into()),
                engine_running: true,
                source: "detected".into(),
                hint: None,
                ..Default::default()
            };
        }
        first.get_or_insert(cli);
    }
    match first {
        Some(cli) => ContainerStatus {
            runtime: Some(cli.into()),
            engine_running: false,
            source: "detected".into(),
            hint: Some(not_started(cli)),
            ..Default::default()
        },
        None => ContainerStatus {
            runtime: None,
            engine_running: false,
            source: "none".into(),
            hint: Some(msg!(ServerRunNoCliAtAll)),
            ..Default::default()
        },
    }
}

/// Searches an executable program in the folders of `path`.
pub fn in_path(name: &str, path: &OsStr) -> Option<PathBuf> {
    std::env::split_paths(path)
        .map(|dir| dir.join(name))
        .find(|file| executable(file))
}

#[cfg(unix)]
fn executable(file: &std::path::Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    file.metadata()
        .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn executable(file: &std::path::Path) -> bool {
    file.is_file()
}

/// `<program> info` without output; succeeds if it ends with status 0. After `INFO_TIME` the
/// process is ended. Blocking.
fn info_succeeds(program: &std::path::Path) -> bool {
    let Ok(mut kind) = Command::new(program)
        .arg("info")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return false;
    };
    let start = Instant::now();
    loop {
        match kind.try_wait() {
            Ok(Some(status)) => return status.success(),
            Ok(None) if start.elapsed() < INFO_TIME => {
                std::thread::sleep(Duration::from_millis(50));
            }
            _ => {
                let _ = kind.kill();
                let _ = kind.wait();
                return false;
            }
        }
    }
}

/// Whether Cockpit runs in its own image: the variable from the `Containerfile` first, then the
/// marker files of Docker (`/.dockerenv`) and Podman (`/run/.containerenv`).
pub fn is_in_container(env: Option<&str>, marker_file: bool) -> bool {
    match env.map(str::trim) {
        Some("" | "0") => marker_file,
        Some(_) => true,
        None => marker_file,
    }
}

/// The socket as a pure function: `exists` says a socket file is there, `connect` is the error of
/// the connection attempt (`None` = it worked).
pub fn socket_state(exists: bool, connect: Option<std::io::ErrorKind>) -> SocketState {
    match (exists, connect) {
        (false, _) => SocketState::Missing,
        (true, None) => SocketState::Reachable,
        (true, Some(std::io::ErrorKind::PermissionDenied)) => SocketState::Denied,
        // A stale file or a dead engine behind it: nothing the user can use.
        (true, Some(_)) => SocketState::Missing,
    }
}

/// Inside the Cockpit container, a missing engine is the normal case: replace the hint of
/// "not installed / not running" by one that says what the container needs. Pure.
pub fn in_container_view(
    mut status: ContainerStatus,
    in_container: bool,
    socket: SocketState,
) -> ContainerStatus {
    status.in_container = in_container;
    status.socket = if in_container {
        socket
    } else {
        SocketState::Missing
    };
    if in_container && !status.engine_running {
        status.hint = Some(match socket {
            SocketState::Denied => msg!(ServerRunInContainerSocketDenied),
            _ => msg!(ServerRunInContainerNoSocket),
        });
    }
    status
}

/// The socket the `docker` CLI would use: `DOCKER_HOST=unix://…`, otherwise the default path.
#[cfg(unix)]
fn probe_socket() -> SocketState {
    let path = std::env::var("DOCKER_HOST")
        .ok()
        .and_then(|h| h.strip_prefix("unix://").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("/var/run/docker.sock"));
    let exists = path.exists();
    let connect = exists
        .then(|| std::os::unix::net::UnixStream::connect(&path).err())
        .flatten()
        .map(|e| e.kind());
    socket_state(exists, connect)
}

#[cfg(not(unix))]
fn probe_socket() -> SocketState {
    SocketState::Missing
}

/// Detects the runtime with real process calls. Blocking.
pub fn detect(config: Option<&str>, env: Option<&str>, path: &OsStr) -> ContainerStatus {
    let status = choose(
        config,
        env,
        |cli| in_path(cli, path).is_some(),
        |cli| in_path(cli, path).is_some_and(|p| info_succeeds(&p)),
    );
    let marker = std::path::Path::new("/.dockerenv").exists()
        || std::path::Path::new("/run/.containerenv").exists();
    let in_container = is_in_container(
        std::env::var("COCKPIT_IN_CONTAINER").ok().as_deref(),
        marker,
    );
    let socket = if in_container {
        probe_socket()
    } else {
        SocketState::Missing
    };
    in_container_view(status, in_container, socket)
}

/// Remembers the result briefly so that `/api/health` does not start `info` on every call.
#[derive(Default)]
pub struct Cache {
    snapshot: Mutex<Option<(Instant, ContainerStatus)>>,
}

impl Cache {
    /// The remembered result or a fresh one. Blocking.
    pub fn fetch(&self, fresh: impl FnOnce() -> ContainerStatus) -> ContainerStatus {
        let mut snapshot = self.snapshot.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((time, status)) = snapshot.as_ref()
            && time.elapsed() < VALID
        {
            return status.clone();
        }
        let updated = fresh();
        *snapshot = Some((Instant::now(), updated.clone()));
        updated
    }
}

#[cfg(test)]
mod tests;
