//! The plain-text summary of the whole setup.

use super::*;

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

pub(super) fn service_line(
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
pub(super) fn jira_line(s: &Snapshot) -> Line {
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
