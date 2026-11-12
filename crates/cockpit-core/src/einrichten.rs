//! Der Einrichtungsassistent: Reihenfolge der Schritte, Eingaben lesen und prüfen, und die
//! Zusammenfassung in Klartext. Reine Logik ohne Oberfläche und Netzwerk; die Seite „Einrichten“
//! verdrahtet sie nur.

use crate::api::{
    AgentStatus, Anbieter, ContainerStatus, Einrichtung, EndpunktPruefung, EndpunktStatus,
};
use crate::connections::{AuthKind, ServiceStatus};

/// Die Namen, unter denen das Backend Schlüssel im Schlüsselbund ablegt (`PUT /api/schluessel/{name}`).
pub const SCHLUESSEL_NAMEN: &[&str] = &[
    "github-oauth",
    "atlassian-oauth",
    "agent-key",
    "litellm",
    "jira",
    "github",
];

/// Längster erlaubter Schlüssel (Zeichen). Echte Tokens sind weit kürzer.
pub const MAX_SCHLUESSEL: usize = 4096;

/// Ein Schritt des Assistenten.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Schritt {
    Container,
    Jira,
    GitHub,
    Modell,
    Fertig,
}

impl Schritt {
    pub const ALLE: [Schritt; 5] = [
        Schritt::Container,
        Schritt::Jira,
        Schritt::GitHub,
        Schritt::Modell,
        Schritt::Fertig,
    ];

    /// Kurzer Name für die Fortschrittsanzeige.
    pub fn name(self) -> &'static str {
        match self {
            Schritt::Container => "Container",
            Schritt::Jira => "Jira",
            Schritt::GitHub => "GitHub",
            Schritt::Modell => "Modell",
            Schritt::Fertig => "Fertig",
        }
    }

    pub fn index(self) -> usize {
        Self::ALLE.iter().position(|s| *s == self).unwrap_or(0)
    }

    pub fn naechster(self) -> Option<Schritt> {
        Self::ALLE.get(self.index() + 1).copied()
    }

    pub fn vorheriger(self) -> Option<Schritt> {
        self.index().checked_sub(1).map(|i| Self::ALLE[i])
    }

    /// Kennung für die Adresse (`#/einrichten/jira`).
    pub fn id(self) -> &'static str {
        match self {
            Schritt::Container => "container",
            Schritt::Jira => "jira",
            Schritt::GitHub => "github",
            Schritt::Modell => "modell",
            Schritt::Fertig => "fertig",
        }
    }

    pub fn aus_id(id: &str) -> Option<Schritt> {
        Self::ALLE.into_iter().find(|s| s.id() == id)
    }
}

/// `true`, wenn noch gar nichts eingerichtet ist – dann erscheint der Assistent beim ersten Start.
pub fn nichts_eingerichtet(
    einrichtung: &Einrichtung,
    jira: &ServiceStatus,
    github: &ServiceStatus,
    endpunkt: &EndpunktStatus,
) -> bool {
    !(jira.verbunden
        || github.verbunden
        || endpunkt.konfiguriert
        || endpunkt.key
        || einrichtung.atlassian_client_id.is_some()
        || einrichtung.github_client_id.is_some()
        || !einrichtung.github_repos.is_empty()
        || !einrichtung.jira_site.is_empty())
}

/// Prüft einen Schlüssel vor dem Senden. Gibt den bereinigten Wert zurück (Leerraum am Rand
/// entfernt, z. B. vom Einfügen). Die Meldungen nennen nie den Wert.
pub fn schluessel_pruefen(wert: &str) -> Result<String, String> {
    let wert = wert.trim();
    if wert.is_empty() {
        return Err("Das Feld ist leer.".into());
    }
    if wert.chars().count() > MAX_SCHLUESSEL {
        return Err("Der Schlüssel ist ungewöhnlich lang – wurde zu viel eingefügt?".into());
    }
    if wert.chars().any(char::is_control) {
        return Err(
            "Der Schlüssel enthält Zeilenumbrüche oder Steuerzeichen – bitte nur den Schlüssel selbst einfügen."
                .into(),
        );
    }
    Ok(wert.to_string())
}

/// Ob `name` ein erlaubter Schlüsselname ist.
pub fn schluessel_name_erlaubt(name: &str) -> bool {
    SCHLUESSEL_NAMEN.contains(&name)
}

