//! Der echte Agent, reiner Teil: Protokoll des Läufers, Befehlszeile für `docker`/`podman`, der
//! Auftrag an den Container, die Voraussetzungen und der Plan, was der Aufpasser als Nächstes tut.
//! Nichts hier startet einen Prozess oder fasst die Platte an – das macht `agent_betrieb`.
//!
//! Regeln für Container (siehe „Container“ in `docs/ROADMAP.md`): nur Befehle und Schalter, die
//! Docker und Podman gleich verstehen; Geheimnisse nur als `-e NAME` (der Wert steht in der
//! Umgebung des Kindprozesses, nie in der Argumentliste); `--rm`; ein Label je Lauf zum Aufräumen;
//! `--add-host cockpit-host:host-gateway`, um Dienste auf dem Rechner zu erreichen.

use std::collections::HashMap;
use std::path::Path;

use cockpit_core::api::{AgentStatus, Anbieter, ContainerStatus};
use cockpit_core::{AgentReport, AgentRun, RunKind, RunState, Workspace};
use serde::Deserialize;
use serde_json::{Value, json};

/// Name des Agent-Images (voll benannt, damit Podman es nicht erst im Netz sucht).
pub const IMAGE: &str = "localhost/cockpit-agent:1";
/// Label, an dem das Backend seine Container wiederfindet und aufräumt.
pub const LABEL: &str = "cockpit.lauf";
/// Name, unter dem der Container den Rechner erreicht.
pub const HOST_NAME: &str = "cockpit-host";
/// Längere Zeilen des Läufers verwirft das Backend.
pub const MAX_ZEILE: usize = 64 * 1024;
const MAX_TEXT: usize = 2_000;
const MAX_LISTE: usize = 200;
/// Mehr Tokens oder Zeit meldet kein einzelner Schritt glaubhaft.
const MAX_TOKEN: u64 = 50_000_000;
const MAX_MS: u64 = 3_600_000;

/// Was ein Container tut.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Modus {
    /// Nur lesen und einen Plan vorlegen.
    Plan,
    /// Das Ticket umsetzen (nach der Freigabe) und committen.
    Umsetzen,
}

impl Modus {
    pub fn kennung(self) -> &'static str {
        match self {
            Modus::Plan => "plan",
            Modus::Umsetzen => "umsetzen",
        }
    }
}

// ---------- Protokoll: Zeilen des Läufers ----------

/// Ein Ereignis des Läufers (eine JSON-Zeile auf stdout). Siehe `agent/README.md`.
#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(tag = "t")]
pub enum Ereignis {
    #[serde(rename = "plan")]
    Plan {
        summary: String,
        #[serde(default)]
        files: Vec<String>,
        #[serde(default)]
        steps: Vec<String>,
    },
    #[serde(rename = "schritt")]
    Schritt {
        titel: String,
        #[serde(default)]
        text: String,
        #[serde(default)]
        token_ein: u64,
        #[serde(default)]
        token_aus: u64,
        #[serde(default)]
        ms: u64,
    },
    #[serde(rename = "dateien")]
    Dateien { dateien: Vec<String> },
    #[serde(rename = "frage")]
    Frage {
        text: String,
        #[serde(default)]
        vorschlaege: Vec<String>,
    },
    #[serde(rename = "hinweise_angekommen")]
    HinweiseAngekommen,
    #[serde(rename = "ergebnis")]
    Ergebnis {
        text: String,
        branch: String,
        commit: String,
    },
    #[serde(rename = "pruefung")]
    Pruefung {
        erreichbar: bool,
        key_ok: bool,
        #[serde(default)]
        hinweis: String,
    },
    #[serde(rename = "fehler")]
    Fehler { text: String },
}

/// Warum eine Zeile nicht angenommen wurde.
#[derive(Debug, PartialEq, Eq)]
pub enum Verworfen {
    ZuGross,
    Unlesbar,
}

fn kuerzen(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut t: String = text.chars().take(max.saturating_sub(1)).collect();
    t.push('…');
    t
}

fn liste(v: Vec<String>) -> Vec<String> {
    v.into_iter()
        .take(MAX_LISTE)
        .map(|s| kuerzen(&s, 300))
        .collect()
}

