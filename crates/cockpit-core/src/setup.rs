//! The setup assistant: order of the steps, reading and checking inputs, and the
//! summary in plain text. Pure logic without UI and network; the „Einrichten“ page
//! merely wires it up.

use crate::api::{
    AgentStatus, ContainerStatus, EndpointCheck, EndpointStatus, JiraCause, Provider, Setup,
};
use crate::connections::{AuthKind, ServiceStatus};

/// The names under which the backend stores keys in the .env file (`PUT /api/secrets/{name}`).
pub const KEY_NAMES: &[&str] = &[
    "github-oauth",
    "atlassian-oauth",
    "agent-key",
    "litellm",
    "jira",
    "github",
    "github-webhook",
    "jira-webhook",
];

/// Where to create an API token at Atlassian.
pub const JIRA_TOKEN_PAGE: &str = "https://id.atlassian.com/manage-profile/security/api-tokens";

/// Longest allowed key (characters). Real tokens are far shorter.
pub const MAX_KEYS: usize = 4096;

/// A step of the assistant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Container,
    Jira,
    GitHub,
    Model,
    Live,
    Done,
}

impl Step {
    pub const ALL: [Step; 6] = [
        Step::Container,
        Step::Jira,
        Step::GitHub,
        Step::Model,
        Step::Live,
        Step::Done,
    ];

    /// Short name for the progress display.
    pub fn name(self) -> &'static str {
        match self {
            Step::Container => "Container",
            Step::Jira => "Jira",
            Step::GitHub => "GitHub",
            Step::Model => "Modell",
            Step::Live => "Live-Update",
            Step::Done => "Fertig",
        }
    }

    pub fn index(self) -> usize {
        Self::ALL.iter().position(|s| *s == self).unwrap_or(0)
    }

    pub fn next(self) -> Option<Step> {
        Self::ALL.get(self.index() + 1).copied()
    }

    pub fn previous(self) -> Option<Step> {
        self.index().checked_sub(1).map(|i| Self::ALL[i])
    }

    /// ID for the address (`#/einrichten/jira`).
    pub fn id(self) -> &'static str {
        match self {
            Step::Container => "container",
            Step::Jira => "jira",
            Step::GitHub => "github",
            Step::Model => "modell",
            Step::Live => "live",
            Step::Done => "fertig",
        }
    }

    pub fn from_id(id: &str) -> Option<Step> {
        Self::ALL.into_iter().find(|s| s.id() == id)
    }
}

/// `true` if nothing at all is set up yet – then the assistant appears on first start.
pub fn nothing_set_up(
    setup: &Setup,
    jira: &ServiceStatus,
    github: &ServiceStatus,
    endpoint: &EndpointStatus,
) -> bool {
    !(jira.connected
        || github.connected
        || endpoint.configured
        || endpoint.key
        || setup.atlassian_client_id.is_some()
        || setup.github_client_id.is_some()
        || !setup.github_repos.is_empty()
        || !setup.jira_site.is_empty())
}

/// Checks a key before sending. Returns the cleaned value (whitespace at the edges
/// removed, e.g. from pasting). The messages never name the value.
pub fn key_verify(value: &str) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty() {
        return Err("Das Feld ist leer.".into());
    }
    if value.chars().count() > MAX_KEYS {
        return Err("Der Schlüssel ist ungewöhnlich lang – wurde zu viel eingefügt?".into());
    }
    if value.chars().any(char::is_control) {
        return Err(
            "Der Schlüssel enthält Zeilenumbrüche oder Steuerzeichen – bitte nur den Schlüssel selbst einfügen."
                .into(),
        );
    }
    Ok(value.to_string())
}

/// Checks the public address of the tunnel (`[server] public_url`): `https://host[:port][/path]`,
/// no spaces, no user name, query or fragment. Returns it without a trailing slash; empty
/// stays empty (= remove). The address is only displayed, the server never calls it.
pub fn public_url_read(text: &str) -> Result<String, String> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(String::new());
    }
    let rest = text.strip_prefix("https://").ok_or(
        "Die öffentliche Adresse muss mit https:// beginnen, z. B. https://cockpit.example.",
    )?;
    let host = rest.split('/').next().unwrap_or("");
    if host.is_empty()
        || text.len() > 200
        || text.chars().any(|c| c.is_whitespace() || c.is_control())
        || text.contains(['?', '#', '@', '\\'])
    {
        return Err(
            "Das ist keine gültige Adresse. Erwartet wird etwas wie https://cockpit.example (ohne Leerzeichen, Zugangsdaten oder Parameter)."
                .into(),
        );
    }
    Ok(text.trim_end_matches('/').to_string())
}

