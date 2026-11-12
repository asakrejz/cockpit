//! Der Einrichtungsassistent: Reihenfolge der Schritte, Eingaben lesen und prüfen, und die
//! Zusammenfassung in Klartext. Reine Logik ohne Oberfläche und Netzwerk; die Seite „Einrichten“
//! verdrahtet sie nur.

use crate::api::{
    AgentStatus, Provider, ContainerStatus, Setup, EndpointCheck, EndpointStatus,
    JiraCause,
};
use crate::connections::{AuthKind, ServiceStatus};

/// Die Namen, unter denen das Backend Schlüssel im Schlüsselbund ablegt (`PUT /api/schluessel/{name}`).
pub const KEY_NAMES: &[&str] = &[
    "github-oauth",
    "atlassian-oauth",
    "agent-key",
    "litellm",
    "jira",
    "github",
];

/// Wo man bei Atlassian einen API-Token erzeugt.
pub const JIRA_TOKEN_PAGE: &str = "https://id.atlassian.com/manage-profile/security/api-tokens";

/// Längster erlaubter Schlüssel (Zeichen). Echte Tokens sind weit kürzer.
pub const MAX_KEYS: usize = 4096;

/// Ein Schritt des Assistenten.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Container,
    Jira,
    GitHub,
    Model,
    Done,
}

impl Step {
    pub const ALL: [Step; 5] = [
        Step::Container,
        Step::Jira,
        Step::GitHub,
        Step::Model,
        Step::Done,
    ];

    /// Kurzer Name für die Fortschrittsanzeige.
    pub fn name(self) -> &'static str {
        match self {
            Step::Container => "Container",
            Step::Jira => "Jira",
            Step::GitHub => "GitHub",
            Step::Model => "Modell",
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

    /// Kennung für die Adresse (`#/einrichten/jira`).
    pub fn id(self) -> &'static str {
        match self {
            Step::Container => "container",
            Step::Jira => "jira",
            Step::GitHub => "github",
            Step::Model => "modell",
            Step::Done => "fertig",
        }
    }

    pub fn from_id(id: &str) -> Option<Step> {
        Self::ALL.into_iter().find(|s| s.id() == id)
    }
}

/// `true`, wenn noch gar nichts eingerichtet ist – dann erscheint der Assistent beim ersten Start.
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

/// Prüft einen Schlüssel vor dem Senden. Gibt den bereinigten Wert zurück (Leerraum am Rand
/// entfernt, z. B. vom Einfügen). Die Meldungen nennen nie den Wert.
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

/// Ob `name` ein erlaubter Schlüsselname ist.
pub fn key_name_allowed(name: &str) -> bool {
    KEY_NAMES.contains(&name)
}

/// Ob `name` ein Token ist, den „Trennen“ aus dem Schlüsselbund löschen darf. Client-Secrets und
/// der Modell-Key gehören nicht dazu.
pub fn key_deletable(name: &str) -> bool {
    matches!(name, "jira" | "github")
}

/// Der Klartext zum Ergebnis des Jira-Verbindungstests. Nennt nie den Token.
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

