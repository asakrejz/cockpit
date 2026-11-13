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

fn not_found(cli: &str) -> String {
    format!(
        "„{cli}“ steht nicht im PATH. Installiere es (Docker: Rancher Desktop mit Engine „dockerd (moby)“; Podman: brew install podman) oder wähle die andere Laufzeit."
    )
}

fn not_started(cli: &str) -> String {
    match cli {
        "podman" => "„podman info“ schlägt fehl. Starte die Maschine mit „podman machine start“.".into(),
        _ => "„docker info“ schlägt fehl. Starte Rancher Desktop und stelle die Engine auf „dockerd (moby)“ (nicht „containerd“).".into(),
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
            };
        }
        let engine_running = running(&cli);
        return ContainerStatus {
            hint: (!engine_running).then(|| not_started(&cli)),
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
        },
        None => ContainerStatus {
            runtime: None,
            engine_running: false,
            source: "none".into(),
            hint: Some(
                "Weder podman noch docker im PATH. Installiere Rancher Desktop (Engine „dockerd (moby)“) oder Podman."
                    .into(),
            ),
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
mod tests {
    use super::*;

    fn all_ok(_: &str) -> bool {
        true
    }
    fn no(_: &str) -> bool {
        false
    }

    #[test]
    fn without_choice_comes_podman_before_docker() {
        let s = choose(None, None, all_ok, all_ok);
        assert_eq!(s.runtime.as_deref(), Some("podman"));
        assert!(s.engine_running && s.hint.is_none());
        assert_eq!(s.source, "detected");
    }

    #[test]
    fn running_podman_not_is_docker_taken() {
        let s = choose(None, None, all_ok, |cli| cli == "docker");
        assert_eq!(s.runtime.as_deref(), Some("docker"));
        assert!(s.engine_running);
        let s = choose(None, None, |cli| cli == "docker", all_ok);
        assert_eq!(s.runtime.as_deref(), Some("docker"));
    }

    #[test]
    fn present_but_without_engine_gives_a_hint() {
        let s = choose(None, None, all_ok, no);
        assert_eq!(s.runtime.as_deref(), Some("podman"), "die erste gefundene");
        assert!(!s.engine_running);
        assert!(s.hint.unwrap().contains("podman machine start"));
        let s = choose(None, None, |c| c == "docker", no);
        assert!(s.hint.unwrap().contains("dockerd (moby)"));
    }

    #[test]
    fn without_both_gives_it_no_runtime() {
        let s = choose(None, None, no, no);
        assert_eq!(s.runtime, None);
        assert_eq!(s.source, "none");
        assert!(s.hint.unwrap().contains("Weder podman noch docker"));
    }

    #[test]
    fn explicit_choice_goes_before_and_env_before_file() {
        let s = choose(Some("podman"), Some("docker"), all_ok, all_ok);
        assert_eq!(s.runtime.as_deref(), Some("docker"));
        assert_eq!(s.source, "env");
        let s = choose(Some("docker"), None, all_ok, all_ok);
        assert_eq!(s.runtime.as_deref(), Some("docker"));
        assert_eq!(s.source, "config");
        // Chosen means chosen: it does not fall back to the other runtime.
        let s = choose(Some("docker"), None, all_ok, |cli| cli == "podman");
        assert_eq!(s.runtime.as_deref(), Some("docker"));
        assert!(!s.engine_running);
        let s = choose(Some("docker"), None, |cli| cli == "podman", all_ok);
        assert!(s.hint.unwrap().contains("nicht im PATH"));
    }

    #[cfg(unix)]
    mod process {
        use super::*;
        use std::os::unix::fs::PermissionsExt;

        fn program(dir: &std::path::Path, name: &str, script: &str) {
            let file = dir.join(name);
            std::fs::write(&file, format!("#!/bin/sh\n{script}\n")).unwrap();
            std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755)).unwrap();
        }

        #[test]
        fn calls_info_on_and_evaluates_the_status() {
            let dir = tempfile::tempdir().unwrap();
            program(dir.path(), "podman", "[ \"$1\" = info ] && exit 1; exit 2");
            program(dir.path(), "docker", "[ \"$1\" = info ] && exit 0; exit 2");
            let s = detect(None, None, dir.path().as_os_str());
            assert_eq!(s.runtime.as_deref(), Some("docker"));
            assert!(s.engine_running);
            let s = detect(Some("podman"), None, dir.path().as_os_str());
            assert_eq!(s.runtime.as_deref(), Some("podman"));
            assert!(!s.engine_running);
        }

        #[test]
        fn a_file_without_exec_permission_counts_not() {
            let dir = tempfile::tempdir().unwrap();
            std::fs::write(dir.path().join("docker"), "#!/bin/sh\nexit 0\n").unwrap();
            let s = detect(None, None, dir.path().as_os_str());
            assert_eq!(s.runtime, None);
        }
    }

    #[test]
    fn cache_asks_only_once() {
        let store = Cache::default();
        let mut counter = 0;
        for _ in 0..3 {
            store.fetch(|| {
                counter += 1;
                choose(None, None, no, no)
            });
        }
        assert_eq!(counter, 1);
    }
}
