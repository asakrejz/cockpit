//! Kinds of model endpoints and the model selection.

use cockpit_i18n::{Msg, msg};

use super::*;

/// Where the agent sends its model requests. In `config.toml` the only difference is whether an
/// address is entered; "proxy" and "other endpoint" differ only in the hints in the assistant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EndpointKind {
    /// Directly at the provider of the model (no address): Anthropic, for Codex OpenAI.
    Direct,
    /// A proxy of your own like LiteLLM.
    Proxy,
    /// Another compatible endpoint (Anthropic-compatible, for Codex OpenAI-compatible).
    Compatible,
}

impl EndpointKind {
    pub const ALL: [EndpointKind; 3] = [
        EndpointKind::Direct,
        EndpointKind::Proxy,
        EndpointKind::Compatible,
    ];

    pub fn id(self) -> &'static str {
        match self {
            EndpointKind::Direct => "direkt",
            EndpointKind::Proxy => "proxy",
            EndpointKind::Compatible => "kompatibel",
        }
    }

    pub fn from_id(id: &str) -> Option<EndpointKind> {
        Self::ALL.into_iter().find(|a| a.id() == id)
    }

    /// The text in the drop-down; the direct way is named after the provider of the chosen tool.
    pub fn label(self, tool: Provider) -> Msg {
        match self {
            EndpointKind::Direct => msg!(SetupModelKindDirect, vendor = tool.vendor()),
            EndpointKind::Proxy => msg!(SetupModelKindProxy),
            EndpointKind::Compatible => msg!(SetupModelKindCompatible),
        }
    }

    /// What is in the stored configuration: without an address direct, otherwise a proxy.
    pub fn from_url(url: &str) -> EndpointKind {
        if url.trim().is_empty() {
            EndpointKind::Direct
        } else {
            EndpointKind::Proxy
        }
    }

    /// Does this kind need an address?
    pub fn needs_address(self) -> bool {
        self != EndpointKind::Direct
    }

    /// Explanation under the selection.
    pub fn explanation(self, tool: Provider) -> Msg {
        let vendor = tool.vendor();
        match self {
            EndpointKind::Direct => msg!(SetupModelExplainDirect, vendor = vendor),
            EndpointKind::Proxy => msg!(SetupModelExplainProxy),
            EndpointKind::Compatible => msg!(SetupModelExplainCompatible, vendor = vendor),
        }
    }

    pub fn url_placeholder(self) -> &'static str {
        match self {
            EndpointKind::Direct => "",
            EndpointKind::Proxy => "https://litellm.example",
            EndpointKind::Compatible => "https://endpoint.example",
        }
    }

    pub fn key_label(self, tool: Provider) -> Msg {
        match self {
            EndpointKind::Direct if tool.openai_compatible() => msg!(SetupModelKeyOpenai),
            EndpointKind::Direct => msg!(SetupModelKeyAnthropic),
            EndpointKind::Proxy => msg!(SetupModelKeyProxy),
            EndpointKind::Compatible => msg!(SetupModelKeyEndpoint),
        }
    }
}

/// A row of the drop-down for the model name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelOption {
    pub id: String,
    /// The stored model is not in the loaded list.
    pub unknown: bool,
}

/// The selection for the drop-down: the loaded list; a stored model that is missing from it
/// stays visible at the front (marked `unknown`), so that it does not silently disappear.
pub fn model_selection(list: &[String], stored: &str) -> Vec<ModelOption> {
    let stored = stored.trim();
    let mut from: Vec<ModelOption> = Vec::with_capacity(list.len() + 1);
    if !stored.is_empty() && !list.iter().any(|m| m == stored) {
        from.push(ModelOption {
            id: stored.to_string(),
            unknown: true,
        });
    }
    from.extend(list.iter().map(|m| ModelOption {
        id: m.clone(),
        unknown: false,
    }));
    from
}

/// The hint under the drop-down when the stored model is not in the list.
pub fn model_hint(list: &[String], stored: &str) -> Option<Msg> {
    let g = stored.trim();
    (!g.is_empty() && !list.is_empty() && !list.iter().any(|m| m == g))
        .then(|| msg!(CoreSetupModelNotInList, model = g))
}

/// Result of the connection test in one sentence of plain text.
pub fn check_plain_text(p: &EndpointCheck) -> (Tone, Msg) {
    if !p.configured {
        return (Tone::Open, msg!(CoreSetupCheckNothing));
    }
    if !p.reachable {
        return (Tone::Problem, msg!(CoreSetupCheckUnreachable));
    }
    if !p.key_ok {
        return (Tone::Problem, msg!(CoreSetupCheckKeyRejected));
    }
    if !p.model_present {
        return (Tone::Problem, msg!(CoreSetupCheckModelUnknown));
    }
    (Tone::Good, msg!(CoreSetupCheckOk))
}