/// Die Site einer Jira-Cloud-Instanz als Adresse: `firma` → `https://firma.atlassian.net`,
/// `firma.atlassian.net` und `https://firma.atlassian.net/jira/…` → `https://firma.atlassian.net`.
/// Leer bleibt leer.
pub fn jira_site_read(text: &str) -> Result<String, String> {
    let t = text.trim();
    if t.is_empty() {
        return Ok(String::new());
    }
    if t.chars().any(char::is_whitespace) {
        return Err("Die Adresse darf keine Leerzeichen enthalten.".into());
    }
    // Der eigene Rechner (für Tests mit einer Attrappe) darf ohne https angesprochen werden.
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

/// Jira-Projektschlüssel aus einem Text („bau, web“ oder eine Zeile je Projekt), groß geschrieben
/// und ohne Doppelte.
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

/// GitHub-Repos aus einem Text. Erlaubt `owner/name`, ganze Adressen (`https://github.com/o/n`,
/// auch mit `.git`) und Trennung durch Komma, Leerzeichen oder Zeilen.
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
        // Bei einer ganzen Adresse zählen nur die ersten beiden Teile (`…/o/n/pull/3`).
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

/// Eine Client-ID aus dem Eingabefeld: ohne Leerraum am Rand, ohne Leerzeichen darin.
pub fn client_id_read(text: &str) -> Result<String, String> {
    let t = text.trim();
    if t.chars().any(char::is_whitespace) {
        return Err(
            "Die Client-ID enthält Leerzeichen – bitte genau so kopieren, wie sie dasteht.".into(),
        );
    }
    Ok(t.to_string())
}

/// Die Adresse des Endpunkts (LiteLLM, ein anderer kompatibler Dienst): ohne `http://` oder
/// `https://` wird `http://` ergänzt (`localhost:4000` → `http://localhost:4000`), ein
/// Schrägstrich am Ende fällt weg. Leer bleibt leer (= direkt beim Anbieter).
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

/// Wohin der Agent seine Modellanfragen schickt. In `config.toml` unterscheidet sich nur, ob eine
/// Adresse eingetragen ist; „Proxy“ und „anderer Endpunkt“ unterscheiden nur die Hinweise im Assistenten.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EndpointKind {
    /// Direkt bei Anthropic (keine Adresse, Key von der Anthropic-Konsole).
    Direct,
    /// Ein eigener Proxy wie LiteLLM.
    Proxy,
    /// Ein anderer Anthropic-kompatibler Endpunkt.
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

    pub fn label(self) -> &'static str {
        match self {
            EndpointKind::Direct => "Direkt bei Anthropic",
            EndpointKind::Proxy => "Eigener Proxy (z. B. LiteLLM)",
            EndpointKind::Compatible => "Anderer kompatibler Endpunkt",
        }
    }

    /// Was in der gespeicherten Konfiguration steht: ohne Adresse direkt, sonst ein Proxy.
    pub fn from_url(url: &str) -> EndpointKind {
        if url.trim().is_empty() {
            EndpointKind::Direct
        } else {
            EndpointKind::Proxy
        }
    }

    /// Braucht diese Art eine Adresse?
    pub fn needs_address(self) -> bool {
        self != EndpointKind::Direct
    }

    /// Erklärung unter der Auswahl.
    pub fn explanation(self) -> &'static str {
        match self {
            EndpointKind::Direct => {
                "Der Agent spricht ohne Zwischenstation mit Anthropic. Du brauchst einen API-Key aus der Anthropic-Konsole."
            }
            EndpointKind::Proxy => {
                "Eine Zwischenstation, die dein Team betreibt (z. B. LiteLLM). Du bekommst Adresse und Key vom Betreiber."
            }
            EndpointKind::Compatible => {
                "Ein Dienst, der die Schnittstelle von Anthropic nachbildet. Trage seine Adresse und seinen Key ein."
            }
        }
    }

    pub fn url_placeholder(self) -> &'static str {
        match self {
            EndpointKind::Direct => "",
            EndpointKind::Proxy => "https://litellm.example",
            EndpointKind::Compatible => "https://endpunkt.example",
        }
    }

    pub fn key_label(self) -> &'static str {
        match self {
            EndpointKind::Direct => "Anthropic-API-Key",
            EndpointKind::Proxy => "Key des Proxys",
            EndpointKind::Compatible => "Key des Endpunkts",
        }
    }
}

/// Eine Zeile des Drop-downs für den Modellnamen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelOption {
    pub id: String,
    /// Das gespeicherte Modell steht nicht in der geladenen Liste.
    pub unknown: bool,
}

/// Die Auswahl fürs Drop-down: die geladene Liste; ein gespeichertes Modell, das darin fehlt,
/// bleibt vorne sichtbar (als `unbekannt` markiert), damit es nicht stillschweigend verschwindet.
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

/// Prüft einen von Hand getippten Modellnamen: nicht leer, ohne Leerraum und Steuerzeichen, nicht zu lang.
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