/// Liest eine Zeile des Läufers. Zu große, unlesbare oder unbekannte Zeilen werden verworfen;
/// Texte, Listen und Zahlen sind danach begrenzt. Das Backend vertraut dem Container nicht.
pub fn zeile_lesen(zeile: &str) -> Result<Ereignis, Verworfen> {
    if zeile.len() > MAX_ZEILE {
        return Err(Verworfen::ZuGross);
    }
    let e: Ereignis = serde_json::from_str(zeile.trim()).map_err(|_| Verworfen::Unlesbar)?;
    Ok(match e {
        Ereignis::Plan {
            summary,
            files,
            steps,
        } => Ereignis::Plan {
            summary: kuerzen(&summary, MAX_TEXT),
            files: liste(files),
            steps: liste(steps),
        },
        Ereignis::Schritt {
            titel,
            text,
            token_ein,
            token_aus,
            ms,
        } => Ereignis::Schritt {
            titel: kuerzen(&titel, 120),
            text: kuerzen(&text, 300),
            token_ein: token_ein.min(MAX_TOKEN),
            token_aus: token_aus.min(MAX_TOKEN),
            ms: ms.min(MAX_MS),
        },
        Ereignis::Dateien { dateien } => Ereignis::Dateien {
            dateien: liste(dateien),
        },
        Ereignis::Frage { text, vorschlaege } => Ereignis::Frage {
            text: kuerzen(&text, MAX_TEXT),
            vorschlaege: liste(vorschlaege).into_iter().take(6).collect(),
        },
        Ereignis::Ergebnis {
            text,
            branch,
            commit,
        } => Ereignis::Ergebnis {
            text: kuerzen(&text, 1_500),
            branch: kuerzen(&branch, 200),
            commit: kuerzen(&commit, 64),
        },
        Ereignis::Pruefung {
            erreichbar,
            key_ok,
            hinweis,
        } => Ereignis::Pruefung {
            erreichbar,
            key_ok,
            hinweis: kuerzen(&hinweis, 300),
        },
        Ereignis::Fehler { text } => Ereignis::Fehler {
            text: kuerzen(&text, MAX_TEXT),
        },
        gleich @ Ereignis::HinweiseAngekommen => gleich,
    })
}

/// Was ein Ereignis am Lauf ändert. `None` für Ereignisse, die nicht (oder in diesem Modus nicht)
/// in den Lauf gehören: Ergebnis und Fehler wertet der Betrieb selbst aus.
pub fn als_meldung(e: &Ereignis, modus: Modus) -> Option<AgentReport> {
    match (e, modus) {
        (
            Ereignis::Plan {
                summary,
                files,
                steps,
            },
            Modus::Plan,
        ) => Some(AgentReport::Plan {
            summary: summary.clone(),
            files: files.clone(),
            steps: steps.clone(),
        }),
        (
            Ereignis::Schritt {
                titel,
                text,
                token_ein,
                token_aus,
                ms,
            },
            Modus::Plan | Modus::Umsetzen,
        ) => Some(AgentReport::Step {
            title: titel.clone(),
            text: text.clone(),
            input_tokens: *token_ein,
            output_tokens: *token_aus,
            millis: *ms,
        }),
        (Ereignis::Dateien { dateien }, Modus::Umsetzen) => Some(AgentReport::Files {
            files: dateien.clone(),
        }),
        (Ereignis::Frage { text, vorschlaege }, Modus::Umsetzen) => Some(AgentReport::Question {
            text: text.clone(),
            suggestions: vorschlaege.clone(),
        }),
        (Ereignis::HinweiseAngekommen, Modus::Umsetzen) => {
            Some(AgentReport::HintsDelivered { count: 1 })
        }
        _ => None,
    }
}

// ---------- Befehlszeile ----------

/// Alles, was `docker run` / `podman run` für einen Lauf braucht.
pub struct LaufBefehl<'a> {
    pub name: &'a str,
    /// Wert des Labels `cockpit.lauf` (der Schlüssel des Laufs).
    pub lauf: &'a str,
    pub klon: &'a Path,
    /// Nur lesen (Plan-Modus): Der Container ändert den Klon nicht.
    pub klon_nur_lesen: bool,
    pub auftrag: &'a Path,
    /// Mit offenem stdin (`-i`) für Antworten und Hinweise.
    pub stdin_offen: bool,
}

/// Die Argumente nach `docker`/`podman`. Enthält nie ein Geheimnis: Der Läufer bekommt Adresse,
/// Key und Modell als `-e NAME` und liest die Werte aus der Umgebung des Kindprozesses
/// ([`umgebung_fuer_kind`]).
pub fn run_args(b: &LaufBefehl) -> Vec<String> {
    let s = |x: &str| x.to_string();
    let mut a = vec![s("run"), s("--rm")];
    if b.stdin_offen {
        a.push(s("-i"));
    }
    a.extend([
        s("--name"),
        s(b.name),
        s("--label"),
        format!("{LABEL}={}", b.lauf),
        s("--add-host"),
        format!("{HOST_NAME}:host-gateway"),
        s("--cap-drop"),
        s("ALL"),
        s("--security-opt"),
        s("no-new-privileges"),
        s("--pids-limit"),
        s("1024"),
        s("--memory"),
        s("4g"),
        s("-v"),
        format!(
            "{}:/work{}",
            b.klon.display(),
            if b.klon_nur_lesen { ":ro" } else { "" }
        ),
        s("-v"),
        format!("{}:/auftrag:ro", b.auftrag.display()),
    ]);
    for name in KIND_UMGEBUNG {
        a.extend([s("-e"), s(name)]);
    }
    a.push(s(IMAGE));
    a
}

/// Die Namen, die `docker run` per `-e NAME` weitergibt (nur der Name, nie ein Wert).
pub const KIND_UMGEBUNG: [&str; 4] = [
    "ANTHROPIC_BASE_URL",
    "ANTHROPIC_AUTH_TOKEN",
    "ANTHROPIC_MODEL",
    "ANTHROPIC_API_KEY",
];

