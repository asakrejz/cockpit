//! Anmeldung per OAuth: GitHub (GitHub App, Nutzer-Token mit Refresh) und Atlassian (OAuth 2.0
//! 3LO, Resource-level).
//!
//! Das Backend leitet den Browser zur Anmeldung (`state` + PKCE), tauscht im Callback den `code`
//! gegen Tokens und frischt sie vor Ablauf auf. Das Client-Secret und die Tokens verlassen das
//! Backend nie. Alles hier ist rein oder nimmt die Adressen als [`Endpunkte`] entgegen, damit
//! sich jeder Fall gegen einen nachgebauten Dienst testen lässt.
//!
//! Gegen die echten Dienste ist das noch nicht geprüft.

use std::collections::HashMap;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::http::{client, sende};
use crate::{Fehler, Secret};

/// Die Dienste mit OAuth.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Dienst {
    GitHub,
    Atlassian,
}

impl Dienst {
    /// So heißt der Dienst in Adressen: `/api/oauth/<kennung>/…`.
    pub fn kennung(self) -> &'static str {
        match self {
            Dienst::GitHub => "github",
            Dienst::Atlassian => "atlassian",
        }
    }

    pub fn von_kennung(k: &str) -> Option<Self> {
        match k {
            "github" => Some(Dienst::GitHub),
            "atlassian" => Some(Dienst::Atlassian),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Dienst::GitHub => "GitHub",
            Dienst::Atlassian => "Atlassian",
        }
    }
}

/// Berechtigungen bei Atlassian; `offline_access` liefert das Refresh-Token.
pub const ATLASSIAN_SCOPES: &str = "read:jira-work write:jira-work read:jira-user offline_access";

/// Die Adressen der Dienste. Standard sind die echten; Tests setzen den nachgebauten Dienst ein.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpunkte {
    pub github_anmeldung: String,
    pub github_token: String,
    pub atlassian_anmeldung: String,
    pub atlassian_token: String,
    /// Basis von `accessible-resources` und `ex/jira/<cloudId>`.
    pub atlassian_api: String,
}

impl Default for Endpunkte {
    fn default() -> Self {
        Self {
            github_anmeldung: "https://github.com/login/oauth/authorize".into(),
            github_token: "https://github.com/login/oauth/access_token".into(),
            atlassian_anmeldung: "https://auth.atlassian.com/authorize".into(),
            atlassian_token: "https://auth.atlassian.com/oauth/token".into(),
            atlassian_api: "https://api.atlassian.com".into(),
        }
    }
}

/// Die Zugangsdaten der App eines Dienstes. Das Secret steht nur hier und wird nie ausgegeben.
pub struct App<'a> {
    pub client_id: &'a str,
    pub client_secret: &'a Secret,
    pub redirect_uri: &'a str,
}

// ───────────────────────── Tokens ─────────────────────────

/// Access- und Refresh-Token eines Dienstes. Liegt als ein Eintrag im Schlüsselbund.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tokens {
    pub access: Secret,
    pub refresh: Option<Secret>,
    /// Zeitpunkt (Sekunden seit 1970), ab dem `access` nicht mehr gilt. `None`: läuft nicht ab.
    pub laeuft_ab_s: Option<u64>,
}

/// So früh vor dem Ablauf wird aufgefrischt.
const VORLAUF_S: u64 = 120;

impl Tokens {
    /// Muss vor dem Gebrauch aufgefrischt werden?
    pub fn braucht_auffrischen(&self, jetzt_s: u64) -> bool {
        self.laeuft_ab_s.is_some_and(|ab| jetzt_s + VORLAUF_S >= ab)
    }

    /// Text für den Schlüsselbund.
    pub fn als_text(&self) -> String {
        serde_json::json!({
            "access": self.access.expose(),
            "refresh": self.refresh.as_ref().map(Secret::expose),
            "laeuft_ab_s": self.laeuft_ab_s,
        })
        .to_string()
    }

    /// Gegenstück zu [`als_text`](Self::als_text); `None` bei Unlesbarem oder leerem Token.
    pub fn aus_text(text: &str) -> Option<Self> {
        #[derive(Deserialize)]
        struct Roh {
            access: String,
            #[serde(default)]
            refresh: Option<String>,
            #[serde(default)]
            laeuft_ab_s: Option<u64>,
        }
        let r: Roh = serde_json::from_str(text).ok()?;
        (!r.access.is_empty()).then(|| Self {
            access: Secret::new(r.access),
            refresh: r.refresh.filter(|t| !t.is_empty()).map(Secret::new),
            laeuft_ab_s: r.laeuft_ab_s,
        })
    }
}

