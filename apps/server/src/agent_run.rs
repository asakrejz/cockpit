//! The real agent, pure part: protocol of the runner, command line for `docker`/`podman`, the
//! job for the container, the prerequisites and the plan of what the watcher does next.
//! Nothing here starts a process or touches the disk – that is done by `agent_ops`.
//!
//! Rules for containers (see "Container rules" in `docs/architecture.md`): only commands and switches that
//! Docker and Podman understand the same way; secrets only as `-e NAME` (the value is in the
//! environment of the child process, never in the argument list); `--rm`; a label per run for cleaning up;
//! `--add-host cockpit-host:host-gateway` to reach services on the machine.

use std::collections::HashMap;
use std::path::Path;

use cockpit_core::api::{AgentStatus, ContainerStatus, ImageState, Provider};
use cockpit_core::{
    AgentRepo, AgentReport, AgentRepos, AgentRun, DiffSide, RunKind, RunState, Workspace,
};
use cockpit_integrations::github::followup;
use serde::Deserialize;
use serde_json::{Value, json};

pub use crate::agent_image::IMAGE;

mod events;
pub use events::*;

mod command;
pub use command::*;

mod job;
pub use job::*;

mod prerequisites;
pub use prerequisites::*;

mod plane;
pub use plane::*;
/// Label by which the backend finds its containers again and cleans up.
pub const LABEL: &str = "cockpit.run";
/// Name under which the container reaches the machine.
pub const HOST_NAME: &str = "cockpit-host";
/// Longer lines of the runner are discarded by the backend.
pub const MAX_LINE: usize = 64 * 1024;
const MAX_TEXT: usize = 2_000;
const MAX_LIST: usize = 200;
/// No diff has more lines per file than this.
const MAX_LINE_NO: u32 = 10_000_000;
/// No single step credibly reports more tokens or time.
const MAX_TOKEN: u64 = 50_000_000;
const MAX_MS: u64 = 3_600_000;

// ---------- Protocol: lines of the runner ----------

// ---------- Command line ----------

// ---------- Job ----------

// ---------- Prerequisites ----------

// ---------- Watcher: what is to be done next? ----------

#[cfg(test)]
mod tests;