/// Die Site einer Jira-Cloud-Instanz als Adresse: `firma` → `https://firma.atlassian.net`,
/// `firma.atlassian.net` und `https://firma.atlassian.net/jira/…` → `https://firma.atlassian.net`.
/// Leer bleibt leer.
pub fn jira_site_lesen(text: &str) -> Result<String, String> {
    let t = text.trim();
    if t.is_empty() {
        return Ok(String::new());
    }
    if t.chars().any(char::is_whitespace) {
        return Err("Die Adresse darf keine Leerzeichen enthalten.".into());
    }
    let (schema, rest) = match t.split_once("://") {
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
    let gueltig = host
        .split('.')
        .all(|t| !t.is_empty() && t.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'));
    if !gueltig {
        return Err(format!(
            "„{host}“ sieht nicht wie eine Jira-Adresse aus, z. B. firma.atlassian.net."
        ));
    }
    Ok(format!("{schema}://{host}"))
}

/// Jira-Projektschlüssel aus einem Text („bau, web“ oder eine Zeile je Projekt), groß geschrieben
/// und ohne Doppelte.
pub fn projekte_lesen(text: &str) -> Vec<String> {
    let mut aus: Vec<String> = Vec::new();
    for teil in text.split(|c: char| c == ',' || c == ';' || c.is_whitespace()) {
        let p = teil.trim().to_uppercase();
        if !p.is_empty() && !aus.contains(&p) {
            aus.push(p);
        }
    }
    aus
}

/// GitHub-Repos aus einem Text. Erlaubt `owner/name`, ganze Adressen (`https://github.com/o/n`,
/// auch mit `.git`) und Trennung durch Komma, Leerzeichen oder Zeilen.
pub fn repos_lesen(text: &str) -> Result<Vec<String>, String> {
    let mut aus: Vec<String> = Vec::new();
    for teil in text.split(|c: char| c == ',' || c == ';' || c.is_whitespace()) {
        let mut r = teil.trim();
        if r.is_empty() {
            continue;
        }
        for vorsatz in ["https://github.com/", "http://github.com/", "github.com/"] {
            if let Some(rest) = r.strip_prefix(vorsatz) {
                r = rest;
                break;
            }
        }
        let r = r.trim_end_matches('/');
        let r = r.strip_suffix(".git").unwrap_or(r);
        // Bei einer ganzen Adresse zählen nur die ersten beiden Teile (`…/o/n/pull/3`).
        let mut teile = r.split('/');
        let (Some(o), Some(n)) = (teile.next(), teile.next()) else {
            return Err(format!(
                "„{teil}“ ist kein Repo – gebraucht wird die Form firma/app."
            ));
        };
        let zeichen = |t: &str| {
            !t.is_empty()
                && t.chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
        };
        if !zeichen(o) || !zeichen(n) {
            return Err(format!(
                "„{teil}“ ist kein Repo – gebraucht wird die Form firma/app."
            ));
        }
        let repo = format!("{o}/{n}");
        if !aus.contains(&repo) {
            aus.push(repo);
        }
    }
    Ok(aus)
}

/// Eine Client-ID aus dem Eingabefeld: ohne Leerraum am Rand, ohne Leerzeichen darin.
pub fn client_id_lesen(text: &str) -> Result<String, String> {
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
pub fn endpunkt_url_lesen(text: &str) -> Result<String, String> {
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
pub enum EndpunktArt {
    /// Direkt bei Anthropic (keine Adresse, Key von der Anthropic-Konsole).
    Direkt,
    /// Ein eigener Proxy wie LiteLLM.
    Proxy,
    /// Ein anderer Anthropic-kompatibler Endpunkt.
    Kompatibel,
}

impl EndpunktArt {
    pub const ALLE: [EndpunktArt; 3] = [
        EndpunktArt::Direkt,
        EndpunktArt::Proxy,
        EndpunktArt::Kompatibel,
    ];

    pub fn id(self) -> &'static str {
        match self {
            EndpunktArt::Direkt => "direkt",
            EndpunktArt::Proxy => "proxy",
            EndpunktArt::Kompatibel => "kompatibel",
        }
    }

    pub fn aus_id(id: &str) -> Option<EndpunktArt> {
        Self::ALLE.into_iter().find(|a| a.id() == id)
    }

    pub fn bezeichnung(self) -> &'static str {
        match self {
            EndpunktArt::Direkt => "Direkt bei Anthropic",
            EndpunktArt::Proxy => "Eigener Proxy (z. B. LiteLLM)",
            EndpunktArt::Kompatibel => "Anderer kompatibler Endpunkt",
        }
    }

    /// Was in der gespeicherten Konfiguration steht: ohne Adresse direkt, sonst ein Proxy.
    pub fn aus_url(url: &str) -> EndpunktArt {
        if url.trim().is_empty() {
            EndpunktArt::Direkt
        } else {
            EndpunktArt::Proxy
        }
    }

    /// Braucht diese Art eine Adresse?
    pub fn braucht_adresse(self) -> bool {
        self != EndpunktArt::Direkt
    }

    /// Erklärung unter der Auswahl.
    pub fn erklaerung(self) -> &'static str {
        match self {
            EndpunktArt::Direkt => {
                "Der Agent spricht ohne Zwischenstation mit Anthropic. Du brauchst einen API-Key aus der Anthropic-Konsole."
            }
            EndpunktArt::Proxy => {
                "Eine Zwischenstation, die dein Team betreibt (z. B. LiteLLM). Du bekommst Adresse und Key vom Betreiber."
            }
            EndpunktArt::Kompatibel => {
                "Ein Dienst, der die Schnittstelle von Anthropic nachbildet. Trage seine Adresse und seinen Key ein."
            }
        }
    }

    pub fn url_platzhalter(self) -> &'static str {
        match self {
            EndpunktArt::Direkt => "",
            EndpunktArt::Proxy => "https://litellm.example",
            EndpunktArt::Kompatibel => "https://endpunkt.example",
        }
    }

    pub fn key_bezeichnung(self) -> &'static str {
        match self {
            EndpunktArt::Direkt => "Anthropic-API-Key",
            EndpunktArt::Proxy => "Key des Proxys",
            EndpunktArt::Kompatibel => "Key des Endpunkts",
        }
    }
}

