//! Detect the container runtime (Docker or Podman), only via the command line.
//!
//! Rules (see "Container rules" in `docs/architecture.md`): the backend calls `docker` or `podman` as a
//! process, never the socket. The choice is made in this order: `COCKPIT_CONTAINER`,
//! `[agent] container` in `config.toml`, otherwise first `podman`, then `docker` in the `PATH`. Whether the
//! engine runs is shown by `<cli> info`. Nothing is built or started here.

use std::ffi::OsStr;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use cockpit_core::api::ContainerStatus;
use cockpit_i18n::{Msg, msg};

/// This long `<cli> info` may take before it counts as "not running".
const INFO_TIME: Duration = Duration::from_secs(8);
/// This long a result applies before `/api/health` asks again (`info` can take seconds).
const VALID: Duration = Duration::from_secs(10);
/// Order of the search without an explicit choice.
const SEARCH: [&str; 2] = ["podman", "docker"];

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

/// The hint as plain German text and as a message.
fn hint(m: Msg) -> (Option<String>, Option<Msg>) {
    (Some(m.de()), Some(m))
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
            let (hint, hint_msg) = hint(not_found(&cli));
            return ContainerStatus {
                hint,
                hint_msg,
                runtime: Some(cli),
                engine_running: false,
                source: source.into(),
            };
        }
        let engine_running = running(&cli);
        let (hint, hint_msg) = if engine_running {
            (None, None)
        } else {
            hint(not_started(&cli))
        };
        return ContainerStatus {
            hint,
            hint_msg,
            runtime: Some(cli),
            engine_running,
            source: source.into(),
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
                hint_msg: None,
            };
        }
        first.get_or_insert(cli);
    }
    match first {
        Some(cli) => {
            let (hint, hint_msg) = hint(not_started(cli));
            ContainerStatus {
                runtime: Some(cli.into()),
                engine_running: false,
                source: "detected".into(),
                hint,
                hint_msg,
            }
        }
        None => {
            let (hint, hint_msg) = hint(msg!(ServerRunNoCliAtAll));
            ContainerStatus {
                runtime: None,
                engine_running: false,
                source: "none".into(),
                hint,
                hint_msg,
            }
        }
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

/// Detects the runtime with real process calls. Blocking.
pub fn detect(config: Option<&str>, env: Option<&str>, path: &OsStr) -> ContainerStatus {
    choose(
        config,
        env,
        |cli| in_path(cli, path).is_some(),
        |cli| in_path(cli, path).is_some_and(|p| info_succeeds(&p)),
    )
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
