//! Der Einrichtungsassistent: Reihenfolge der Schritte, Eingaben lesen und prüfen, und die
//! Zusammenfassung in Klartext. Reine Logik ohne Oberfläche und Netzwerk; die Seite „Einrichten“
//! verdrahtet sie nur.

use crate::api::{ContainerStatus, Einrichtung, LitellmPruefung, LitellmStatus};
use crate::connections::{AuthKind, ServiceStatus};

/// Die Namen, unter denen das Backend Schlüssel im Schlüsselbund ablegt (`PUT /api/schluessel/{name}`).
pub const SCHLUESSEL_NAMEN: &[&str] = &[
    "github-oauth",
    "atlassian-oauth",
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
    LiteLlm,
    Fertig,
}

impl Schritt {
    pub const ALLE: [Schritt; 5] = [
        Schritt::Container,
        Schritt::Jira,
        Schritt::GitHub,
        Schritt::LiteLlm,
        Schritt::Fertig,
    ];

    /// Kurzer Name für die Fortschrittsanzeige.
    pub fn name(self) -> &'static str {
        match self {
            Schritt::Container => "Container",
            Schritt::Jira => "Jira",
            Schritt::GitHub => "GitHub",
            Schritt::LiteLlm => "Modell",
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
            Schritt::LiteLlm => "modell",
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
    litellm: &LitellmStatus,
) -> bool {
    !(jira.verbunden
        || github.verbunden
        || litellm.konfiguriert
        || litellm.key
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

/// Die Adresse des LiteLLM-Proxys: ohne `http://` oder `https://` wird `http://` ergänzt
/// (`localhost:4000` → `http://localhost:4000`), ein Schrägstrich am Ende fällt weg. Leer bleibt leer.
pub fn litellm_url_lesen(text: &str) -> Result<String, String> {
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
    pub litellm: LitellmStatus,
    pub container: ContainerStatus,
    /// Ergebnis des letzten Verbindungstests zum Proxy, falls schon gelaufen.
    pub pruefung: Option<LitellmPruefung>,
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
        let (ton, text) = match (&s.pruefung, s.litellm.konfiguriert) {
            (Some(p), _) => pruefung_klartext(p),
            (None, false) => (Ton::Offen, "Noch nicht eingerichtet.".to_string()),
            (None, true) if !s.litellm.key => (
                Ton::Problem,
                "Adresse eingetragen, aber der Key fehlt noch.".to_string(),
            ),
            (None, true) if s.litellm.modell.is_none() => (
                Ton::Problem,
                "Adresse und Key sind da, aber der Modellname fehlt.".to_string(),
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
            schritt: Schritt::LiteLlm,
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
pub fn pruefung_klartext(p: &LitellmPruefung) -> (Ton, String) {
    if !p.konfiguriert {
        return (Ton::Offen, "Noch keine Adresse eingetragen.".into());
    }
    if !p.erreichbar {
        return (
            Ton::Problem,
            "Der Proxy antwortet nicht. Stimmt die Adresse, und bist du im richtigen Netz oder VPN?"
                .into(),
        );
    }
    if !p.key_ok {
        return (
            Ton::Problem,
            "Der Proxy ist erreichbar, nimmt den Key aber nicht an. Prüfe, ob du den ganzen Key kopiert hast."
                .into(),
        );
    }
    if !p.modell_vorhanden {
        return (
            Ton::Problem,
            "Verbindung und Key sind in Ordnung, aber der Proxy kennt den Modellnamen nicht. Frag den Betreiber nach dem genauen Namen."
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
        assert_eq!(Schritt::Fertig.vorheriger(), Some(Schritt::LiteLlm));
        for s in Schritt::ALLE {
            assert_eq!(Schritt::aus_id(s.id()), Some(s));
        }
        assert_eq!(Schritt::aus_id("unsinn"), None);
    }

    #[test]
    fn first_start_is_detected() {
        let e = Einrichtung::default();
        let aus = ServiceStatus::default();
        let l = LitellmStatus::default();
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
        let llm = LitellmStatus {
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
    fn litellm_url_gets_a_scheme() {
        assert_eq!(
            litellm_url_lesen("localhost:4000/").unwrap(),
            "http://localhost:4000"
        );
        assert_eq!(
            litellm_url_lesen(" https://llm.firma.example ").unwrap(),
            "https://llm.firma.example"
        );
        assert_eq!(litellm_url_lesen("").unwrap(), "");
        for schlecht in ["ftp://x", "http://", "a b", "https:///"] {
            assert!(litellm_url_lesen(schlecht).is_err(), "{schlecht}");
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

        s.litellm.konfiguriert = true;
        assert_eq!(zusammenfassung(&s)[3].ton, Ton::Problem);
        s.litellm.key = true;
        assert!(zusammenfassung(&s)[3].text.contains("Modellname"));
        s.litellm.modell = Some("m".into());
        assert_eq!(zusammenfassung(&s)[3].ton, Ton::Offen);
        s.pruefung = Some(LitellmPruefung {
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
        let mut p = LitellmPruefung {
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
        assert_eq!(pruefung_klartext(&LitellmPruefung::default()).0, Ton::Offen);
    }
}
