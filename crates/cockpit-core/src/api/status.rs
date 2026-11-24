//! Wire shapes of the status the backend reports (health, agent, endpoint, container, image).

use super::*;

/// What the agent works with: the tool that runs in the container (adapters in `apps/agent/runner`).
/// The Claude Agent SDK and Claude Code talk to an Anthropic-compatible endpoint, Codex to an OpenAI-compatible one.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Provider {
    #[default]
    ClaudeAgentSdk,
    ClaudeCode,
    Codex,
}

impl Provider {
    pub const ALL: [Provider; 3] = [
        Provider::ClaudeAgentSdk,
        Provider::ClaudeCode,
        Provider::Codex,
    ];

    /// The name in `config.toml` (`[agent] provider`) and in the JSON.
    pub fn id(self) -> &'static str {
        match self {
            Provider::ClaudeAgentSdk => "claude-agent-sdk",
            Provider::ClaudeCode => "claude-code",
            Provider::Codex => "codex",
        }
    }

    pub fn from_id(text: &str) -> Option<Provider> {
        let t = text.trim().to_lowercase();
        Self::ALL.into_iter().find(|a| a.id() == t)
    }

    /// The name for the UI.
    pub fn label(self) -> &'static str {
        match self {
            Provider::ClaudeAgentSdk => "Claude Agent SDK",
            Provider::ClaudeCode => "Claude Code",
            Provider::Codex => "Codex",
        }
    }

    /// Codex talks to an OpenAI-compatible endpoint, the other tools to an Anthropic-compatible one.
    pub fn openai_compatible(self) -> bool {
        self == Provider::Codex
    }

    /// Who offers the model directly (without an endpoint address), for plain-text messages.
    pub fn vendor(self) -> &'static str {
        if self.openai_compatible() {
            "OpenAI"
        } else {
            "Anthropic"
        }
    }
}

/// The agent's connection to its model, as `GET /api/health` reports it. Never the key itself,
/// only whether one is in the .env file.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct EndpointStatus {
    /// `[agent.endpoint]` (or the older `[litellm]`) is in `config.toml`.
    #[serde(default)]
    pub configured: bool,
    #[serde(default)]
    pub provider: Provider,
    /// Address of the endpoint; empty = directly at the provider.
    #[serde(default)]
    pub url: Option<String>,
    /// The model name.
    #[serde(default)]
    pub model: Option<String>,
    /// The key (`agent-key`) is in the .env file.
    #[serde(default)]
    pub key: bool,
}

/// The container runtime, as `GET /api/health` reports it.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct ContainerStatus {
    /// `docker` or `podman`; `None` if none was found or chosen.
    #[serde(default)]
    pub runtime: Option<String>,
    /// `<cli> info` succeeded: the engine is running.
    #[serde(default)]
    pub engine_running: bool,
    /// Where the choice comes from: `config`, `env`, `detected` or `none`.
    #[serde(default)]
    pub source: String,
    /// What to do if it does not work.
    #[serde(default)]
    pub hint: Option<String>,
    /// The same as `hint`, as code and parameters (the UI shows it in its language).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hint_msg: Option<cockpit_i18n::Msg>,
    /// Cockpit runs inside its own container image (`COCKPIT_IN_CONTAINER`, `/.dockerenv`, `/run/.containerenv`).
    #[serde(default)]
    pub in_container: bool,
    /// Only meaningful with `in_container`: the host's engine socket inside the container.
    #[serde(default)]
    pub socket: SocketState,
}

/// The engine socket as seen from inside the Cockpit container.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SocketState {
    /// No socket mounted (or not checked).
    #[default]
    Missing,
    /// A socket is there, but this user may not use it.
    Denied,
    /// A socket is there and accepts connections.
    Reachable,
}