/// GitHub events the webhook should send (Settings › Webhooks › „Let me select individual events“).
pub const GITHUB_WEBHOOK_EVENTS: &[&str] = &[
    "Pull requests",
    "Pull request reviews",
    "Pull request review comments",
    "Check runs",
    "Check suites",
    "Issue comments",
];

/// Jira events the webhook should send (Admin › System › WebHooks).
pub const JIRA_WEBHOOK_EVENTS: &[&str] = &[
    "Issue: created, updated, deleted",
    "Comment: created, updated, deleted",
];

/// The address to enter at GitHub or Jira: the public address of the tunnel plus the route. Without
/// an address a placeholder shows how it will look.
pub fn webhook_target(public_url: Option<&str>, path: &str) -> String {
    format!(
        "{}{path}",
        public_url
            .filter(|u| !u.is_empty())
            .unwrap_or("<öffentliche Adresse>")
    )
}

/// "Zuletzt empfangen vor 3 Min. (pull_request)" – only time and event type, never content.
pub fn webhook_received_text(last_ms: Option<u64>, event: Option<&str>, now_ms: u64) -> String {
    match last_ms {
        None => "Noch nichts empfangen (seit dem Start von Cockpit).".into(),
        Some(t) => {
            let when = crate::notice::ago(t, now_ms);
            match event.filter(|e| !e.is_empty()) {
                Some(e) => format!("Zuletzt empfangen {when} ({e})."),
                None => format!("Zuletzt empfangen {when}."),
            }
        }
    }
}

/// Whether `name` is an allowed key name.
pub fn key_name_allowed(name: &str) -> bool {
    KEY_NAMES.contains(&name)
}

/// Whether `name` is a token that „Trennen“ may delete from the .env file. Client secrets and
/// the model key do not belong to it.
pub fn key_deletable(name: &str) -> bool {
    matches!(name, "jira" | "github")
}

/// The plain text for the result of the Jira connection test. Never names the token.
pub fn jira_check_text(cause: JiraCause, name: Option<&str>) -> String {
    match cause {
        JiraCause::Ok => match name.filter(|n| !n.is_empty()) {
            Some(n) => format!("Jira antwortet und kennt dich: {n}."),
            None => "Jira antwortet und kennt dich.".into(),
        },
        JiraCause::Details => {
            "Es fehlen noch Angaben: Adresse der Site, E-Mail-Adresse und Token.".into()
        }
        JiraCause::Address => {
            "Unter dieser Adresse antwortet kein Jira. Prüfe die Adresse – sie steht in der Adresszeile, wenn du Jira im Browser öffnest (z. B. firma.atlassian.net)."
                .into()
        }
        JiraCause::Token => {
            "Jira lehnt den Token ab. Prüfe die E-Mail-Adresse (die deines Atlassian-Kontos) und erzeuge im Zweifel einen neuen Token. Ist er abgelaufen, hilft ebenfalls ein neuer."
                .into()
        }
        JiraCause::Permission => {
            "Der Token ist gültig, aber Jira verweigert den Zugriff. Dein Konto braucht Zugang zu dieser Site – frag die Administration."
                .into()
        }
        JiraCause::Reachable => {
            "Jira ist nicht erreichbar. Stimmt die Adresse, und bist du im richtigen Netz oder VPN?"
                .into()
        }
        JiraCause::Other => {
            "Jira hat unerwartet geantwortet. Versuche es in einer Minute noch einmal.".into()
        }
    }
}