/// Antwort des Token-Endpunkts. GitHub antwortet auch bei Fehlern mit 200 und `error`.
#[derive(Deserialize)]
struct TokenAntwort {
    access_token: Option<String>,
    refresh_token: Option<String>,
    expires_in: Option<u64>,
    error: Option<String>,
    error_description: Option<String>,
}

/// Liest die Antwort des Token-Endpunkts. `alt`: das bisherige Refresh-Token, falls der Dienst
/// kein neues schickt.
fn tokens_aus(
    body: &str,
    jetzt_s: u64,
    alt: Option<&Secret>,
    geheim: &[&str],
) -> Result<Tokens, Fehler> {
    // Kurze Werte würden beliebige Wörter treffen; echte Codes und Secrets sind länger.
    let rein = |m: String| {
        geheim
            .iter()
            .filter(|g| g.len() >= 8)
            .fold(m, |m, g| m.replace(g, "***"))
    };
    let a: TokenAntwort =
        serde_json::from_str(body).map_err(|e| Fehler::Antwort(rein(e.to_string())))?;
    if let Some(fehler) = a.error {
        let text = a.error_description.unwrap_or(fehler);
        return Err(Fehler::Abgelehnt(rein(text.chars().take(240).collect())));
    }
    let access = a
        .access_token
        .filter(|t| !t.is_empty())
        .ok_or_else(|| Fehler::Antwort("Kein access_token in der Antwort".into()))?;
    Ok(Tokens {
        access: Secret::new(access),
        refresh: a
            .refresh_token
            .filter(|t| !t.is_empty())
            .map(Secret::new)
            .or_else(|| alt.cloned()),
        laeuft_ab_s: a.expires_in.map(|s| jetzt_s + s),
    })
}

// ───────────────────────── Anmeldung ─────────────────────────

/// `n` Zufallsbytes als URL-sicherer Text.
fn zufall(n: usize) -> Result<String, Fehler> {
    let mut buf = vec![0u8; n];
    getrandom::fill(&mut buf)
        .map_err(|e| Fehler::Einstellung(format!("Kein Zufall verfügbar: {e}")))?;
    Ok(URL_SAFE_NO_PAD.encode(buf))
}

/// Die PKCE-Prüfsumme zum `verifier` (Methode `S256`).
pub fn pkce_challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

/// Die Adresse, zu der der Browser für die Anmeldung geleitet wird.
pub fn anmelde_url(
    e: &Endpunkte,
    dienst: Dienst,
    app: &App<'_>,
    state: &str,
    challenge: &str,
) -> Result<String, Fehler> {
    let mut p: Vec<(&str, &str)> = vec![
        ("client_id", app.client_id),
        ("redirect_uri", app.redirect_uri),
        ("state", state),
        ("code_challenge", challenge),
        ("code_challenge_method", "S256"),
    ];
    let basis = match dienst {
        Dienst::GitHub => &e.github_anmeldung,
        Dienst::Atlassian => {
            p.extend([
                ("audience", "api.atlassian.com"),
                ("scope", ATLASSIAN_SCOPES),
                ("response_type", "code"),
                ("prompt", "consent"),
            ]);
            &e.atlassian_anmeldung
        }
    };
    reqwest::Url::parse_with_params(basis, &p)
        .map(String::from)
        .map_err(|e| Fehler::Einstellung(format!("Anmelde-Adresse ungültig: {e}")))
}