/// Whether the agent can start, as `GET /api/health` reports it.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct AgentStatus {
    /// Runs can start: everything necessary is set up.
    #[serde(default)]
    pub ready: bool,
    /// The agent image exists, current or not (`image inspect`).
    #[serde(default)]
    pub image: bool,
    /// Whether the image matches the sources of this Cockpit version (fingerprint).
    #[serde(default)]
    pub image_state: ImageState,
    /// Plain-text reason when `image_state` is `unknown`.
    #[serde(default)]
    pub image_hint: Option<String>,
    /// The same as `image_hint`, as code and parameters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_hint_msg: Option<cockpit_i18n::Msg>,
    /// Name of the image, always the same: `localhost/cockpit-agent:local`.
    #[serde(default)]
    pub image_name: String,
    /// The default repo of the agent (`owner/name`), if there is one.
    #[serde(default)]
    pub repo: Option<String>,
    /// The repos the agent may work in, with base branch and the Jira projects routed to each,
    /// and the default. The interface preselects the repo with the same rules the backend applies.
    #[serde(default)]
    pub repos: AgentRepos,
    /// What is still missing, one sentence per entry in plain text with the place in the assistant.
    #[serde(default)]
    pub missing: Vec<String>,
    /// The same as `missing`, as code and parameters.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub missing_msgs: Vec<cockpit_i18n::Msg>,
}

/// How the agent image compares with the sources of this Cockpit version.
///
/// The image carries the label `cockpit.fingerprint` (a SHA-256 over the files that make the
/// image, see the server); it is compared with the fingerprint of the current sources.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ImageState {
    /// There is no image (yet).
    #[default]
    Missing,
    /// The image exists, but the label is missing or differs: it must be rebuilt.
    Outdated,
    /// The image was built from the current sources.
    Current,
    /// The image exists, but the fingerprint of the sources cannot be computed.
    Unknown,
}

/// State of „build image“ (`GET`/`POST /api/agent/image`).
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct ImageBuild {
    /// The build is running right now.
    #[serde(default)]
    pub running: bool,
    /// The image exists (after the last build or already before).
    #[serde(default)]
    pub present: bool,
    /// Whether the image is current (see [`ImageState`]).
    #[serde(default)]
    pub state: ImageState,
    /// Why the last build failed, in plain text.
    #[serde(default)]
    pub error: Option<String>,
    /// The same as `error`, as code and parameters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_msg: Option<cockpit_i18n::Msg>,
    /// The last lines of the build output.
    #[serde(default)]
    pub lines: Vec<String>,
}

/// Response to `GET /api/agent/check/container`: does a container reach the endpoint?
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct ContainerCheck {
    /// It could be checked (image, runtime and endpoint are there).
    #[serde(default)]
    pub checked: bool,
    /// The Mac (the backend) reaches the endpoint.
    #[serde(default)]
    pub mac_reached: bool,
    /// The container reaches it.
    #[serde(default)]
    pub container_reached: bool,
    /// The endpoint accepts the key in the container.
    #[serde(default)]
    pub key_ok: bool,
    /// What to do next or what the cause was.
    #[serde(default)]
    pub hint: Option<String>,
    /// The same as `hint`, as code and parameters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hint_msg: Option<cockpit_i18n::Msg>,
}

/// Response to `GET /api/agent/check`: what the connection test found. Never contains the key.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct EndpointCheck {
    /// There is something to check (address or model entered); otherwise nothing was called.
    #[serde(default)]
    pub configured: bool,
    /// The endpoint answered `GET <url>/v1/models`.
    #[serde(default)]
    pub reachable: bool,
    /// The endpoint accepted the key.
    #[serde(default)]
    pub key_ok: bool,
    /// The configured model name is in the endpoint's list.
    #[serde(default)]
    pub model_present: bool,
    /// What to do next or what the cause was.
    #[serde(default)]
    pub hint: Option<String>,
    /// The same as code and parameters, rendered in the chosen language.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub msg: Option<cockpit_i18n::Msg>,
}

/// Response to `GET /api/agent/models`: the model IDs of the endpoint (IDs only, Claude models
/// first, at most [`MAX_MODELS`]). Never contains the key.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct ModelList {
    /// The list could be loaded.
    #[serde(default)]
    pub loaded: bool,
    #[serde(default)]
    pub reachable: bool,
    #[serde(default)]
    pub key_ok: bool,
    #[serde(default)]
    pub models: Vec<String>,
    /// Plain text if something did not work.
    #[serde(default)]
    pub hint: Option<String>,
    /// The same as code and parameters, rendered in the chosen language.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub msg: Option<cockpit_i18n::Msg>,
}

/// The list delivers at most this many model IDs.
pub const MAX_MODELS: usize = 200;