/// Der Hinweis unter dem Drop-down, wenn das gespeicherte Modell nicht in der Liste steht.
pub fn model_hint(list: &[String], stored: &str) -> Option<String> {
    let g = stored.trim();
    (!g.is_empty() && !list.is_empty() && !list.iter().any(|m| m == g)).then(|| {
        format!("„{g}“ steht nicht in der Liste des Endpunkts. Wähle ein Modell aus der Liste, oder lass es, wenn du den Namen sicher kennst.")
    })
}

/// Wie gut ein Punkt der Zusammenfassung steht.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    /// Läuft (grüner Haken).
    Good,
    /// Noch nicht eingerichtet oder übersprungen – kein Fehler.
    Open,
    /// Etwas ist schiefgegangen oder unvollständig.
    Problem,
}

/// Eine Zeile der Zusammenfassung.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    pub name: &'static str,
    pub tone: Tone,
    /// Klartext, was Sache ist.
    pub text: String,
    /// Der Schritt, in dem sich das beheben lässt.
    pub step: Step,
}

/// Was die Zusammenfassung wissen muss.
#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub setup: Setup,
    pub jira: ServiceStatus,
    pub github: ServiceStatus,
    pub endpoint: EndpointStatus,
    pub container: ContainerStatus,
    /// Ob der echte Agent loslaufen kann (Image gebaut, alles eingerichtet).
    pub agent: AgentStatus,
    /// Ergebnis des letzten Verbindungstests zum Endpunkt, falls schon gelaufen.
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

/// Die Jira-Zeile: Token (Standard) oder OAuth, mit der Art der Anmeldung im Text.
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
        // Der Weg über die Atlassian-App ist angefangen: wie bisher sagen, was fehlt.
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