/// Die Umgebung des `docker run`-Prozesses: Dort stehen die Werte, die `-e NAME` weitergibt.
/// `Some` setzt den Wert, `None` entfernt ihn aus der geerbten Umgebung, damit nie ein Key aus der
/// Shell der Person in den Container wandert.
///
/// * Mit Adresse (Proxy wie LiteLLM, anderer kompatibler Endpunkt): `ANTHROPIC_BASE_URL` und
///   `ANTHROPIC_AUTH_TOKEN` (Bearer).
/// * Ohne Adresse (direkt bei Anthropic): `ANTHROPIC_API_KEY` (`x-api-key`), keine Basisadresse.
pub fn umgebung_fuer_kind(
    endpunkt_url: Option<&str>,
    key: &str,
    modell: &str,
) -> Vec<(&'static str, Option<String>)> {
    let (basis, token, api_key) = match endpunkt_url.map(str::trim).filter(|u| !u.is_empty()) {
        Some(url) => (Some(fuer_container(url)), Some(key.to_string()), None),
        None => (None, None, Some(key.to_string())),
    };
    vec![
        ("ANTHROPIC_BASE_URL", basis),
        ("ANTHROPIC_AUTH_TOKEN", token),
        ("ANTHROPIC_MODEL", Some(modell.to_string())),
        ("ANTHROPIC_API_KEY", api_key),
    ]
}

/// Ein Proxy auf dem eigenen Rechner (`localhost`, `127.0.0.1`) heißt im Container `cockpit-host`.
pub fn fuer_container(url: &str) -> String {
    let url = url.trim().trim_end_matches('/');
    let Some((schema, rest)) = url.split_once("://") else {
        return url.to_string();
    };
    let (host_port, pfad) = match rest.find('/') {
        Some(i) => rest.split_at(i),
        None => (rest, ""),
    };
    let (host, port) = match host_port.rsplit_once(':') {
        Some((h, p)) if p.chars().all(|c| c.is_ascii_digit()) && !h.ends_with(']') => (h, Some(p)),
        Some((h, p)) if h.ends_with(']') => (h, Some(p)),
        _ => (host_port, None),
    };
    if matches!(host, "localhost" | "127.0.0.1" | "[::1]" | "0.0.0.0") {
        let port = port.map(|p| format!(":{p}")).unwrap_or_default();
        return format!("{schema}://{HOST_NAME}{port}{pfad}");
    }
    url.to_string()
}

/// Ein Name für den Container: nur erlaubte Zeichen, eindeutig durch die Zeit.
pub fn container_name(lauf: &str, jetzt_ms: u64) -> String {
    let sauber: String = lauf
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '-'
            }
        })
        .take(40)
        .collect();
    format!("cockpit-lauf-{sauber}-{jetzt_ms}")
}

/// Ein Ordnername für den Lauf (der Schlüssel enthält Zeichen wie `#`).
pub fn lauf_ordner(lauf: &str) -> String {
    lauf.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect()
}

// ---------- Auftrag ----------

/// Der Auftrag als JSON für den Läufer (`/auftrag/auftrag.json`). Enthält keine Geheimnisse.
pub fn auftrag_json(ws: &Workspace, run: &AgentRun, modus: Modus, branch: &str) -> Value {
    let item = ws.item_by_key(&run.ticket_key);
    let kommentare: Vec<String> = item
        .map(|i| {
            i.comments
                .iter()
                .rev()
                .take(10)
                .rev()
                .map(|c| kuerzen(&format!("{}: {}", c.author.name, c.text), 800))
                .collect()
        })
        .unwrap_or_default();
    let mut hinweise: Vec<String> = Vec::new();
    if !run.options.hint.trim().is_empty() {
        hinweise.push(run.options.hint.trim().to_string());
    }
    // Hinweise, die der Nutzer während des Laufs gab, und seine Antworten, falls der Lauf neu startet.
    let bisher: Vec<String> = run
        .messages
        .iter()
        .rev()
        .take(15)
        .rev()
        .map(|m| kuerzen(&m.text, 300))
        .collect();
    let rules = &run.rules;
    let anweisungen: Vec<String> = rules
        .instructions
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect();
    json!({
        "modus": modus.kennung(),
        "ticket": {
            "key": run.ticket_key,
            "titel": run.ticket_title,
            "beschreibung": kuerzen(item.map_or("", |i| i.description.as_str()), 6_000),
            "kommentare": kommentare,
        },
        "hinweise": hinweise,
        "plan_anmerkungen": run.plan_notes,
        "plan": run.agent_plan.as_ref().filter(|_| modus == Modus::Umsetzen).map(|p| json!({
            "summary": p.summary, "files": p.files, "steps": p.steps,
        })),
        "bisher": if run.completed > 0 { bisher } else { Vec::new() },
        "regeln": {
            "erlaubte_pfade": rules.allowed_paths,
            "verbotene_pfade": rules.forbidden_paths,
            "pflicht_tests": rules.required_tests,
            "anweisungen": anweisungen,
        },
        "tests_ausfuehren": run.options.run_tests,
        "branch": branch,
        "basis_branch": run.options.base_branch,
    })
}