/// Tauscht den `code` aus dem Callback gegen Tokens. Läuft nur im Backend (mit dem Secret).
pub async fn code_tauschen(
    e: &Endpunkte,
    dienst: Dienst,
    app: &App<'_>,
    code: &str,
    verifier: &str,
    jetzt_s: u64,
) -> Result<Tokens, Fehler> {
    let geheim = [app.client_secret.expose(), code, verifier];
    let http = client()?;
    let req = match dienst {
        Dienst::GitHub => http
            .post(&e.github_token)
            .header("Accept", "application/json")
            .form(&[
                ("client_id", app.client_id),
                ("client_secret", app.client_secret.expose()),
                ("code", code),
                ("redirect_uri", app.redirect_uri),
                ("code_verifier", verifier),
            ]),
        Dienst::Atlassian => http
            .post(&e.atlassian_token)
            .header("Accept", "application/json")
            .json(&serde_json::json!({
                "grant_type": "authorization_code",
                "client_id": app.client_id,
                "client_secret": app.client_secret.expose(),
                "code": code,
                "redirect_uri": app.redirect_uri,
                "code_verifier": verifier,
            })),
    };
    let body = sende(req).await.map_err(|f| f.ohne_schluessel(geheim[0]))?;
    tokens_aus(&body, jetzt_s, None, &geheim)
}

/// Holt mit dem Refresh-Token neue Tokens. Fehlt es, ist das ein Fall von „nicht angemeldet“.
pub async fn auffrischen(
    e: &Endpunkte,
    dienst: Dienst,
    app: &App<'_>,
    alt: &Tokens,
    jetzt_s: u64,
) -> Result<Tokens, Fehler> {
    let Some(refresh) = alt.refresh.as_ref() else {
        return Err(Fehler::NichtAngemeldet);
    };
    let geheim = [app.client_secret.expose(), refresh.expose()];
    let http = client()?;
    let req = match dienst {
        Dienst::GitHub => http
            .post(&e.github_token)
            .header("Accept", "application/json")
            .form(&[
                ("client_id", app.client_id),
                ("client_secret", app.client_secret.expose()),
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh.expose()),
            ]),
        Dienst::Atlassian => http
            .post(&e.atlassian_token)
            .header("Accept", "application/json")
            .json(&serde_json::json!({
                "grant_type": "refresh_token",
                "client_id": app.client_id,
                "client_secret": app.client_secret.expose(),
                "refresh_token": refresh.expose(),
            })),
    };
    let body = sende(req).await.map_err(|f| f.ohne_schluessel(geheim[0]))?;
    // Atlassian dreht das Refresh-Token bei jeder Auffrischung: das neue ersetzt das alte.
    tokens_aus(&body, jetzt_s, Some(refresh), &geheim)
}

/// Widerruft das Token beim Dienst. Nur GitHub bietet das als Aufruf an (`DELETE
/// /applications/<client_id>/token`); bei Atlassian bleibt der Entzug in den Kontoeinstellungen
/// (`Ok(false)`: beim Dienst nichts widerrufen).
pub async fn widerrufen(
    dienst: Dienst,
    app: &App<'_>,
    github_api: &str,
    tokens: &Tokens,
) -> Result<bool, Fehler> {
    match dienst {
        Dienst::Atlassian => Ok(false),
        Dienst::GitHub => {
            let basis = crate::http::basis_url(github_api)?;
            let req = client()?
                .delete(format!("{basis}/applications/{}/token", app.client_id))
                .basic_auth(app.client_id, Some(app.client_secret.expose()))
                .header("Accept", "application/vnd.github+json")
                .header("X-GitHub-Api-Version", "2022-11-28")
                .json(&serde_json::json!({ "access_token": tokens.access.expose() }));
            match sende(req).await {
                Ok(_) => Ok(true),
                // Schon widerrufen oder abgelaufen: Ziel erreicht.
                Err(Fehler::NichtGefunden) => Ok(true),
                Err(f) => Err(f
                    .ohne_schluessel(app.client_secret.expose())
                    .ohne_schluessel(tokens.access.expose())),
            }
        }
    }
}

// ───────────────────────── Atlassian: Site ─────────────────────────

/// Eine Jira-Site, auf die die Zustimmung gilt.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Site {
    pub id: String,
    pub name: String,
    pub url: String,
}

/// `GET /oauth/token/accessible-resources`: die Sites, die der Nutzer freigegeben hat.
pub async fn sites(e: &Endpunkte, access: &Secret) -> Result<Vec<Site>, Fehler> {
    let basis = crate::http::basis_url(&e.atlassian_api)?;
    let req = client()?
        .get(format!("{basis}/oauth/token/accessible-resources"))
        .bearer_auth(access.expose())
        .header("Accept", "application/json");
    let body = sende(req)
        .await
        .map_err(|f| f.ohne_schluessel(access.expose()))?;
    serde_json::from_str(&body).map_err(|e| Fehler::Antwort(e.to_string()))
}