/// Die Zusammenfassung am Ende: je Punkt ein Haken oder ein Satz, was fehlt.
pub fn summary(s: &Snapshot) -> Vec<Line> {
    let e = &s.setup;
    let container = match (&s.container.runtime, s.container.engine_running) {
        // Läuft der echte Agent, fehlt ohne Image noch ein Schritt.
        (Some(l), true) if s.agent.mode == "echt" && !s.agent.image => (
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
            _ if !s.endpoint.provider.implemented() => (
                Tone::Problem,
                s.endpoint
                    .provider
                    .not_implemented()
                    .unwrap_or_else(|| Provider::default().label().to_string()),
            ),
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

/// Ergebnis des Verbindungstests in einem Satz Klartext.
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
mod tests {
    use super::*;
    use crate::api::KeyResponse;

    #[test]
    fn steps_walk_forward_and_back() {
        assert_eq!(Step::Container.previous(), None);
        assert_eq!(Step::Container.next(), Some(Step::Jira));
        assert_eq!(Step::Done.next(), None);
        assert_eq!(Step::Done.previous(), Some(Step::Model));
        for s in Step::ALL {
            assert_eq!(Step::from_id(s.id()), Some(s));
        }
        assert_eq!(Step::from_id("unsinn"), None);
    }

    #[test]
    fn first_start_is_detected() {
        let e = Setup::default();
        let from = ServiceStatus::default();
        let l = EndpointStatus::default();
        assert!(nothing_set_up(&e, &from, &from, &l));
        let with_id = Setup {
            github_client_id: Some("Iv1".into()),
            ..Default::default()
        };
        assert!(!nothing_set_up(&with_id, &from, &from, &l));
        let an = ServiceStatus {
            connected: true,
            ..Default::default()
        };
        assert!(!nothing_set_up(&e, &an, &from, &l));
        let llm = EndpointStatus {
            key: true,
            ..Default::default()
        };
        assert!(!nothing_set_up(&e, &from, &from, &llm));
    }

    #[test]
    fn keys_are_trimmed_and_checked_without_echo() {
        assert_eq!(
            key_verify("  test-wert-123\n").unwrap(),
            "test-wert-123"
        );
        for bad in ["", "   ", "a\nb", "a\u{0}b"] {
            let error = key_verify(bad).unwrap_err();
            assert!(!error.contains('\u{0}'));
        }
        let long = "x".repeat(MAX_KEYS + 1);
        assert!(key_verify(&long).is_err());
        let msg = key_verify("geheim\nzwei").unwrap_err();
        assert!(!msg.contains("geheim"), "{msg}");
    }

    #[test]
    fn only_known_key_names() {
        for n in [
            "github-oauth",
            "atlassian-oauth",
            "agent-key",
            "litellm",
            "jira",
            "github",
        ] {
            assert!(key_name_allowed(n));
        }
        for n in ["", "github-oauth-token", "../x", "Jira", "atlassian"] {
            assert!(!key_name_allowed(n), "{n}");
        }
    }

    #[test]
    fn jira_site_in_all_spellings() {
        for ein in [
            "firma",
            "Firma.atlassian.net",
            "https://firma.atlassian.net/",
            "https://firma.atlassian.net/jira/software/projects/BAU/boards/1",
            "  firma.atlassian.net  ",
        ] {
            assert_eq!(
                jira_site_read(ein).unwrap(),
                "https://firma.atlassian.net",
                "{ein}"
            );
        }
        assert_eq!(jira_site_read("").unwrap(), "");
        // Nur der eigene Rechner geht ohne https.
        assert_eq!(
            jira_site_read("http://127.0.0.1:9911/x").unwrap(),
            "http://127.0.0.1:9911"
        );
        assert!(jira_site_read("http://127.0.0.1:").is_err());
        assert!(jira_site_read("http://127.0.0.1.evil.example").is_err());
        assert_eq!(
            jira_site_read("jira.firma.example").unwrap(),
            "https://jira.firma.example"
        );
        for bad in [
            "http://firma.atlassian.net",
            "ftp://x",
            "fir ma",
            "https://",
            "fi_rma",
        ] {
            assert!(jira_site_read(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn projects_are_uppercased_and_deduplicated() {
        assert_eq!(
            projects_read("bau, web;BAU\nops"),
            vec!["BAU", "WEB", "OPS"]
        );
        assert!(projects_read("  ").is_empty());
    }

    #[test]
    fn repos_from_names_and_urls() {
        assert_eq!(
            repos_read(
                "firma/app, https://github.com/firma/web.git\ngithub.com/firma/api/pull/3 firma/app"
            )
            .unwrap(),
            vec!["firma/app", "firma/web", "firma/api"]
        );
        assert!(repos_read("").unwrap().is_empty());
        for bad in ["app", "firma/", "/app", "fi rma/app x", "firma/ap$p"] {
            assert!(repos_read(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn endpoint_url_gets_a_scheme() {
        assert_eq!(
            endpoint_url_read("localhost:4000/").unwrap(),
            "http://localhost:4000"
        );
        assert_eq!(
            endpoint_url_read(" https://llm.firma.example ").unwrap(),
            "https://llm.firma.example"
        );
        assert_eq!(endpoint_url_read("").unwrap(), "");
        for bad in ["ftp://x", "http://", "a b", "https:///"] {
            assert!(endpoint_url_read(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn client_ids_have_no_spaces() {
        assert_eq!(client_id_read(" Iv23li ").unwrap(), "Iv23li");
        assert!(client_id_read("Iv23 li").is_err());
        assert_eq!(client_id_read("").unwrap(), "");
    }

    fn oauth(connected: bool) -> ServiceStatus {
        ServiceStatus {
            connected,
            kind: if connected {
                AuthKind::OAuth
            } else {
                AuthKind::None
            },
            ..Default::default()
        }
    }

    #[test]
    fn summary_explains_in_plain_words() {
        let empty = summary(&Snapshot::default());
        assert_eq!(empty.len(), 4);
        assert!(empty.iter().all(|z| z.tone == Tone::Open), "{empty:?}");

        // Client-ID da, Secret fehlt: ein Problem, das den Schritt nennt.
        let mut s = Snapshot::default();
        s.setup.github_client_id = Some("Iv1".into());
        let z = summary(&s);
        assert_eq!(z[2].tone, Tone::Problem);
        assert_eq!(z[2].step, Step::GitHub);
        assert!(z[2].text.contains("Client-Secret"));

        // Beides da, nur anmelden fehlt: offen mit Handlungsanweisung.
        s.setup.key = vec![KeyResponse {
            name: "github-oauth".into(),
            set: true,
        }];
        let z = summary(&s);
        assert_eq!(z[2].tone, Tone::Open);
        assert!(z[2].text.contains("Mit GitHub anmelden"));

        // Angemeldet, aber ohne Repos: Problem.
        s.github = oauth(true);
        assert_eq!(summary(&s)[2].tone, Tone::Problem);
        s.setup.github_repos = vec!["firma/app".into()];
        assert_eq!(summary(&s)[2].tone, Tone::Good);
    }

    #[test]
    fn container_and_model_lines() {
        let mut s = Snapshot::default();
        s.container.runtime = Some("docker".into());
        let z = summary(&s);
        assert_eq!(z[0].tone, Tone::Problem);
        s.container.engine_running = true;
        assert_eq!(summary(&s)[0].tone, Tone::Good);

        s.endpoint.configured = true;
        assert_eq!(summary(&s)[3].tone, Tone::Problem);
        s.endpoint.key = true;
        assert!(summary(&s)[3].text.contains("Modellname"));
        s.endpoint.model = Some("m".into());
        assert_eq!(summary(&s)[3].tone, Tone::Open);
        s.check = Some(EndpointCheck {
            configured: true,
            reachable: true,
            key_ok: true,
            model_present: true,
            hint: None,
        });
        assert_eq!(summary(&s)[3].tone, Tone::Good);
    }

    #[test]
    fn test_result_tells_where_it_failed() {
        let mut p = EndpointCheck {
            configured: true,
            ..Default::default()
        };
        assert!(check_plain_text(&p).1.contains("antwortet nicht"));
        p.reachable = true;
        assert!(check_plain_text(&p).1.contains("Key"));
        p.key_ok = true;
        assert!(check_plain_text(&p).1.contains("Modellnamen"));
        p.model_present = true;
        assert_eq!(check_plain_text(&p).0, Tone::Good);
        assert_eq!(
            check_plain_text(&EndpointCheck::default()).0,
            Tone::Open
        );
    }

    #[test]
    fn summary_asks_for_the_image_when_the_real_agent_needs_it() {
        let mut s = Snapshot::default();
        s.container.runtime = Some("docker".into());
        s.container.engine_running = true;
        // Simuliert oder unbekannt: wie bisher grün.
        assert_eq!(summary(&s)[0].tone, Tone::Good);
        s.agent.mode = "echt".into();
        let z = &summary(&s)[0];
        assert_eq!(z.tone, Tone::Open);
        assert!(z.text.contains("Image bauen"));
        s.agent.image = true;
        assert_eq!(summary(&s)[0].tone, Tone::Good);
    }

    #[test]
    fn endpoint_kinds_roundtrip_and_follow_the_url() {
        for a in EndpointKind::ALL {
            assert_eq!(EndpointKind::from_id(a.id()), Some(a));
        }
        assert_eq!(EndpointKind::from_url(""), EndpointKind::Direct);
        assert_eq!(EndpointKind::from_url("http://x:1"), EndpointKind::Proxy);
        assert!(!EndpointKind::Direct.needs_address());
        assert!(EndpointKind::Compatible.needs_address());
        assert!(!EndpointKind::Direct.explanation().contains("LiteLLM"));
    }

    #[test]
    fn model_choice_keeps_an_unknown_saved_model_visible() {
        let list = vec!["claude-a".to_string(), "gpt-x".to_string()];
        let o = model_selection(&list, "claude-a");
        assert_eq!(o.len(), 2);
        assert!(o.iter().all(|m| !m.unknown));
        assert_eq!(model_hint(&list, "claude-a"), None);

        let o = model_selection(&list, " alt-modell ");
        assert_eq!(o.len(), 3);
        assert_eq!((o[0].id.as_str(), o[0].unknown), ("alt-modell", true));
        assert!(
            model_hint(&list, "alt-modell")
                .unwrap()
                .contains("alt-modell")
        );

        // Leere Liste oder nichts gespeichert: kein Hinweis, keine Zusatzzeile.
        assert!(model_selection(&list, "").len() == 2);
        assert_eq!(model_hint(&[], "x"), None);
        assert_eq!(model_hint(&list, ""), None);
    }

    #[test]
    fn hand_typed_model_names_are_checked() {
        assert_eq!(
            model_name_read("  claude-sonnet-5-5 ").unwrap(),
            "claude-sonnet-5-5"
        );
        assert_eq!(model_name_read("").unwrap(), "");
        assert_eq!(model_name_read("a/b:c.1").unwrap(), "a/b:c.1");
        for bad in ["a b", "a\nb", &"x".repeat(201)] {
            assert!(model_name_read(bad).is_err());
        }
    }

    #[test]
    fn an_unimplemented_tool_is_a_problem_in_the_summary() {
        let mut s = Snapshot::default();
        s.endpoint.provider = Provider::Codex;
        s.endpoint.configured = true;
        let z = &summary(&s)[3];
        assert_eq!(z.tone, Tone::Problem);
        assert!(z.text.contains("Codex") && z.text.contains("noch nicht umgesetzt"));
    }

    #[test]
    fn jira_line_names_the_kind_of_login() {
        let token = ServiceStatus {
            connected: true,
            kind: AuthKind::Token,
            ..Default::default()
        };
        let mut s = Snapshot {
            jira: token,
            ..Default::default()
        };
        let z = &summary(&s)[1];
        assert_eq!((z.tone, z.name), (Tone::Good, "Jira"));
        assert!(z.text.contains("mit API-Token"), "{}", z.text);
        s.jira = oauth(true);
        let z = &summary(&s)[1];
        assert_eq!(z.tone, Tone::Good);
        assert!(z.text.contains("mit OAuth"), "{}", z.text);
    }

    #[test]
    fn jira_line_says_what_the_token_way_lacks() {
        let mut s = Snapshot::default();
        let z = &summary(&s)[1];
        assert_eq!(z.tone, Tone::Open);
        assert!(z.text.contains("API-Token"));
        s.setup.jira_site = "https://firma.atlassian.net".into();
        let z = &summary(&s)[1];
        assert!(
            z.text.contains("E-Mail-Adresse und der Token"),
            "{}",
            z.text
        );
        s.setup.jira_email = "a@b.de".into();
        let z = &summary(&s)[1];
        assert!(z.text.ends_with("der Token."), "{}", z.text);
        // Der Weg über die App bleibt erhalten.
        let mut o = Snapshot::default();
        o.setup.atlassian_client_id = Some("id".into());
        let z = &summary(&o)[1];
        assert_eq!(z.tone, Tone::Problem);
        assert!(z.text.contains("Client-Secret"));
    }

    #[test]
    fn first_start_counts_token_and_oauth_alike() {
        let from = ServiceStatus::default();
        let l = EndpointStatus::default();
        let e = Setup::default();
        for kind in [AuthKind::Token, AuthKind::OAuth] {
            let an = ServiceStatus {
                connected: true,
                kind,
                ..Default::default()
            };
            assert!(!nothing_set_up(&e, &an, &from, &l));
        }
    }

    #[test]
    fn jira_test_texts_never_mention_a_token_value() {
        for u in [
            JiraCause::Ok,
            JiraCause::Details,
            JiraCause::Address,
            JiraCause::Token,
            JiraCause::Permission,
            JiraCause::Reachable,
            JiraCause::Other,
        ] {
            let t = jira_check_text(u, Some("Ada"));
            assert!(!t.is_empty());
            assert!(!t.contains("test-wert"));
        }
        assert!(jira_check_text(JiraCause::Ok, Some("Ada")).contains("kennt dich: Ada"));
        assert!(jira_check_text(JiraCause::Ok, None).ends_with("kennt dich."));
    }

    #[test]
    fn only_tokens_can_be_deleted() {
        assert!(key_deletable("jira") && key_deletable("github"));
        for n in ["litellm", "atlassian-oauth", "github-oauth", "", "Jira"] {
            assert!(!key_deletable(n), "{n}");
        }
    }
}