/// Eine Zeile des Drop-downs für den Modellnamen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModellOption {
    pub id: String,
    /// Das gespeicherte Modell steht nicht in der geladenen Liste.
    pub unbekannt: bool,
}

/// Die Auswahl fürs Drop-down: die geladene Liste; ein gespeichertes Modell, das darin fehlt,
/// bleibt vorne sichtbar (als `unbekannt` markiert), damit es nicht stillschweigend verschwindet.
pub fn modell_auswahl(liste: &[String], gespeichert: &str) -> Vec<ModellOption> {
    let gespeichert = gespeichert.trim();
    let mut aus: Vec<ModellOption> = Vec::with_capacity(liste.len() + 1);
    if !gespeichert.is_empty() && !liste.iter().any(|m| m == gespeichert) {
        aus.push(ModellOption {
            id: gespeichert.to_string(),
            unbekannt: true,
        });
    }
    aus.extend(liste.iter().map(|m| ModellOption {
        id: m.clone(),
        unbekannt: false,
    }));
    aus
}

/// Prüft einen von Hand getippten Modellnamen: nicht leer, ohne Leerraum und Steuerzeichen, nicht zu lang.
pub fn modellname_lesen(text: &str) -> Result<String, String> {
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
pub fn modell_hinweis(liste: &[String], gespeichert: &str) -> Option<String> {
    let g = gespeichert.trim();
    (!g.is_empty() && !liste.is_empty() && !liste.iter().any(|m| m == g)).then(|| {
        format!("„{g}“ steht nicht in der Liste des Endpunkts. Wähle ein Modell aus der Liste, oder lass es, wenn du den Namen sicher kennst.")
    })
}

/// Wie gut ein Punkt der Zusammenfassung steht.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ton {
    /// Läuft (grüner Haken).
    Gut,
    /// Noch nicht eingerichtet oder übersprungen – kein Fehler.
    Offen,
    /// Etwas ist schiefgegangen oder unvollständig.
    Problem,
}

/// Eine Zeile der Zusammenfassung.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Zeile {
    pub name: &'static str,
    pub ton: Ton,
    /// Klartext, was Sache ist.
    pub text: String,
    /// Der Schritt, in dem sich das beheben lässt.
    pub schritt: Schritt,
}