/// Wählt die Site: genau eine automatisch, sonst nach `gewuenscht` (Name, Adresse oder Id aus
/// der Konfiguration). Bei mehreren ohne Wunsch nennt der Fehler die Möglichkeiten.
pub fn site_waehlen(sites: &[Site], gewuenscht: Option<&str>) -> Result<Site, Fehler> {
    let namen = || {
        sites
            .iter()
            .map(|s| s.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    };
    match (sites, gewuenscht.map(str::trim).filter(|g| !g.is_empty())) {
        ([], _) => Err(Fehler::NichtMoeglich(
            "Atlassian hat keine Jira-Site freigegeben – bei der Zustimmung eine Site wählen"
                .into(),
        )),
        (liste, Some(wunsch)) => {
            let w = wunsch.trim_end_matches('/').to_ascii_lowercase();
            liste
                .iter()
                .find(|s| {
                    s.id.eq_ignore_ascii_case(&w)
                        || s.name.to_ascii_lowercase() == w
                        || s.url.trim_end_matches('/').to_ascii_lowercase() == w
                })
                .cloned()
                .ok_or_else(|| {
                    Fehler::NichtMoeglich(format!(
                        "Die Site „{wunsch}“ ist nicht freigegeben. Freigegeben: {}",
                        namen()
                    ))
                })
        }
        ([eine], None) => Ok(eine.clone()),
        (_, None) => Err(Fehler::NichtMoeglich(format!(
            "Mehrere Sites freigegeben ({}) – in config.toml unter [atlassian] `site` setzen",
            namen()
        ))),
    }
}

/// Die Basis für Jira über das Atlassian-Gateway: `<api>/ex/jira/<cloudId>`.
pub fn jira_basis(e: &Endpunkte, cloud_id: &str) -> Result<String, Fehler> {
    let ok = !cloud_id.is_empty()
        && cloud_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-');
    if !ok {
        return Err(Fehler::Einstellung("Ungültige Site-Id".into()));
    }
    Ok(format!(
        "{}/ex/jira/{cloud_id}",
        crate::http::basis_url(&e.atlassian_api)?
    ))
}

// ───────────────────────── laufende Anmeldungen ─────────────────────────

/// So lange gilt ein `state`.
pub const STATE_GUELTIG_MS: u64 = 10 * 60 * 1000;
/// Mehr offene Anmeldungen gleichzeitig gibt es nicht (lokal, eine Person).
const MAX_OFFEN: usize = 16;

struct Offen {
    dienst: Dienst,
    verifier: String,
    ab_ms: u64,
}

/// Warum ein Callback nicht zu einer Anmeldung passt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StateFehler {
    /// Kein `state` oder unbekannt – auch ein zweiter Aufruf desselben Callbacks.
    Unbekannt,
    Abgelaufen,
    /// Der `state` gehört zu einer Anmeldung bei einem anderen Dienst.
    FalscherDienst,
}

impl StateFehler {
    pub fn text(&self) -> &'static str {
        match self {
            StateFehler::Unbekannt => {
                "Die Anmeldung ist unbekannt oder wurde schon verwendet – bitte neu starten"
            }
            StateFehler::Abgelaufen => "Die Anmeldung hat zu lange gedauert – bitte neu starten",
            StateFehler::FalscherDienst => "Die Anmeldung gehört zu einem anderen Dienst",
        }
    }
}

/// Offene Anmeldungen: `state` → PKCE-Verifier. Einmalig (wird beim Entnehmen gelöscht) und
/// kurzlebig. Nur im Speicher, nie auf der Platte.
#[derive(Default)]
pub struct Anmeldungen {
    offen: HashMap<String, Offen>,
}

impl Anmeldungen {
    /// Beginnt eine Anmeldung: liefert `state` und die PKCE-Prüfsumme.
    pub fn beginnen(&mut self, dienst: Dienst, jetzt_ms: u64) -> Result<(String, String), Fehler> {
        self.offen
            .retain(|_, o| jetzt_ms.saturating_sub(o.ab_ms) <= STATE_GUELTIG_MS);
        while self.offen.len() >= MAX_OFFEN {
            // Die älteste verwerfen.
            let aelteste = self
                .offen
                .iter()
                .min_by_key(|(_, o)| o.ab_ms)
                .map(|(k, _)| k.clone());
            match aelteste {
                Some(k) => self.offen.remove(&k),
                None => break,
            };
        }
        let state = zufall(32)?;
        let verifier = zufall(48)?;
        let challenge = pkce_challenge(&verifier);
        self.offen.insert(
            state.clone(),
            Offen {
                dienst,
                verifier,
                ab_ms: jetzt_ms,
            },
        );
        Ok((state, challenge))
    }