/// The site of a Jira Cloud instance as an address: `firma` → `https://firma.atlassian.net`,
/// `firma.atlassian.net` and `https://firma.atlassian.net/jira/…` → `https://firma.atlassian.net`.
/// Empty stays empty.
pub fn jira_site_read(text: &str) -> Result<String, String> {
    let t = text.trim();
    if t.is_empty() {
        return Ok(String::new());
    }
    if t.chars().any(char::is_whitespace) {
        return Err("Die Adresse darf keine Leerzeichen enthalten.".into());
    }
    // The own machine (for tests with a dummy) may be addressed without https.
    if let Some((s, r)) = t.split_once("://")
        && s.eq_ignore_ascii_case("http")
    {
        let host = r.split(['/', '?', '#']).next().unwrap_or_default();
        let name = host.split(':').next().unwrap_or_default();
        let port_ok = host
            .split_once(':')
            .is_none_or(|(_, p)| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()));
        if matches!(name, "127.0.0.1" | "localhost") && port_ok {
            return Ok(format!("http://{host}"));
        }
    }
    let (scheme, rest) = match t.split_once("://") {
        Some((s, r)) if s.eq_ignore_ascii_case("https") => ("https", r),
        Some((s, _)) if s.eq_ignore_ascii_case("http") => {
            return Err("Jira Cloud erreichst du nur über https://.".into());
        }
        Some(_) => return Err("Die Adresse muss mit https:// beginnen.".into()),
        None => ("https", t),
    };
    let host = rest.split(['/', '?', '#']).next().unwrap_or_default();
    if host.is_empty() {
        return Err("In der Adresse fehlt der Name der Site.".into());
    }
    let host = host.to_ascii_lowercase();
    let host = if host.contains('.') {
        host
    } else {
        format!("{host}.atlassian.net")
    };
    let valid = host
        .split('.')
        .all(|t| !t.is_empty() && t.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'));
    if !valid {
        return Err(format!(
            "„{host}“ sieht nicht wie eine Jira-Adresse aus, z. B. firma.atlassian.net."
        ));
    }
    Ok(format!("{scheme}://{host}"))
}

/// Jira project keys from a text („bau, web“ or one line per project), uppercased
/// and without duplicates.
pub fn projects_read(text: &str) -> Vec<String> {
    let mut from: Vec<String> = Vec::new();
    for part in text.split(|c: char| c == ',' || c == ';' || c.is_whitespace()) {
        let p = part.trim().to_uppercase();
        if !p.is_empty() && !from.contains(&p) {
            from.push(p);
        }
    }
    from
}

/// The agent's repos from the rows of the assistant: `(repo, base branch, projects)` per row
/// plus the default. Empty rows are skipped. Rejects (with a plain-text reason) a repo that
/// is not `owner/name`, one that is listed twice, a base branch with odd characters, a Jira
/// project that is not a key or that goes to two repos, and a default outside the list.
pub fn agent_repos_read(
    rows: &[(String, String, String)],
    default: &str,
) -> Result<crate::api::AgentReposInput, String> {
    let mut repos: Vec<crate::AgentRepo> = Vec::new();
    for (repo, base, projects) in rows {
        if repo.trim().is_empty() {
            continue;
        }
        let name = match repos_read(repo)?.as_slice() {
            [one] => one.clone(),
            _ => return Err(format!("„{}“: bitte ein Repo pro Zeile.", repo.trim())),
        };
        if repos.iter().any(|r| r.repo == name) {
            return Err(format!("Das Repo „{name}“ steht doppelt in der Liste."));
        }
        let base = base.trim();
        let base = if base.is_empty() {
            crate::repos::DEFAULT_BASE
        } else {
            base
        };
        if base.starts_with('-')
            || base.contains("..")
            || !base
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '-' | '_' | '.'))
        {
            return Err(format!(
                "Der Basis-Branch „{base}“ von {name} ist nicht gültig."
            ));
        }
        let projects = projects_read(projects);
        for p in &projects {
            if !crate::model::is_project_key(p) {
                return Err(format!("„{p}“ ist kein Jira-Projektschlüssel (z. B. WEB)."));
            }
            if let Some(other) = repos.iter().find(|r| r.projects.contains(p)) {
                return Err(format!(
                    "Das Jira-Projekt {p} ist schon dem Repo „{}“ zugeordnet.",
                    other.repo
                ));
            }
        }
        repos.push(crate::AgentRepo {
            repo: name,
            base_branch: base.to_string(),
            projects,
        });
    }
    let default = default.trim();
    let default = if default.is_empty() {
        None
    } else {
        let d = repos_read(default)?.into_iter().next().unwrap_or_default();
        if !repos.iter().any(|r| r.repo == d) {
            return Err(format!("Das Standard-Repo „{d}“ muss in der Liste stehen."));
        }
        Some(d)
    };
    Ok(crate::api::AgentReposInput { repos, default })
}