/// Der Auftrag für die Prüfung der Erreichbarkeit.
pub fn pruef_auftrag() -> Value {
    json!({ "modus": "pruefung", "ticket": { "key": "-", "titel": "-" } })
}

// ---------- Voraussetzungen ----------

/// Was für einen echten Lauf da sein muss.
pub struct Voraussetzungen<'a> {
    pub container: &'a ContainerStatus,
    pub image: bool,
    /// Das Werkzeug des Agenten; nur umgesetzte laufen.
    pub anbieter: Anbieter,
    pub modell: bool,
    pub key: bool,
    /// Das Repo, in dem der Agent arbeitet; `Err` mit der Erklärung, wenn es nicht eindeutig ist.
    pub repo: Result<&'a str, String>,
    pub github_zugang: bool,
}

/// Je fehlender Voraussetzung ein Satz in Klartext, mit dem Ort im Assistenten („Einrichten“).
pub fn fehlende(v: &Voraussetzungen) -> Vec<String> {
    let mut f = Vec::new();
    match (&v.container.laufzeit, v.container.engine_laeuft) {
        (None, _) => f.push(
            "Es gibt keine Container-Laufzeit. Öffne „Einrichten“ › „Container“ und installiere Rancher Desktop (dockerd) oder Podman."
                .to_string(),
        ),
        (Some(l), false) => f.push(format!(
            "{l} läuft nicht. Starte es (Rancher Desktop bzw. „podman machine start“) und prüfe es unter „Einrichten“ › „Container“."
        )),
        (Some(_), true) => {
            if !v.image {
                f.push(
                    "Das Agent-Image fehlt. Öffne „Einrichten“ › „Container“ und klicke auf „Image bauen“."
                        .to_string(),
                );
            }
        }
    }
    if let Some(m) = v.anbieter.nicht_umgesetzt() {
        f.push(m);
    } else if !v.modell || !v.key {
        let mut was = Vec::new();
        if !v.modell {
            was.push("der Modellname");
        }
        if !v.key {
            was.push("der Key");
        }
        f.push(format!(
            "Beim Modellzugang fehlt etwas ({}). Trage es unter „Einrichten“ › „Modell“ ein.",
            was.join(", ")
        ));
    }
    if let Err(m) = &v.repo {
        f.push(m.clone());
    }
    if !v.github_zugang {
        f.push(
            "GitHub ist nicht verbunden (der Agent braucht es zum Klonen und Pushen). Melde dich unter „Einrichten“ › „GitHub“ an."
                .to_string(),
        );
    }
    f
}

/// Das Repo des Agenten: `[agent] repo`, sonst das einzige Repo aus `[github] repos`.
pub fn repo_waehlen<'a>(agent: Option<&'a str>, github: &'a [String]) -> Result<&'a str, String> {
    if let Some(r) = agent {
        return Ok(r);
    }
    match github {
        [einzig] => Ok(einzig.as_str()),
        [] => Err(
            "Es ist kein Repo eingetragen, in dem der Agent arbeiten soll. Trage es unter „Einrichten“ › „GitHub“ ein."
                .to_string(),
        ),
        _ => Err(
            "Es sind mehrere Repos eingetragen. Lege in config.toml unter [agent] mit „repo = \"owner/name\"“ fest, in welchem der Agent arbeitet."
                .to_string(),
        ),
    }
}

/// Der Status für `/api/health`.
pub fn status(echt: bool, v: &Voraussetzungen) -> AgentStatus {
    if !echt {
        return AgentStatus {
            modus: "simuliert".into(),
            bereit: true,
            image: v.image,
            image_name: IMAGE.into(),
            repo: None,
            fehlt: Vec::new(),
        };
    }
    let fehlt = fehlende(v);
    AgentStatus {
        modus: "echt".into(),
        bereit: fehlt.is_empty(),
        image: v.image,
        image_name: IMAGE.into(),
        repo: v.repo.as_ref().ok().map(|r| (*r).to_string()),
        fehlt,
    }
}

// ---------- Aufpasser: Was ist als Nächstes zu tun? ----------

/// Was der Aufpasser über einen laufenden Container weiß.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Info {
    pub modus: Modus,
    pub pausiert: bool,
    /// Der Läufer wartet auf die Antwort des Nutzers.
    pub frage_offen: bool,
    /// So viele der wartenden Hinweise sind schon an den Container gegangen.
    pub hinweise_gesendet: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Anweisung {
    Starten(String, Modus),
    /// Den Container beenden, ohne den Lauf zu ändern (er wurde abgebrochen, angehalten …).
    Stoppen(String),
    Pausieren(String),
    Fortsetzen(String),
    Antwort(String, String),
    Hinweise(String, Vec<String>),
}