    /// Entnimmt den Verifier zum `state`. Danach ist er verbraucht – auch bei einem Fehler.
    pub fn entnehmen(
        &mut self,
        state: &str,
        dienst: Dienst,
        jetzt_ms: u64,
    ) -> Result<String, StateFehler> {
        let o = self.offen.remove(state).ok_or(StateFehler::Unbekannt)?;
        if jetzt_ms.saturating_sub(o.ab_ms) > STATE_GUELTIG_MS {
            return Err(StateFehler::Abgelaufen);
        }
        if o.dienst != dienst {
            return Err(StateFehler::FalscherDienst);
        }
        Ok(o.verifier)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_nach_rfc_7636() {
        // Beispiel aus RFC 7636, Anhang B.
        assert_eq!(
            pkce_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    fn app<'a>(secret: &'a Secret) -> App<'a> {
        App {
            client_id: "id123",
            client_secret: secret,
            redirect_uri: "http://127.0.0.1:8787/api/oauth/github/callback",
        }
    }

    #[test]
    fn anmelde_url_github_und_atlassian() {
        let e = Endpunkte::default();
        let s = Secret::new("geheim");
        let u = anmelde_url(&e, Dienst::GitHub, &app(&s), "st", "ch").unwrap();
        assert!(u.starts_with("https://github.com/login/oauth/authorize?"));
        assert!(u.contains("client_id=id123"));
        assert!(u.contains("state=st"));
        assert!(u.contains("code_challenge=ch"));
        assert!(u.contains("code_challenge_method=S256"));
        assert!(u.contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A8787"));
        assert!(!u.contains("geheim"));
        assert!(!u.contains("scope"), "GitHub Apps haben keine Scopes");

        let u = anmelde_url(&e, Dienst::Atlassian, &app(&s), "st", "ch").unwrap();
        assert!(u.starts_with("https://auth.atlassian.com/authorize?"));
        assert!(u.contains("audience=api.atlassian.com"));
        assert!(
            u.contains("scope=read%3Ajira-work+write%3Ajira-work+read%3Ajira-user+offline_access")
        );
        assert!(u.contains("response_type=code"));
        assert!(!u.contains("geheim"));
    }

    #[test]
    fn tokens_rundreise_und_ablauf() {
        let t = Tokens {
            access: Secret::new("a"),
            refresh: Some(Secret::new("r")),
            laeuft_ab_s: Some(1000),
        };
        assert_eq!(Tokens::aus_text(&t.als_text()), Some(t.clone()));
        assert!(!t.braucht_auffrischen(800));
        assert!(t.braucht_auffrischen(900), "kurz vor Ablauf");
        assert!(t.braucht_auffrischen(2000));
        let ewig = Tokens {
            laeuft_ab_s: None,
            ..t
        };
        assert!(!ewig.braucht_auffrischen(u64::MAX / 2));
        assert_eq!(Tokens::aus_text("kaputt"), None);
        assert_eq!(Tokens::aus_text(r#"{"access":""}"#), None);
        assert!(format!("{ewig:?}").contains('a'));
        assert!(!format!("{ewig:?}").contains("\"r\""));
    }

    #[test]
    fn token_antwort_fehler_und_erfolg() {
        let ok = tokens_aus(
            r#"{"access_token":"A","refresh_token":"R","expires_in":3600}"#,
            100,
            None,
            &[],
        )
        .unwrap();
        assert_eq!(ok.access.expose(), "A");
        assert_eq!(ok.refresh.unwrap().expose(), "R");
        assert_eq!(ok.laeuft_ab_s, Some(3700));

        // Ohne neues Refresh-Token bleibt das alte.
        let alt = Secret::new("alt");
        let t = tokens_aus(r#"{"access_token":"A"}"#, 0, Some(&alt), &[]).unwrap();
        assert_eq!(t.refresh.unwrap().expose(), "alt");
        assert_eq!(t.laeuft_ab_s, None);

        // GitHub: 200 mit Fehler.
        let f = tokens_aus(
            r#"{"error":"bad_verification_code","error_description":"Der Code ist abgelaufen GEHEIMNIS123"}"#,
            0,
            None,
            &["GEHEIMNIS123"],
        )
        .unwrap_err();
        assert!(
            matches!(&f, Fehler::Abgelehnt(m) if m.contains("abgelaufen") && !m.contains("GEHEIMNIS123"))
        );
        assert!(matches!(
            tokens_aus("{}", 0, None, &[]),
            Err(Fehler::Antwort(_))
        ));
    }

    fn site(id: &str, name: &str) -> Site {
        Site {
            id: id.into(),
            name: name.into(),
            url: format!("https://{name}.atlassian.net"),
        }
    }

    #[test]
    fn site_wahl() {
        let eine = [site("abc-1", "firma")];
        assert_eq!(site_waehlen(&eine, None).unwrap().id, "abc-1");
        let zwei = [site("abc-1", "firma"), site("def-2", "privat")];
        let f = site_waehlen(&zwei, None).unwrap_err().to_string();
        assert!(
            f.contains("firma") && f.contains("privat") && f.contains("site"),
            "{f}"
        );
        for wunsch in ["privat", "PRIVAT", "def-2", "https://privat.atlassian.net/"] {
            assert_eq!(
                site_waehlen(&zwei, Some(wunsch)).unwrap().id,
                "def-2",
                "{wunsch}"
            );
        }
        assert!(site_waehlen(&zwei, Some("fremd")).is_err());
        assert!(
            site_waehlen(&[], None)
                .unwrap_err()
                .to_string()
                .contains("keine Jira-Site")
        );
        // Leerer Wunsch zählt als keiner.
        assert!(site_waehlen(&eine, Some(" ")).is_ok());
    }

    #[test]
    fn jira_basis_prueft_die_id() {
        let e = Endpunkte::default();
        assert_eq!(
            jira_basis(&e, "1324a887-451a").unwrap(),
            "https://api.atlassian.com/ex/jira/1324a887-451a"
        );
        for schlecht in ["", "a/b", "a?b", "../x"] {
            assert!(jira_basis(&e, schlecht).is_err(), "{schlecht}");
        }
    }

    #[test]
    fn state_ist_einmalig_kurzlebig_und_dienstgebunden() {
        let mut a = Anmeldungen::default();
        let (state, challenge) = a.beginnen(Dienst::GitHub, 1000).unwrap();
        assert!(state.len() >= 40, "genug Zufall");
        let verifier = a.entnehmen(&state, Dienst::GitHub, 2000).unwrap();
        assert_eq!(pkce_challenge(&verifier), challenge);
        // Zweiter Callback mit demselben state.
        assert_eq!(
            a.entnehmen(&state, Dienst::GitHub, 2000),
            Err(StateFehler::Unbekannt)
        );
        // Abgelaufen.
        let (s2, _) = a.beginnen(Dienst::GitHub, 0).unwrap();
        assert_eq!(
            a.entnehmen(&s2, Dienst::GitHub, STATE_GUELTIG_MS + 1),
            Err(StateFehler::Abgelaufen)
        );
        // Anderer Dienst – und der state ist trotzdem verbraucht.
        let (s3, _) = a.beginnen(Dienst::Atlassian, 0).unwrap();
        assert_eq!(
            a.entnehmen(&s3, Dienst::GitHub, 1),
            Err(StateFehler::FalscherDienst)
        );
        assert_eq!(
            a.entnehmen(&s3, Dienst::Atlassian, 1),
            Err(StateFehler::Unbekannt)
        );
        assert_eq!(
            a.entnehmen("", Dienst::GitHub, 1),
            Err(StateFehler::Unbekannt)
        );
    }

    #[test]
    fn offene_anmeldungen_sind_begrenzt() {
        let mut a = Anmeldungen::default();
        for i in 0..40 {
            a.beginnen(Dienst::GitHub, i).unwrap();
        }
        assert!(a.offen.len() <= MAX_OFFEN);
    }

    #[test]
    fn dienst_kennungen() {
        for d in [Dienst::GitHub, Dienst::Atlassian] {
            assert_eq!(Dienst::von_kennung(d.kennung()), Some(d));
        }
        assert_eq!(Dienst::von_kennung("gitlab"), None);
    }
}
