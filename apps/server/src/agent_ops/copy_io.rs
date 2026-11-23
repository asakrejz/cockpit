//! Strategy "copy": the agent container gets its folders without bind mounts.
//!
//! Why: with `-v <path>:/work` the engine resolves `<path>` on its own machine. When Cockpit runs in a
//! container, that path is one of the *Cockpit container* and does not exist on the host. So the
//! container is created (`create`), the clone and the job are copied in (`cp`), it is started
//! (`start -a -i`), and the clone is copied out again (`cp`) before the container is removed. Only
//! the `docker` command is used, never the socket; the rules for the container stay as they are
//! (label, no token inside, `rm -f` at the end, push only by the backend after its own checks).

use std::fs;
use std::io;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::time::Duration;

use super::*;
use agent_run::{JOB_DIR, WORK_DIR, copy_in_args, copy_out_args};

/// Longest time for `create` or one `cp`.
const STEP_TIME: Duration = Duration::from_secs(120);

/// Changes the mode of every file and folder below `dir` (symlinks are left alone).
fn chmod_walk(dir: &Path, change: fn(u32, bool) -> u32) -> io::Result<()> {
    #[cfg(unix)]
    {
        let meta = fs::symlink_metadata(dir)?;
        if meta.file_type().is_symlink() {
            return Ok(());
        }
        let mode = meta.permissions().mode() & 0o7777;
        let new = change(mode, meta.is_dir());
        if new != mode {
            fs::set_permissions(dir, fs::Permissions::from_mode(new))?;
        }
        if meta.is_dir() {
            for e in fs::read_dir(dir)? {
                chmod_walk(&e?.path(), change)?;
            }
        }
    }
    #[cfg(not(unix))]
    let _ = (dir, change);
    Ok(())
}

/// The agent inside the container runs as another user than the one that owns the copy (`cp` into a
/// container makes root the owner). So everything below the clone gets `a+rwX` before it is copied in.
/// The copy lives only in the throw-away container; [`tighten`] takes it back after the copy-out.
pub(super) fn open_up(dir: &Path) -> io::Result<()> {
    chmod_walk(dir, |m, is_dir| {
        m | 0o666 | if is_dir || m & 0o111 != 0 { 0o111 } else { 0 }
    })
}

/// Removes write access for group and others again (what came back out of the container).
pub(super) fn tighten(dir: &Path) -> io::Result<()> {
    chmod_walk(dir, |m, _| m & !0o022)
}

/// Runs `<cli> <args>`; `Err` with the last line of the error output.
async fn cli_step(cli: &Path, args: &[String]) -> Result<(), String> {
    let mut cmd = Command::new(cli);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    match tokio::time::timeout(STEP_TIME, cmd.output()).await {
        Err(_) => Err("timeout".into()),
        Ok(Err(e)) => Err(e.to_string()),
        Ok(Ok(out)) if out.status.success() => Ok(()),
        Ok(Ok(out)) => Err(last_line(&String::from_utf8_lossy(&out.stderr), &[])
            .unwrap_or_else(|| format!("exit code {}", out.status.code().unwrap_or(-1)))),
    }
}

/// `create` (with the environment for `-e NAME` already set on `create`), then the clone to `/work`
/// and the job to `/job`. On any failure the container is removed again.
pub(super) async fn prepare(
    cli: &Path,
    mut create: Command,
    name: &str,
    clone: &Path,
    job: &Path,
) -> Result<(), Msg> {
    create
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    match tokio::time::timeout(STEP_TIME, create.output()).await {
        Err(_) => return Err(msg!(ServerRunContainerNotStarted, error = "timeout")),
        Ok(Err(e)) => {
            return Err(msg!(
                ServerRunContainerNotStartedHint,
                error = e.to_string()
            ));
        }
        Ok(Ok(out)) if !out.status.success() => {
            let line = last_line(&String::from_utf8_lossy(&out.stderr), &[])
                .unwrap_or_else(|| format!("exit code {}", out.status.code().unwrap_or(-1)));
            return Err(msg!(ServerRunContainerNotStartedHint, error = line));
        }
        Ok(Ok(_)) => {}
    }
    let c = clone.to_path_buf();
    let opened = tokio::task::spawn_blocking(move || open_up(&c)).await;
    let result = match opened {
        Ok(Ok(())) => {
            let mut r = cli_step(cli, &copy_in_args(clone, name, WORK_DIR)).await;
            if r.is_ok() {
                r = cli_step(cli, &copy_in_args(job, name, JOB_DIR)).await;
            }
            r
        }
        Ok(Err(e)) => Err(e.to_string()),
        Err(_) => Err("internal error".into()),
    };
    if let Err(e) = result {
        container_remove(cli, name).await;
        return Err(msg!(ServerRunCopyInFailed, error = e));
    }
    Ok(())
}

/// Copies `/work` of the container back into `clone`. Into a fresh folder first and then swapped in,
/// so that a file the agent deleted is gone in the clone too, and a failed copy leaves the old clone.
pub(super) async fn fetch(cli: &Path, name: &str, clone: &Path) -> Result<(), Msg> {
    let mut out = clone.as_os_str().to_owned();
    out.push(".out");
    let out = PathBuf::from(out);
    let o = out.clone();
    let _ = tokio::task::spawn_blocking(move || {
        let _ = fs::remove_dir_all(&o);
        fs::create_dir_all(&o)
    })
    .await;
    let result = cli_step(cli, &copy_out_args(name, &out)).await;
    let (c, o) = (clone.to_path_buf(), out.clone());
    let swapped = tokio::task::spawn_blocking(move || -> Result<(), String> {
        result?;
        tighten(&o).map_err(|e| e.to_string())?;
        fs::remove_dir_all(&c).map_err(|e| e.to_string())?;
        fs::rename(&o, &c).map_err(|e| e.to_string())
    })
    .await
    .unwrap_or_else(|_| Err("internal error".into()));
    if let Err(e) = swapped {
        let _ = fs::remove_dir_all(&out);
        return Err(msg!(ServerRunCopyOutFailed, error = e));
    }
    Ok(())
}