/// Vergleicht die Läufe des Workspace mit den laufenden Containern. Rein: Wer sie ausführt, ist
/// der Betrieb. `gesperrt` sind Läufe, die gerade erst gescheitert sind (kein sofortiger Neustart).
pub fn plane(
    ws: &Workspace,
    laufend: &HashMap<String, Info>,
    gesperrt: &[String],
) -> Vec<Anweisung> {
    let mut a = Vec::new();
    for run in ws.runs.iter().filter(|r| r.kind == RunKind::Implement) {
        let key = run.key();
        match (laufend.get(&key), run.state) {
            (None, RunState::Running) if !gesperrt.contains(&key) => {
                a.push(Anweisung::Starten(key, Modus::Umsetzen));
            }
            (None, RunState::AwaitingApproval)
                if run.agent_plan.is_none() && !gesperrt.contains(&key) =>
            {
                a.push(Anweisung::Starten(key, Modus::Plan));
            }
            (None, _) => {}
            (Some(i), state) => {
                match state {
                    // Der Plan-Container ist überholt (Plan liegt vor, freigegeben, abgebrochen …).
                    RunState::AwaitingApproval if i.modus == Modus::Plan => {
                        if run.agent_plan.is_some() {
                            a.push(Anweisung::Stoppen(key));
                        }
                        continue;
                    }
                    RunState::Running | RunState::AwaitingAnswer | RunState::Paused
                        if i.modus == Modus::Plan =>
                    {
                        a.push(Anweisung::Stoppen(key));
                        continue;
                    }
                    RunState::Running | RunState::AwaitingAnswer | RunState::Paused => {}
                    // Abgebrochen, angehalten (Grenze), fertig, zurückgesetzt …
                    _ => {
                        a.push(Anweisung::Stoppen(key));
                        continue;
                    }
                }
                match (state, i.pausiert) {
                    (RunState::Paused, false) => a.push(Anweisung::Pausieren(key.clone())),
                    (RunState::Running, true) => a.push(Anweisung::Fortsetzen(key.clone())),
                    _ => {}
                }
                if state == RunState::Running
                    && i.frage_offen
                    && let Some(antwort) = run
                        .messages
                        .iter()
                        .rev()
                        .find(|m| m.kind == cockpit_core::MessageKind::Answer)
                {
                    a.push(Anweisung::Antwort(key.clone(), antwort.text.clone()));
                }
                if i.hinweise_gesendet < run.pending_hints.len() {
                    a.push(Anweisung::Hinweise(
                        key.clone(),
                        run.pending_hints[i.hinweise_gesendet..].to_vec(),
                    ));
                }
            }
        }
    }
    // Container ohne Lauf (gelöscht, zurückgesetzt): beenden.
    for key in laufend.keys() {
        if !ws
            .runs
            .iter()
            .any(|r| r.kind == RunKind::Implement && &r.key() == key)
        {
            a.push(Anweisung::Stoppen(key.clone()));
        }
    }
    a
}

#[cfg(test)]
mod tests {
    use super::*;
    use cockpit_core::{Action, Actor, AgentOptions, demo};

    const T: u64 = 1_700_000_000_000;