/// GitHub repos from a text. Allows `owner/name`, full addresses (`https://github.com/o/n`,
/// also with `.git`) and separation by comma, space or lines.
pub fn repos_read(text: &str) -> Result<Vec<String>, String> {
    let mut from: Vec<String> = Vec::new();
    for part in text.split(|c: char| c == ',' || c == ';' || c.is_whitespace()) {
        let mut r = part.trim();
        if r.is_empty() {
            continue;
        }
        for prefix in ["https://github.com/", "http://github.com/", "github.com/"] {
            if let Some(rest) = r.strip_prefix(prefix) {
                r = rest;
                break;
            }
        }
        let r = r.trim_end_matches('/');
        let r = r.strip_suffix(".git").unwrap_or(r);
        // For a full address only the first two parts count (`…/o/n/pull/3`).
        let mut parts = r.split('/');
        let (Some(o), Some(n)) = (parts.next(), parts.next()) else {
            return Err(format!(
                "„{part}“ ist kein Repo – gebraucht wird die Form firma/app."
            ));
        };
        let chars = |t: &str| {
            !t.is_empty()
                && t.chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
        };
        if !chars(o) || !chars(n) {
            return Err(format!(
                "„{part}“ ist kein Repo – gebraucht wird die Form firma/app."
            ));
        }
        let repo = format!("{o}/{n}");
        if !from.contains(&repo) {
            from.push(repo);
        }
    }
    Ok(from)
}

/// A client ID from the input field: without whitespace at the edges, without spaces inside.
pub fn client_id_read(text: &str) -> Result<String, String> {
    let t = text.trim();
    if t.chars().any(char::is_whitespace) {
        return Err(
            "Die Client-ID enthält Leerzeichen – bitte genau so kopieren, wie sie dasteht.".into(),
        );
    }
    Ok(t.to_string())
}

/// The address of the endpoint (LiteLLM, another compatible service): without `http://` or
/// `https://`, `http://` is added (`localhost:4000` → `http://localhost:4000`), a
/// trailing slash is dropped. Empty stays empty (= directly at the provider).
pub fn endpoint_url_read(text: &str) -> Result<String, String> {
    let t = text.trim();
    if t.is_empty() {
        return Ok(String::new());
    }
    if t.chars().any(char::is_whitespace) {
        return Err("Die Adresse darf keine Leerzeichen enthalten.".into());
    }
    let url = if t.contains("://") {
        t.to_string()
    } else {
        format!("http://{t}")
    };
    let rest = url
        .strip_prefix("http://")
        .or_else(|| url.strip_prefix("https://"))
        .ok_or("Die Adresse muss mit http:// oder https:// beginnen.")?;
    if rest.trim_matches('/').is_empty() {
        return Err("In der Adresse fehlt der Rechnername, z. B. localhost:4000.".into());
    }
    Ok(url.trim_end_matches('/').to_string())
}

/// Where the agent sends its model requests. In `config.toml` the only difference is whether an
/// address is entered; „Proxy“ and „other endpoint“ differ only in the hints in the assistant.
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
    pub fn label(self, tool: Provider) -> String {
        match self {
            EndpointKind::Direct => format!("Direkt bei {}", tool.vendor()),
            EndpointKind::Proxy => "Eigener Proxy (z. B. LiteLLM)".into(),
            EndpointKind::Compatible => "Anderer kompatibler Endpunkt".into(),
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
    pub fn explanation(self, tool: Provider) -> String {
        let vendor = tool.vendor();
        match self {
            EndpointKind::Direct => format!(
                "Der Agent spricht ohne Zwischenstation mit {vendor}. Du brauchst einen API-Key aus der Konsole von {vendor}."
            ),
            EndpointKind::Proxy => {
                "Eine Zwischenstation, die dein Team betreibt (z. B. LiteLLM). Du bekommst Adresse und Key vom Betreiber."
                    .into()
            }
            EndpointKind::Compatible => format!(
                "Ein Dienst, der die Schnittstelle von {vendor} nachbildet. Trage seine Adresse und seinen Key ein."
            ),
        }
    }

    pub fn url_placeholder(self) -> &'static str {
        match self {
            EndpointKind::Direct => "",
            EndpointKind::Proxy => "https://litellm.example",
            EndpointKind::Compatible => "https://endpunkt.example",
        }
    }

    pub fn key_label(self, tool: Provider) -> &'static str {
        match self {
            EndpointKind::Direct if tool.openai_compatible() => "OpenAI-API-Key",
            EndpointKind::Direct => "Anthropic-API-Key",
            EndpointKind::Proxy => "Key des Proxys",
            EndpointKind::Compatible => "Key des Endpunkts",
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

/// Checks a model name typed by hand: not empty, without whitespace and control characters, not too long.
pub fn model_name_read(text: &str) -> Result<String, String> {
    let t = text.trim();
    if t.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err("Der Modellname enthält Leerzeichen oder Sonderzeichen.".into());
    }
    if t.chars().count() > 200 {
        return Err("Der Modellname ist zu lang.".into());
    }
    Ok(t.to_string())
}

/// The hint under the drop-down when the stored model is not in the list.
pub fn model_hint(list: &[String], stored: &str) -> Option<String> {
    let g = stored.trim();
    (!g.is_empty() && !list.is_empty() && !list.iter().any(|m| m == g)).then(|| {
        format!("„{g}“ steht nicht in der Liste des Endpunkts. Wähle ein Modell aus der Liste, oder lass es, wenn du den Namen sicher kennst.")
    })
}

/// How well an item of the summary stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    /// Works (green checkmark).
    Good,
    /// Not set up yet or skipped – no error.
    Open,
    /// Something went wrong or is incomplete.
    Problem,
}