/// Was die Zusammenfassung wissen muss.
#[derive(Clone, Debug, Default)]
pub struct Stand {
    pub einrichtung: Einrichtung,
    pub jira: ServiceStatus,
    pub github: ServiceStatus,
    pub endpunkt: EndpunktStatus,
    pub container: ContainerStatus,
    /// Ob der echte Agent loslaufen kann (Image gebaut, alles eingerichtet).
    pub agent: AgentStatus,
    /// Ergebnis des letzten Verbindungstests zum Endpunkt, falls schon gelaufen.
    pub pruefung: Option<EndpunktPruefung>,
}

fn dienst_zeile(
    name: &'static str,
    schritt: Schritt,
    status: &ServiceStatus,
    client_id: bool,
    secret: bool,
    token_weg: &str,
) -> Zeile {
    let (ton, text) = match (status.verbunden, status.art) {
        (true, AuthKind::OAuth) => (Ton::Gut, format!("Verbunden – du bist über {name} angemeldet.")),
        (true, _) => (Ton::Gut, "Verbunden – mit einem Zugangs-Token.".to_string()),
        (false, _) if !client_id && !secret => (
            Ton::Offen,
            format!("Noch nicht eingerichtet. {token_weg}"),
        ),
        (false, _) if !client_id => (
            Ton::Problem,
            "Die Client-ID fehlt noch. Du findest sie in der Entwicklerseite deiner App.".into(),
        ),
        (false, _) if !secret => (
            Ton::Problem,
            "Das Client-Secret fehlt noch. Es wird nur einmal angezeigt – ist es weg, erzeuge in deiner App ein neues."
                .into(),
        ),
        (false, _) => (
            Ton::Offen,
            format!("Fast fertig: Klicke auf „Mit {name} anmelden“."),
        ),
    };
    Zeile {
        name,
        ton,
        text,
        schritt,
    }
}

/// Die Zusammenfassung am Ende: je Punkt ein Haken oder ein Satz, was fehlt.
pub fn zusammenfassung(s: &Stand) -> Vec<Zeile> {
    let e = &s.einrichtung;
    let container = match (&s.container.laufzeit, s.container.engine_laeuft) {
        // Läuft der echte Agent, fehlt ohne Image noch ein Schritt.
        (Some(l), true) if s.agent.modus == "echt" && !s.agent.image => (
            Ton::Offen,
            format!(
                "{l} läuft. Das Agent-Image ist noch nicht gebaut – klicke im Schritt „Container“ auf „Image bauen“."
            ),
        ),
        (Some(l), true) => (Ton::Gut, format!("{l} ist installiert und läuft.")),
        (Some(l), false) => (
            Ton::Problem,
            format!(
                "{l} ist installiert, aber nicht gestartet. Öffne das Programm und warte, bis es bereit ist."
            ),
        ),
        (None, _) => (
            Ton::Offen,
            "Noch kein Docker oder Podman gefunden. Du brauchst es erst für den Agenten.".into(),
        ),
    };
    let mut jira = dienst_zeile(
        "Atlassian",
        Schritt::Jira,
        &s.jira,
        e.atlassian_client_id.is_some(),
        e.hat_schluessel("atlassian-oauth"),
        if e.jira_site.is_empty() {
            "Dafür brauchst du einen Atlassian-Zugang."
        } else {
            "Die Anmeldung per Token ist nicht abgeschlossen."
        },
    );
    jira.name = "Jira";
    let mut github = dienst_zeile(
        "GitHub",
        Schritt::GitHub,
        &s.github,
        e.github_client_id.is_some(),
        e.hat_schluessel("github-oauth"),
        "Dafür brauchst du eine GitHub App.",
    );
    github.name = "GitHub";
    if github.ton == Ton::Gut && e.github_repos.is_empty() {
        github.ton = Ton::Problem;
        github.text =
            "Angemeldet, aber ohne Repos: Trage ein, welche Repos Cockpit abgleichen soll.".into();
    }

    let modell = {
        let (ton, text) = match (&s.pruefung, s.endpunkt.konfiguriert || s.endpunkt.key) {
            _ if !s.endpunkt.anbieter.umgesetzt() => (
                Ton::Problem,
                s.endpunkt
                    .anbieter
                    .nicht_umgesetzt()
                    .unwrap_or_else(|| Anbieter::default().bezeichnung().to_string()),
            ),
            (Some(p), _) => pruefung_klartext(p),
            (None, false) => (Ton::Offen, "Noch nicht eingerichtet.".to_string()),
            (None, true) if !s.endpunkt.key => (
                Ton::Problem,
                "Eingetragen, aber der Key fehlt noch.".to_string(),
            ),
            (None, true) if s.endpunkt.modell.is_none() => (
                Ton::Problem,
                "Key ist da, aber der Modellname fehlt noch.".to_string(),
            ),
            (None, true) => (
                Ton::Offen,
                "Eingetragen, aber noch nicht getestet. Starte den Verbindungstest.".to_string(),
            ),
        };
        Zeile {
            name: "Modellzugang",
            ton,
            text,
            schritt: Schritt::Modell,
        }
    };

    vec![
        Zeile {
            name: "Container",
            ton: container.0,
            text: container.1,
            schritt: Schritt::Container,
        },
        jira,
        github,
        modell,
    ]
}

