//! "Clean up": what no running run needs any more. First a preview, then – after the user's
//! confirmation – only what the preview named *and* is still an orphan.
//!
//! * Containers carrying the run label (`cockpit.run`) that belong to no run that works. Only
//!   `ps --filter label=…` and `rm -f` on the command line, never the socket.
//! * Folders under `<agent dir>/runs` that no active run uses.
//!
//! Never the image, never a container without the label, never anything outside `runs`.

use cockpit_core::api::{CleanupPreview, CleanupRequest, CleanupResult, OrphanContainer};

use super::*;

impl AppState {
    /// The folder names under `runs` (real folders only) that no active run needs.
    fn orphan_folders(&self, keep: &HashSet<String>) -> Vec<String> {
        let Ok(entries) = std::fs::read_dir(self.agent_dir().join("runs")) else {
            return Vec::new();
        };
        let mut v: Vec<String> = entries
            .flatten()
            .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| !keep.contains(n))
            .collect();
        v.sort();
        v
    }

    /// Folder names and container-name prefixes of everything that is still in use.
    fn in_use(&self) -> (HashSet<String>, Vec<String>) {
        let ws = self.inner.live.copy();
        let mut keys: HashSet<String> = lock(&self.inner.agent.running).keys().cloned().collect();
        keys.extend(ws.runs.iter().filter(|r| r.is_active()).map(|r| r.key()));
        let folders = keys.iter().map(|k| agent_run::run_dir(k)).collect();
        let prefixes = keys
            .iter()
            .map(|k| agent_run::container_name_prefix(k))
            .collect();
        (folders, prefixes)
    }

    /// Containers with the run label that no working run owns. `None` if the container program
    /// cannot be asked. Blocking.
    fn orphan_containers(&self, prefixes: &[String]) -> Option<Vec<OrphanContainer>> {
        let status = self.container_status();
        let cli = self.cli_path(&status).filter(|_| status.engine_running)?;
        let out = std::process::Command::new(cli)
            .args([
                "ps",
                "-a",
                "--filter",
                &format!("label={LABEL}"),
                "--format",
            ])
            .arg("{{.ID}} {{.Names}}")
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .ok()?;
        Some(
            String::from_utf8_lossy(&out.stdout)
                .lines()
                .filter_map(|l| {
                    let mut p = l.split_whitespace();
                    Some(OrphanContainer {
                        id: p.next()?.to_string(),
                        name: p.next().unwrap_or_default().to_string(),
                    })
                })
                .filter(|c| !prefixes.iter().any(|p| c.name.starts_with(p.as_str())))
                .collect(),
        )
    }

    /// What "Clean up" would remove. Changes nothing. Blocking.
    pub(crate) fn cleanup_preview(&self) -> CleanupPreview {
        let (folders, prefixes) = self.in_use();
        let containers = self.orphan_containers(&prefixes);
        CleanupPreview {
            engine_missing: containers.is_none(),
            containers: containers.unwrap_or_default(),
            folders: self.orphan_folders(&folders),
        }
    }

    /// Removes what the request confirms and what is still an orphan right now. Blocking.
    pub(crate) fn cleanup_apply(&self, confirmed: &CleanupRequest) -> CleanupResult {
        let now = self.cleanup_preview();
        let ids: Vec<&str> = now
            .containers
            .iter()
            .map(|c| c.id.as_str())
            .filter(|id| confirmed.containers.iter().any(|c| c == id))
            .collect();
        if !ids.is_empty() {
            let status = self.container_status();
            if let Some(cli) = self.cli_path(&status) {
                let _ = std::process::Command::new(cli)
                    .args(["rm", "-f"])
                    .args(&ids)
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status();
            }
        }
        let runs = self.agent_dir().join("runs");
        let mut folders = 0;
        for name in now.folders.iter().filter(|f| confirmed.folders.contains(f)) {
            if std::fs::remove_dir_all(runs.join(name)).is_ok() {
                folders += 1;
            }
        }
        CleanupResult {
            containers: ids.len(),
            folders,
        }
    }
}