/// A row of the summary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    pub name: &'static str,
    pub tone: Tone,
    /// Plain text of what is going on.
    pub text: String,
    /// The step in which this can be fixed.
    pub step: Step,
}

/// What the summary needs to know.
#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub setup: Setup,
    pub jira: ServiceStatus,
    pub github: ServiceStatus,
    pub endpoint: EndpointStatus,
    pub container: ContainerStatus,
    /// Whether the real agent can start (image built, everything set up).
    pub agent: AgentStatus,
    /// Result of the last connection test to the endpoint, if it has run already.
    pub check: Option<EndpointCheck>,
}

fn service_line(
    name: &'static str,
    step: Step,
    status: &ServiceStatus,
    client_id: bool,
    secret: bool,
    token_way: &str,
) -> Line {
    let (tone, text) = match (status.connected, status.kind) {
        (true, AuthKind::OAuth) => (Tone::Good, format!("Verbunden – du bist über {name} angemeldet.")),
        (true, _) => (Tone::Good, "Verbunden – mit API-Token.".to_string()),
        (false, _) if !client_id && !secret => (
            Tone::Open,
            format!("Noch nicht eingerichtet. {token_way}"),
        ),
        (false, _) if !client_id => (
            Tone::Problem,
            "Die Client-ID fehlt noch. Du findest sie in der Entwicklerseite deiner App.".into(),
        ),
        (false, _) if !secret => (
            Tone::Problem,
            "Das Client-Secret fehlt noch. Es wird nur einmal angezeigt – ist es weg, erzeuge in deiner App ein neues."
                .into(),
        ),
        (false, _) => (
            Tone::Open,
            format!("Fast fertig: Klicke auf „Mit {name} anmelden“."),
        ),
    };
    Line {
        name,
        tone,
        text,
        step,
    }
}

/// The Jira row: token (default) or OAuth, with the kind of sign-in in the text.
fn jira_line(s: &Snapshot) -> Line {
    let e = &s.setup;
    let client_id = e.atlassian_client_id.is_some();
    let secret = e.has_key("atlassian-oauth");
    let (tone, text) = match (s.jira.connected, s.jira.kind) {
        (true, AuthKind::OAuth) => (
            Tone::Good,
            "Verbunden – mit OAuth (über deine Atlassian-App angemeldet).".to_string(),
        ),
        (true, _) => (Tone::Good, "Verbunden – mit API-Token.".to_string()),
        // The way via the Atlassian app is started: say what is missing, as before.
        (false, _) if client_id || secret => {
            let z = service_line("Atlassian", Step::Jira, &s.jira, client_id, secret, "");
            (z.tone, z.text)
        }
        (false, _) => {
            let missing: Vec<&str> = [
                (e.jira_site.is_empty(), "die Adresse der Site"),
                (e.jira_email.is_empty(), "die E-Mail-Adresse"),
                (!e.has_key("jira"), "der Token"),
            ]
            .into_iter()
            .filter_map(|(missing, name)| missing.then_some(name))
            .collect();
            if missing.len() == 3 {
                (
                    Tone::Open,
                    "Noch nicht eingerichtet. Am einfachsten geht es mit einem API-Token."
                        .to_string(),
                )
            } else {
                (
                    Tone::Open,
                    format!(
                        "Die Anmeldung mit API-Token ist noch nicht fertig: Es fehlt {}.",
                        missing.join(" und ")
                    ),
                )
            }
        }
    };
    Line {
        name: "Jira",
        tone,
        text,
        step: Step::Jira,
    }
}