    #[test]
    fn zeilen_werden_gelesen_gekuerzt_und_unbekanntes_verworfen() {
        let e = zeile_lesen(r#"{"t":"schritt","titel":"Datei lesen","text":"a","token_ein":10,"token_aus":2,"ms":5}"#)
            .unwrap();
        assert_eq!(
            e,
            Ereignis::Schritt {
                titel: "Datei lesen".into(),
                text: "a".into(),
                token_ein: 10,
                token_aus: 2,
                ms: 5
            }
        );
        assert_eq!(zeile_lesen("das ist kein json"), Err(Verworfen::Unlesbar));
        assert_eq!(zeile_lesen(r#"{"t":"rm -rf"}"#), Err(Verworfen::Unlesbar));
        assert_eq!(zeile_lesen(r#"{"t":"schritt"}"#), Err(Verworfen::Unlesbar));
        assert_eq!(zeile_lesen(""), Err(Verworfen::Unlesbar));
        let gross = format!(r#"{{"t":"fehler","text":"{}"}}"#, "x".repeat(MAX_ZEILE));
        assert_eq!(zeile_lesen(&gross), Err(Verworfen::ZuGross));
        // Übergroße Werte werden begrenzt, nicht übernommen.
        let Ok(Ereignis::Schritt {
            token_ein,
            ms,
            titel,
            ..
        }) = zeile_lesen(&format!(
            r#"{{"t":"schritt","titel":"{}","token_ein":99999999999999,"ms":99999999999}}"#,
            "y".repeat(5_000)
        ))
        else {
            panic!()
        };
        assert_eq!(token_ein, MAX_TOKEN);
        assert_eq!(ms, MAX_MS);
        assert_eq!(titel.chars().count(), 120);
        let Ok(Ereignis::Dateien { dateien }) = zeile_lesen(&format!(
            r#"{{"t":"dateien","dateien":[{}]}}"#,
            vec![r#""a""#; 1_000].join(",")
        )) else {
            panic!()
        };
        assert_eq!(dateien.len(), MAX_LISTE);
    }

    #[test]
    fn ereignisse_werden_nur_im_passenden_modus_zu_meldungen() {
        let plan = Ereignis::Plan {
            summary: "s".into(),
            files: vec![],
            steps: vec![],
        };
        assert!(als_meldung(&plan, Modus::Plan).is_some());
        assert!(als_meldung(&plan, Modus::Umsetzen).is_none());
        let frage = Ereignis::Frage {
            text: "?".into(),
            vorschlaege: vec![],
        };
        assert!(als_meldung(&frage, Modus::Plan).is_none());
        assert!(als_meldung(&frage, Modus::Umsetzen).is_some());
        let fehler = Ereignis::Fehler { text: "x".into() };
        assert!(als_meldung(&fehler, Modus::Umsetzen).is_none());
    }

    #[test]
    fn befehlszeile_ohne_geheimnisse_und_mit_den_gemeinsamen_schaltern() {
        let b = LaufBefehl {
            name: "cockpit-lauf-BAU-142-1",
            lauf: "BAU-142",
            klon: Path::new("/daten/klon"),
            klon_nur_lesen: false,
            auftrag: Path::new("/daten/auftrag"),
            stdin_offen: true,
        };
        let a = run_args(&b);
        let text = a.join(" ");
        for muss in [
            "run --rm -i",
            "--label cockpit.lauf=BAU-142",
            "--add-host cockpit-host:host-gateway",
            "-v /daten/klon:/work -v /daten/auftrag:/auftrag:ro",
            "-e ANTHROPIC_BASE_URL -e ANTHROPIC_AUTH_TOKEN -e ANTHROPIC_MODEL -e ANTHROPIC_API_KEY",
            "--cap-drop ALL",
        ] {
            assert!(text.contains(muss), "{muss} fehlt in {text}");
        }
        assert_eq!(a.last().unwrap(), IMAGE);
        // Voll benanntes Image, kein SELinux-Zusatz, kein --user (steht im Containerfile).
        assert!(IMAGE.starts_with("localhost/"));
        assert!(
            !text.contains(":Z")
                && !text.contains("--user")
                && !text.contains("host.docker.internal")
        );
        // Nach `-e` steht nur der Name, nie ein `=` mit Wert.
        for (i, x) in a.iter().enumerate() {
            if x == "-e" {
                assert!(!a[i + 1].contains('='), "{}", a[i + 1]);
            }
        }
        // Die Werte gehören in die Umgebung des Kindprozesses.
        let env = umgebung_fuer_kind(
            Some("http://localhost:4000/"),
            "sk-geheim",
            "claude-sonnet-5-5",
        );
        assert!(env.contains(&(
            "ANTHROPIC_BASE_URL",
            Some("http://cockpit-host:4000".to_string())
        )));
        assert!(env.contains(&("ANTHROPIC_AUTH_TOKEN", Some("sk-geheim".to_string()))));
        // Mit Adresse nie ein `ANTHROPIC_API_KEY` aus der Umgebung der Person.
        assert!(env.contains(&("ANTHROPIC_API_KEY", None)));
        assert!(!text.contains("sk-geheim"));
        let ro = run_args(&LaufBefehl {
            klon_nur_lesen: true,
            stdin_offen: false,
            ..b
        });
        assert!(ro.join(" ").contains("/daten/klon:/work:ro"));
        assert!(!ro.contains(&"-i".to_string()));
    }

    #[test]
    fn direkt_beim_anbieter_gibt_nur_den_api_key_weiter() {
        for leer in [None, Some(""), Some("  ")] {
            let env = umgebung_fuer_kind(leer, "sk-ant-geheim", "claude-opus-5-5");
            assert!(env.contains(&("ANTHROPIC_API_KEY", Some("sk-ant-geheim".to_string()))));
            assert!(env.contains(&("ANTHROPIC_BASE_URL", None)));
            assert!(env.contains(&("ANTHROPIC_AUTH_TOKEN", None)));
            assert!(env.contains(&("ANTHROPIC_MODEL", Some("claude-opus-5-5".to_string()))));
        }
        // Jeder Name, den wir setzen oder entfernen, steht auch in der `-e`-Liste.
        let namen: Vec<&str> = umgebung_fuer_kind(None, "k", "m")
            .iter()
            .map(|(n, _)| *n)
            .collect();
        for n in namen {
            assert!(KIND_UMGEBUNG.contains(&n));
        }
    }

    #[test]
    fn proxy_auf_dem_rechner_heisst_im_container_cockpit_host() {
        for (ein, aus) in [
            ("http://localhost:4000", "http://cockpit-host:4000"),
            ("http://127.0.0.1:4000/", "http://cockpit-host:4000"),
            ("https://localhost/v1", "https://cockpit-host/v1"),
            ("http://[::1]:4000", "http://cockpit-host:4000"),
            ("https://litellm.example", "https://litellm.example"),
            (
                "https://litellm.example:8443/x/",
                "https://litellm.example:8443/x",
            ),
            ("https://localhost.example", "https://localhost.example"),
        ] {
            assert_eq!(fuer_container(ein), aus, "{ein}");
        }
    }

    #[test]
    fn namen_sind_sauber() {
        assert_eq!(container_name("BAU-142", 7), "cockpit-lauf-BAU-142-7");
        assert_eq!(container_name("a b/#1", 7), "cockpit-lauf-a-b--1-7");
        assert_eq!(lauf_ordner("a b/#1"), "a_b__1");
    }

    fn start(ws: &mut Workspace, plan_first: bool) {
        ws.apply(
            Action::StartAgent {
                ticket: "BAU-142".into(),
                options: AgentOptions {
                    plan_first,
                    ..AgentOptions::default()
                },
            },
            Actor::Du,
            T,
        )
        .unwrap();
    }

    fn info(modus: Modus) -> Info {
        Info {
            modus,
            pausiert: false,
            frage_offen: false,
            hinweise_gesendet: 0,
        }
    }

    fn laufend(m: Modus) -> HashMap<String, Info> {
        HashMap::from([("BAU-142".to_string(), info(m))])
    }

    #[test]
    fn der_auftrag_traegt_ticket_regeln_und_branch_aber_keine_geheimnisse() {
        let mut ws = demo::workspace();
        start(&mut ws, false);
        let run = ws.run("BAU-142").unwrap();
        let j = auftrag_json(&ws, &run, Modus::Umsetzen, "claude/BAU-142-x");
        assert_eq!(j["modus"], "umsetzen");
        assert_eq!(j["ticket"]["key"], "BAU-142");
        assert_eq!(j["branch"], "claude/BAU-142-x");
        assert_eq!(j["basis_branch"], "main");
        let text = j.to_string();
        assert!(!text.contains("TOKEN") && !text.contains("sk-"));
        assert_eq!(pruef_auftrag()["modus"], "pruefung");
    }

    #[test]
    fn aufpasser_startet_plan_und_lauf_je_nach_zustand() {
        let mut ws = demo::workspace();
        assert!(plane(&ws, &HashMap::new(), &[]).is_empty());
        start(&mut ws, true);
        assert_eq!(
            plane(&ws, &HashMap::new(), &[]),
            [Anweisung::Starten("BAU-142".into(), Modus::Plan)]
        );
        // Mit laufendem Plan-Container passiert nichts, bis der Plan da ist.
        assert!(plane(&ws, &laufend(Modus::Plan), &[]).is_empty());
        ws.report_agent(
            "BAU-142",
            AgentReport::Plan {
                summary: "s".into(),
                files: vec![],
                steps: vec![],
            },
            T,
        )
        .unwrap();
        assert_eq!(
            plane(&ws, &laufend(Modus::Plan), &[]),
            [Anweisung::Stoppen("BAU-142".into())],
            "ein fertiger Plan-Container wird nicht weiter gebraucht"
        );
        assert!(
            plane(&ws, &HashMap::new(), &[]).is_empty(),
            "wartet auf die Freigabe"
        );
        ws.apply(
            Action::ApprovePlan {
                ticket: "BAU-142".into(),
            },
            Actor::Du,
            T,
        )
        .unwrap();
        assert_eq!(
            plane(&ws, &HashMap::new(), &[]),
            [Anweisung::Starten("BAU-142".into(), Modus::Umsetzen)]
        );
        // Ein gerade gescheiterter Lauf startet nicht sofort neu.
        assert!(plane(&ws, &HashMap::new(), &["BAU-142".to_string()]).is_empty());
    }

    #[test]
    fn aufpasser_stoppt_bei_abbruch_grenze_und_verwaisten_containern() {
        let mut ws = demo::workspace();
        start(&mut ws, false);
        assert!(plane(&ws, &laufend(Modus::Umsetzen), &[]).is_empty());
        let mut stopp = ws.clone();
        stopp
            .apply(
                Action::CancelAgent {
                    ticket: "BAU-142".into(),
                },
                Actor::Du,
                T,
            )
            .unwrap();
        assert_eq!(
            plane(&stopp, &laufend(Modus::Umsetzen), &[]),
            [Anweisung::Stoppen("BAU-142".into())]
        );
        // Grenze erreicht: angehalten.
        let mut grenze = ws.clone();
        let mut l = grenze.limits.clone();
        l.run_budget_cents = Some(1);
        grenze.set_limits(l);
        grenze
            .report_agent(
                "BAU-142",
                AgentReport::Step {
                    title: "t".into(),
                    text: String::new(),
                    input_tokens: 1_000_000,
                    output_tokens: 0,
                    millis: 1,
                },
                T,
            )
            .unwrap();
        assert_eq!(
            plane(&grenze, &laufend(Modus::Umsetzen), &[]),
            [Anweisung::Stoppen("BAU-142".into())]
        );
        // Container für einen Lauf, den es nicht gibt.
        let leer = demo::workspace();
        assert_eq!(
            plane(&leer, &laufend(Modus::Umsetzen), &[]),
            [Anweisung::Stoppen("BAU-142".into())]
        );
    }

    #[test]
    fn aufpasser_reicht_pause_antwort_und_hinweise_weiter() {
        let mut ws = demo::workspace();
        start(&mut ws, false);
        ws.apply(
            Action::PauseAgent {
                run: "BAU-142".into(),
            },
            Actor::Du,
            T,
        )
        .unwrap();
        assert_eq!(
            plane(&ws, &laufend(Modus::Umsetzen), &[]),
            [Anweisung::Pausieren("BAU-142".into())]
        );
        ws.apply(
            Action::ResumeAgent {
                run: "BAU-142".into(),
            },
            Actor::Du,
            T,
        )
        .unwrap();
        let mut i = info(Modus::Umsetzen);
        i.pausiert = true;
        let mut m = HashMap::from([("BAU-142".to_string(), i)]);
        assert_eq!(
            plane(&ws, &m, &[]),
            [Anweisung::Fortsetzen("BAU-142".into())]
        );

        // Frage und Antwort.
        ws.report_agent(
            "BAU-142",
            AgentReport::Question {
                text: "Welcher?".into(),
                suggestions: vec![],
            },
            T,
        )
        .unwrap();
        m.get_mut("BAU-142").unwrap().pausiert = false;
        m.get_mut("BAU-142").unwrap().frage_offen = true;
        assert!(plane(&ws, &m, &[]).is_empty(), "solange nicht beantwortet");
        ws.apply(
            Action::AnswerAgent {
                run: "BAU-142".into(),
                text: "der blaue".into(),
            },
            Actor::Du,
            T,
        )
        .unwrap();
        assert_eq!(
            plane(&ws, &m, &[]),
            [Anweisung::Antwort("BAU-142".into(), "der blaue".into())]
        );
        m.get_mut("BAU-142").unwrap().frage_offen = false;

        // Hinweise gehen genau einmal raus.
        ws.apply(
            Action::SteerAgent {
                run: "BAU-142".into(),
                text: "Kleine Commits".into(),
            },
            Actor::Du,
            T,
        )
        .unwrap();
        assert_eq!(
            plane(&ws, &m, &[]),
            [Anweisung::Hinweise(
                "BAU-142".into(),
                vec!["Kleine Commits".into()]
            )]
        );
        m.get_mut("BAU-142").unwrap().hinweise_gesendet = 1;
        assert!(plane(&ws, &m, &[]).is_empty());
    }

    #[test]
    fn ein_noch_nicht_umgesetztes_werkzeug_wird_in_klartext_abgelehnt() {
        let laeuft = ContainerStatus {
            laufzeit: Some("docker".into()),
            engine_laeuft: true,
            ..ContainerStatus::default()
        };
        for a in [Anbieter::ClaudeCode, Anbieter::Codex] {
            let v = Voraussetzungen {
                container: &laeuft,
                image: true,
                anbieter: a,
                modell: true,
                key: true,
                repo: Ok("firma/app"),
                github_zugang: true,
            };
            let f = fehlende(&v);
            assert_eq!(f.len(), 1, "{f:?}");
            assert!(f[0].contains(a.bezeichnung()) && f[0].contains("noch nicht umgesetzt"));
            assert!(!status(true, &v).bereit);
        }
    }

    #[test]
    fn voraussetzungen_sagen_in_klartext_was_fehlt_und_wohin() {
        let kein = ContainerStatus::default();
        let v = Voraussetzungen {
            container: &kein,
            image: false,
            anbieter: Anbieter::ClaudeAgentSdk,
            modell: false,
            key: false,
            repo: Err("Es ist kein Repo eingetragen.".into()),
            github_zugang: false,
        };
        let f = fehlende(&v);
        assert_eq!(f.len(), 4);
        assert!(f[0].contains("Einrichten") && f[0].contains("Container"));
        assert!(f[1].contains("der Modellname, der Key") && !f[1].contains("Adresse"));
        assert!(f[3].contains("GitHub"));
        let laeuft = ContainerStatus {
            laufzeit: Some("docker".into()),
            engine_laeuft: true,
            ..ContainerStatus::default()
        };
        let fast = Voraussetzungen {
            container: &laeuft,
            image: false,
            anbieter: Anbieter::ClaudeAgentSdk,
            modell: true,
            key: true,
            repo: Ok("firma/app"),
            github_zugang: true,
        };
        let f = fehlende(&fast);
        assert_eq!(f.len(), 1);
        assert!(f[0].contains("Image bauen"));
        let s = status(
            true,
            &Voraussetzungen {
                image: true,
                ..fast
            },
        );
        assert!(s.bereit && s.modus == "echt" && s.repo.as_deref() == Some("firma/app"));
        let nicht_laeuft = ContainerStatus {
            laufzeit: Some("podman".into()),
            engine_laeuft: false,
            ..ContainerStatus::default()
        };
        let f = fehlende(&Voraussetzungen {
            container: &nicht_laeuft,
            image: true,
            anbieter: Anbieter::ClaudeAgentSdk,
            modell: true,
            key: true,
            repo: Ok("a/b"),
            github_zugang: true,
        });
        assert!(f[0].contains("podman machine start"));
        // Simuliert: immer bereit.
        assert!(status(false, &v).bereit);
    }

    #[test]
    fn das_repo_des_agenten() {
        let eins = vec!["a/b".to_string()];
        let zwei = vec!["a/b".to_string(), "c/d".to_string()];
        assert_eq!(repo_waehlen(Some("x/y"), &zwei), Ok("x/y"));
        assert_eq!(repo_waehlen(None, &eins), Ok("a/b"));
        assert!(repo_waehlen(None, &zwei).unwrap_err().contains("[agent]"));
        assert!(repo_waehlen(None, &[]).unwrap_err().contains("kein Repo"));
    }
}