/// Ergebnis des Verbindungstests in einem Satz Klartext.
pub fn pruefung_klartext(p: &EndpunktPruefung) -> (Ton, String) {
    if !p.konfiguriert {
        return (Ton::Offen, "Noch nichts eingetragen.".into());
    }
    if !p.erreichbar {
        return (
            Ton::Problem,
            "Der Endpunkt antwortet nicht. Stimmt die Adresse, und bist du im richtigen Netz oder VPN?"
                .into(),
        );
    }
    if !p.key_ok {
        return (
            Ton::Problem,
            "Der Endpunkt ist erreichbar, nimmt den Key aber nicht an. Prüfe, ob du den ganzen Key kopiert hast."
                .into(),
        );
    }
    if !p.modell_vorhanden {
        return (
            Ton::Problem,
            "Verbindung und Key sind in Ordnung, aber der Endpunkt kennt den Modellnamen nicht. Lade die Modelle und wähle eines aus der Liste."
                .into(),
        );
    }
    (
        Ton::Gut,
        "Verbindung, Key und Modell sind in Ordnung.".into(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::SchluesselAntwort;

    #[test]
    fn steps_walk_forward_and_back() {
        assert_eq!(Schritt::Container.vorheriger(), None);
        assert_eq!(Schritt::Container.naechster(), Some(Schritt::Jira));
        assert_eq!(Schritt::Fertig.naechster(), None);
        assert_eq!(Schritt::Fertig.vorheriger(), Some(Schritt::Modell));
        for s in Schritt::ALLE {
            assert_eq!(Schritt::aus_id(s.id()), Some(s));
        }
        assert_eq!(Schritt::aus_id("unsinn"), None);
    }

    #[test]
    fn first_start_is_detected() {
        let e = Einrichtung::default();
        let aus = ServiceStatus::default();
        let l = EndpunktStatus::default();
        assert!(nichts_eingerichtet(&e, &aus, &aus, &l));
        let mit_id = Einrichtung {
            github_client_id: Some("Iv1".into()),
            ..Default::default()
        };
        assert!(!nichts_eingerichtet(&mit_id, &aus, &aus, &l));
        let an = ServiceStatus {
            verbunden: true,
            ..Default::default()
        };
        assert!(!nichts_eingerichtet(&e, &an, &aus, &l));
        let llm = EndpunktStatus {
            key: true,
            ..Default::default()
        };
        assert!(!nichts_eingerichtet(&e, &aus, &aus, &llm));
    }

    #[test]
    fn keys_are_trimmed_and_checked_without_echo() {
        assert_eq!(
            schluessel_pruefen("  test-wert-123\n").unwrap(),
            "test-wert-123"
        );
        for schlecht in ["", "   ", "a\nb", "a\u{0}b"] {
            let fehler = schluessel_pruefen(schlecht).unwrap_err();
            assert!(!fehler.contains('\u{0}'));
        }
        let lang = "x".repeat(MAX_SCHLUESSEL + 1);
        assert!(schluessel_pruefen(&lang).is_err());
        let msg = schluessel_pruefen("geheim\nzwei").unwrap_err();
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
            assert!(schluessel_name_erlaubt(n));
        }
        for n in ["", "github-oauth-token", "../x", "Jira", "atlassian"] {
            assert!(!schluessel_name_erlaubt(n), "{n}");
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
                jira_site_lesen(ein).unwrap(),
                "https://firma.atlassian.net",
                "{ein}"
            );
        }
        assert_eq!(jira_site_lesen("").unwrap(), "");
        assert_eq!(
            jira_site_lesen("jira.firma.example").unwrap(),
            "https://jira.firma.example"
        );
        for schlecht in [
            "http://firma.atlassian.net",
            "ftp://x",
            "fir ma",
            "https://",
            "fi_rma",
        ] {
            assert!(jira_site_lesen(schlecht).is_err(), "{schlecht}");
        }
    }

    #[test]
    fn projects_are_uppercased_and_deduplicated() {
        assert_eq!(
            projekte_lesen("bau, web;BAU\nops"),
            vec!["BAU", "WEB", "OPS"]
        );
        assert!(projekte_lesen("  ").is_empty());
    }

    #[test]
    fn repos_from_names_and_urls() {
        assert_eq!(
            repos_lesen(
                "firma/app, https://github.com/firma/web.git\ngithub.com/firma/api/pull/3 firma/app"
            )
            .unwrap(),
            vec!["firma/app", "firma/web", "firma/api"]
        );
        assert!(repos_lesen("").unwrap().is_empty());
        for schlecht in ["app", "firma/", "/app", "fi rma/app x", "firma/ap$p"] {
            assert!(repos_lesen(schlecht).is_err(), "{schlecht}");
        }
    }

    #[test]
    fn endpoint_url_gets_a_scheme() {
        assert_eq!(
            endpunkt_url_lesen("localhost:4000/").unwrap(),
            "http://localhost:4000"
        );
        assert_eq!(
            endpunkt_url_lesen(" https://llm.firma.example ").unwrap(),
            "https://llm.firma.example"
        );
        assert_eq!(endpunkt_url_lesen("").unwrap(), "");
        for schlecht in ["ftp://x", "http://", "a b", "https:///"] {
            assert!(endpunkt_url_lesen(schlecht).is_err(), "{schlecht}");
        }
    }

    #[test]
    fn client_ids_have_no_spaces() {
        assert_eq!(client_id_lesen(" Iv23li ").unwrap(), "Iv23li");
        assert!(client_id_lesen("Iv23 li").is_err());
        assert_eq!(client_id_lesen("").unwrap(), "");
    }

    fn oauth(verbunden: bool) -> ServiceStatus {
        ServiceStatus {
            verbunden,
            art: if verbunden {
                AuthKind::OAuth
            } else {
                AuthKind::None
            },
            ..Default::default()
        }
    }

    #[test]
    fn summary_explains_in_plain_words() {
        let leer = zusammenfassung(&Stand::default());
        assert_eq!(leer.len(), 4);
        assert!(leer.iter().all(|z| z.ton == Ton::Offen), "{leer:?}");

        // Client-ID da, Secret fehlt: ein Problem, das den Schritt nennt.
        let mut s = Stand::default();
        s.einrichtung.github_client_id = Some("Iv1".into());
        let z = zusammenfassung(&s);
        assert_eq!(z[2].ton, Ton::Problem);
        assert_eq!(z[2].schritt, Schritt::GitHub);
        assert!(z[2].text.contains("Client-Secret"));

        // Beides da, nur anmelden fehlt: offen mit Handlungsanweisung.
        s.einrichtung.schluessel = vec![SchluesselAntwort {
            name: "github-oauth".into(),
            gesetzt: true,
        }];
        let z = zusammenfassung(&s);
        assert_eq!(z[2].ton, Ton::Offen);
        assert!(z[2].text.contains("Mit GitHub anmelden"));

        // Angemeldet, aber ohne Repos: Problem.
        s.github = oauth(true);
        assert_eq!(zusammenfassung(&s)[2].ton, Ton::Problem);
        s.einrichtung.github_repos = vec!["firma/app".into()];
        assert_eq!(zusammenfassung(&s)[2].ton, Ton::Gut);
    }

    #[test]
    fn container_and_model_lines() {
        let mut s = Stand::default();
        s.container.laufzeit = Some("docker".into());
        let z = zusammenfassung(&s);
        assert_eq!(z[0].ton, Ton::Problem);
        s.container.engine_laeuft = true;
        assert_eq!(zusammenfassung(&s)[0].ton, Ton::Gut);

        s.endpunkt.konfiguriert = true;
        assert_eq!(zusammenfassung(&s)[3].ton, Ton::Problem);
        s.endpunkt.key = true;
        assert!(zusammenfassung(&s)[3].text.contains("Modellname"));
        s.endpunkt.modell = Some("m".into());
        assert_eq!(zusammenfassung(&s)[3].ton, Ton::Offen);
        s.pruefung = Some(EndpunktPruefung {
            konfiguriert: true,
            erreichbar: true,
            key_ok: true,
            modell_vorhanden: true,
            hinweis: None,
        });
        assert_eq!(zusammenfassung(&s)[3].ton, Ton::Gut);
    }

    #[test]
    fn test_result_tells_where_it_failed() {
        let mut p = EndpunktPruefung {
            konfiguriert: true,
            ..Default::default()
        };
        assert!(pruefung_klartext(&p).1.contains("antwortet nicht"));
        p.erreichbar = true;
        assert!(pruefung_klartext(&p).1.contains("Key"));
        p.key_ok = true;
        assert!(pruefung_klartext(&p).1.contains("Modellnamen"));
        p.modell_vorhanden = true;
        assert_eq!(pruefung_klartext(&p).0, Ton::Gut);
        assert_eq!(
            pruefung_klartext(&EndpunktPruefung::default()).0,
            Ton::Offen
        );
    }

    #[test]
    fn summary_asks_for_the_image_when_the_real_agent_needs_it() {
        let mut s = Stand::default();
        s.container.laufzeit = Some("docker".into());
        s.container.engine_laeuft = true;
        // Simuliert oder unbekannt: wie bisher grün.
        assert_eq!(zusammenfassung(&s)[0].ton, Ton::Gut);
        s.agent.modus = "echt".into();
        let z = &zusammenfassung(&s)[0];
        assert_eq!(z.ton, Ton::Offen);
        assert!(z.text.contains("Image bauen"));
        s.agent.image = true;
        assert_eq!(zusammenfassung(&s)[0].ton, Ton::Gut);
    }

    #[test]
    fn endpoint_kinds_roundtrip_and_follow_the_url() {
        for a in EndpunktArt::ALLE {
            assert_eq!(EndpunktArt::aus_id(a.id()), Some(a));
        }
        assert_eq!(EndpunktArt::aus_url(""), EndpunktArt::Direkt);
        assert_eq!(EndpunktArt::aus_url("http://x:1"), EndpunktArt::Proxy);
        assert!(!EndpunktArt::Direkt.braucht_adresse());
        assert!(EndpunktArt::Kompatibel.braucht_adresse());
        assert!(!EndpunktArt::Direkt.erklaerung().contains("LiteLLM"));
    }

    #[test]
    fn model_choice_keeps_an_unknown_saved_model_visible() {
        let liste = vec!["claude-a".to_string(), "gpt-x".to_string()];
        let o = modell_auswahl(&liste, "claude-a");
        assert_eq!(o.len(), 2);
        assert!(o.iter().all(|m| !m.unbekannt));
        assert_eq!(modell_hinweis(&liste, "claude-a"), None);

        let o = modell_auswahl(&liste, " alt-modell ");
        assert_eq!(o.len(), 3);
        assert_eq!((o[0].id.as_str(), o[0].unbekannt), ("alt-modell", true));
        assert!(
            modell_hinweis(&liste, "alt-modell")
                .unwrap()
                .contains("alt-modell")
        );

        // Leere Liste oder nichts gespeichert: kein Hinweis, keine Zusatzzeile.
        assert!(modell_auswahl(&liste, "").len() == 2);
        assert_eq!(modell_hinweis(&[], "x"), None);
        assert_eq!(modell_hinweis(&liste, ""), None);
    }

    #[test]
    fn hand_typed_model_names_are_checked() {
        assert_eq!(
            modellname_lesen("  claude-sonnet-5-5 ").unwrap(),
            "claude-sonnet-5-5"
        );
        assert_eq!(modellname_lesen("").unwrap(), "");
        assert_eq!(modellname_lesen("a/b:c.1").unwrap(), "a/b:c.1");
        for schlecht in ["a b", "a\nb", &"x".repeat(201)] {
            assert!(modellname_lesen(schlecht).is_err());
        }
    }

    #[test]
    fn an_unimplemented_tool_is_a_problem_in_the_summary() {
        let mut s = Stand::default();
        s.endpunkt.anbieter = Anbieter::Codex;
        s.endpunkt.konfiguriert = true;
        let z = &zusammenfassung(&s)[3];
        assert_eq!(z.ton, Ton::Problem);
        assert!(z.text.contains("Codex") && z.text.contains("noch nicht umgesetzt"));
    }
}