/// The summary at the end: per item a checkmark or a sentence saying what is missing.
pub fn summary(s: &Snapshot) -> Vec<Line> {
    let e = &s.setup;
    let container = match (&s.container.runtime, s.container.engine_running) {
        // If the real agent runs, one step is still missing without an image.
        (Some(l), true) if s.agent.mode == "real" && !s.agent.image => (
            Tone::Open,
            format!(
                "{l} läuft. Das Agent-Image ist noch nicht gebaut – klicke im Schritt „Container“ auf „Image bauen“."
            ),
        ),
        (Some(l), true) => (Tone::Good, format!("{l} ist installiert und läuft.")),
        (Some(l), false) => (
            Tone::Problem,
            format!(
                "{l} ist installiert, aber nicht gestartet. Öffne das Programm und warte, bis es bereit ist."
            ),
        ),
        (None, _) => (
            Tone::Open,
            "Noch kein Docker oder Podman gefunden. Du brauchst es erst für den Agenten.".into(),
        ),
    };
    let jira = jira_line(s);
    let mut github = service_line(
        "GitHub",
        Step::GitHub,
        &s.github,
        e.github_client_id.is_some(),
        e.has_key("github-oauth"),
        "Dafür brauchst du eine GitHub App.",
    );
    github.name = "GitHub";
    if github.tone == Tone::Good && e.github_repos.is_empty() {
        github.tone = Tone::Problem;
        github.text =
            "Angemeldet, aber ohne Repos: Trage ein, welche Repos Cockpit abgleichen soll.".into();
    }

    let model = {
        let (tone, text) = match (&s.check, s.endpoint.configured || s.endpoint.key) {
            (Some(p), _) => check_plain_text(p),
            (None, false) => (Tone::Open, "Noch nicht eingerichtet.".to_string()),
            (None, true) if !s.endpoint.key => (
                Tone::Problem,
                "Eingetragen, aber der Key fehlt noch.".to_string(),
            ),
            (None, true) if s.endpoint.model.is_none() => (
                Tone::Problem,
                "Key ist da, aber der Modellname fehlt noch.".to_string(),
            ),
            (None, true) => (
                Tone::Open,
                "Eingetragen, aber noch nicht getestet. Starte den Verbindungstest.".to_string(),
            ),
        };
        // Name the chosen tool as soon as something is entered.
        let text = if s.endpoint.configured || s.endpoint.key {
            format!("{}: {text}", s.endpoint.provider.label())
        } else {
            text
        };
        Line {
            name: "Modellzugang",
            tone,
            text,
            step: Step::Model,
        }
    };

    vec![
        Line {
            name: "Container",
            tone: container.0,
            text: container.1,
            step: Step::Container,
        },
        jira,
        github,
        model,
    ]
}

/// Result of the connection test in one sentence of plain text.
pub fn check_plain_text(p: &EndpointCheck) -> (Tone, String) {
    if !p.configured {
        return (Tone::Open, "Noch nichts eingetragen.".into());
    }
    if !p.reachable {
        return (
            Tone::Problem,
            "Der Endpunkt antwortet nicht. Stimmt die Adresse, und bist du im richtigen Netz oder VPN?"
                .into(),
        );
    }
    if !p.key_ok {
        return (
            Tone::Problem,
            "Der Endpunkt ist erreichbar, nimmt den Key aber nicht an. Prüfe, ob du den ganzen Key kopiert hast."
                .into(),
        );
    }
    if !p.model_present {
        return (
            Tone::Problem,
            "Verbindung und Key sind in Ordnung, aber der Endpunkt kennt den Modellnamen nicht. Lade die Modelle und wähle eines aus der Liste."
                .into(),
        );
    }
    (
        Tone::Good,
        "Verbindung, Key und Modell sind in Ordnung.".into(),
    )
}

#[cfg(test)]
mod tests;
