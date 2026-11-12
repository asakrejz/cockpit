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

use crate::http::{client, send};
use crate::{Error, Secret};

/// Die Dienste mit OAuth.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Service {
    GitHub,
    Atlassian,
}

impl Service {
    /// So heißt der Dienst in Adressen: `/api/oauth/<kennung>/…`.
    pub fn id(self) -> &'static str {
        match self {
            Service::GitHub => "github",
            Service::Atlassian => "atlassian",
        }
    }

    pub fn from_id(k: &str) -> Option<Self> {
        match k {
            "github" => Some(Service::GitHub),
            "atlassian" => Some(Service::Atlassian),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Service::GitHub => "GitHub",
            Service::Atlassian => "Atlassian",
        }
    }
}

/// Berechtigungen bei Atlassian; `offline_access` liefert das Refresh-Token.
pub const ATLASSIAN_SCOPES: &str = "read:jira-work write:jira-work read:jira-user offline_access";

/// Die Adressen der Dienste. Standard sind die echten; Tests setzen den nachgebauten Dienst ein.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoints {
    pub github_sign_in: String,
    pub github_token: String,
    pub atlassian_sign_in: String,
    pub atlassian_token: String,
    /// Basis von `accessible-resources` und `ex/jira/<cloudId>`.
    pub atlassian_api: String,
}

impl Default for Endpoints {
    fn default() -> Self {
        Self {
            github_sign_in: "https://github.com/login/oauth/authorize".into(),
            github_token: "https://github.com/login/oauth/access_token".into(),
            atlassian_sign_in: "https://auth.atlassian.com/authorize".into(),
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
    pub expires_in_s: Option<u64>,
}

/// So früh vor dem Ablauf wird aufgefrischt.
const LEAD_S: u64 = 120;

impl Tokens {
    /// Muss vor dem Gebrauch aufgefrischt werden?
    pub fn needs_refresh(&self, now_s: u64) -> bool {
        self.expires_in_s.is_some_and(|ab| now_s + LEAD_S >= ab)
    }

    /// Text für den Schlüsselbund.
    pub fn as_text(&self) -> String {
        serde_json::json!({
            "access": self.access.expose(),
            "refresh": self.refresh.as_ref().map(Secret::expose),
            "laeuft_ab_s": self.expires_in_s,
        })
        .to_string()
    }

    /// Gegenstück zu [`als_text`](Self::als_text); `None` bei Unlesbarem oder leerem Token.
    pub fn from_text(text: &str) -> Option<Self> {
        #[derive(Deserialize)]
        struct Raw {
            access: String,
            #[serde(default)]
            refresh: Option<String>,
            #[serde(default)]
            #[serde(rename = "laeuft_ab_s")]
            expires_in_s: Option<u64>,
        }
        let r: Raw = serde_json::from_str(text).ok()?;
        (!r.access.is_empty()).then(|| Self {
            access: Secret::new(r.access),
            refresh: r.refresh.filter(|t| !t.is_empty()).map(Secret::new),
            expires_in_s: r.expires_in_s,
        })
    }
}

/// Antwort des Token-Endpunkts. GitHub antwortet auch bei Fehlern mit 200 und `error`.
#[derive(Deserialize)]
struct TokenResponse {
    access_token: Option<String>,
    refresh_token: Option<String>,
    expires_in: Option<u64>,
    error: Option<String>,
    error_description: Option<String>,
}

/// Liest die Antwort des Token-Endpunkts. `alt`: das bisherige Refresh-Token, falls der Dienst
/// kein neues schickt.
fn tokens_from(
    body: &str,
    now_s: u64,
    alt: Option<&Secret>,
    secret: &[&str],
) -> Result<Tokens, Error> {
    // Kurze Werte würden beliebige Wörter treffen; echte Codes und Secrets sind länger.
    let pure = |m: String| {
        secret
            .iter()
            .filter(|g| g.len() >= 8)
            .fold(m, |m, g| m.replace(g, "***"))
    };
    let a: TokenResponse =
        serde_json::from_str(body).map_err(|e| Error::Response(pure(e.to_string())))?;
    if let Some(error) = a.error {
        let text = a.error_description.unwrap_or(error);
        return Err(Error::Rejected(pure(text.chars().take(240).collect())));
    }
    let access = a
        .access_token
        .filter(|t| !t.is_empty())
        .ok_or_else(|| Error::Response("Kein access_token in der Antwort".into()))?;
    Ok(Tokens {
        access: Secret::new(access),
        refresh: a
            .refresh_token
            .filter(|t| !t.is_empty())
            .map(Secret::new)
            .or_else(|| alt.cloned()),
        expires_in_s: a.expires_in.map(|s| now_s + s),
    })
}

// ───────────────────────── Anmeldung ─────────────────────────

/// `n` Zufallsbytes als URL-sicherer Text.
fn random(n: usize) -> Result<String, Error> {
    let mut buf = vec![0u8; n];
    getrandom::fill(&mut buf)
        .map_err(|e| Error::Config(format!("Kein Zufall verfügbar: {e}")))?;
    Ok(URL_SAFE_NO_PAD.encode(buf))
}

/// Die PKCE-Prüfsumme zum `verifier` (Methode `S256`).
pub fn pkce_challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

/// Die Adresse, zu der der Browser für die Anmeldung geleitet wird.
pub fn sign_in_url(
    e: &Endpoints,
    service: Service,
    app: &App<'_>,
    state: &str,
    challenge: &str,
) -> Result<String, Error> {
    let mut p: Vec<(&str, &str)> = vec![
        ("client_id", app.client_id),
        ("redirect_uri", app.redirect_uri),
        ("state", state),
        ("code_challenge", challenge),
        ("code_challenge_method", "S256"),
    ];
    let base = match service {
        Service::GitHub => &e.github_sign_in,
        Service::Atlassian => {
            p.extend([
                ("audience", "api.atlassian.com"),
                ("scope", ATLASSIAN_SCOPES),
                ("response_type", "code"),
                ("prompt", "consent"),
            ]);
            &e.atlassian_sign_in
        }
    };
    reqwest::Url::parse_with_params(base, &p)
        .map(String::from)
        .map_err(|e| Error::Config(format!("Anmelde-Adresse ungültig: {e}")))
}

/// Tauscht den `code` aus dem Callback gegen Tokens. Läuft nur im Backend (mit dem Secret).
pub async fn code_exchange(
    e: &Endpoints,
    service: Service,
    app: &App<'_>,
    code: &str,
    verifier: &str,
    now_s: u64,
) -> Result<Tokens, Error> {
    let secret = [app.client_secret.expose(), code, verifier];
    let http = client()?;
    let req = match service {
        Service::GitHub => http
            .post(&e.github_token)
            .header("Accept", "application/json")
            .form(&[
                ("client_id", app.client_id),
                ("client_secret", app.client_secret.expose()),
                ("code", code),
                ("redirect_uri", app.redirect_uri),
                ("code_verifier", verifier),
            ]),
        Service::Atlassian => http
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
    let body = send(req).await.map_err(|f| f.without_key(secret[0]))?;
    tokens_from(&body, now_s, None, &secret)
}

/// Holt mit dem Refresh-Token neue Tokens. Fehlt es, ist das ein Fall von „nicht angemeldet“.
pub async fn refresh(
    e: &Endpoints,
    service: Service,
    app: &App<'_>,
    alt: &Tokens,
    now_s: u64,
) -> Result<Tokens, Error> {
    let Some(refresh) = alt.refresh.as_ref() else {
        return Err(Error::NotSignedIn);
    };
    let secret = [app.client_secret.expose(), refresh.expose()];
    let http = client()?;
    let req = match service {
        Service::GitHub => http
            .post(&e.github_token)
            .header("Accept", "application/json")
            .form(&[
                ("client_id", app.client_id),
                ("client_secret", app.client_secret.expose()),
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh.expose()),
            ]),
        Service::Atlassian => http
            .post(&e.atlassian_token)
            .header("Accept", "application/json")
            .json(&serde_json::json!({
                "grant_type": "refresh_token",
                "client_id": app.client_id,
                "client_secret": app.client_secret.expose(),
                "refresh_token": refresh.expose(),
            })),
    };
    let body = send(req).await.map_err(|f| f.without_key(secret[0]))?;
    // Atlassian dreht das Refresh-Token bei jeder Auffrischung: das neue ersetzt das alte.
    tokens_from(&body, now_s, Some(refresh), &secret)
}

/// Widerruft das Token beim Dienst. Nur GitHub bietet das als Aufruf an (`DELETE
/// /applications/<client_id>/token`); bei Atlassian bleibt der Entzug in den Kontoeinstellungen
/// (`Ok(false)`: beim Dienst nichts widerrufen).
pub async fn revoked(
    service: Service,
    app: &App<'_>,
    github_api: &str,
    tokens: &Tokens,
) -> Result<bool, Error> {
    match service {
        Service::Atlassian => Ok(false),
        Service::GitHub => {
            let base = crate::http::base_url(github_api)?;
            let req = client()?
                .delete(format!("{base}/applications/{}/token", app.client_id))
                .basic_auth(app.client_id, Some(app.client_secret.expose()))
                .header("Accept", "application/vnd.github+json")
                .header("X-GitHub-Api-Version", "2022-11-28")
                .json(&serde_json::json!({ "access_token": tokens.access.expose() }));
            match send(req).await {
                Ok(_) => Ok(true),
                // Schon widerrufen oder abgelaufen: Ziel erreicht.
                Err(Error::NotFound) => Ok(true),
                Err(f) => Err(f
                    .without_key(app.client_secret.expose())
                    .without_key(tokens.access.expose())),
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
pub async fn sites(e: &Endpoints, access: &Secret) -> Result<Vec<Site>, Error> {
    let base = crate::http::base_url(&e.atlassian_api)?;
    let req = client()?
        .get(format!("{base}/oauth/token/accessible-resources"))
        .bearer_auth(access.expose())
        .header("Accept", "application/json");
    let body = send(req)
        .await
        .map_err(|f| f.without_key(access.expose()))?;
    serde_json::from_str(&body).map_err(|e| Error::Response(e.to_string()))
}

/// Wählt die Site: genau eine automatisch, sonst nach `gewuenscht` (Name, Adresse oder Id aus
/// der Konfiguration). Bei mehreren ohne Wunsch nennt der Fehler die Möglichkeiten.
pub fn site_choose(sites: &[Site], requested: Option<&str>) -> Result<Site, Error> {
    let names = || {
        sites
            .iter()
            .map(|s| s.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    };
    match (sites, requested.map(str::trim).filter(|g| !g.is_empty())) {
        ([], _) => Err(Error::NotPossible(
            "Atlassian hat keine Jira-Site freigegeben – bei der Zustimmung eine Site wählen"
                .into(),
        )),
        (list, Some(requested)) => {
            let w = requested.trim_end_matches('/').to_ascii_lowercase();
            list
                .iter()
                .find(|s| {
                    s.id.eq_ignore_ascii_case(&w)
                        || s.name.to_ascii_lowercase() == w
                        || s.url.trim_end_matches('/').to_ascii_lowercase() == w
                })
                .cloned()
                .ok_or_else(|| {
                    Error::NotPossible(format!(
                        "Die Site „{requested}“ ist nicht freigegeben. Freigegeben: {}",
                        names()
                    ))
                })
        }
        ([eine], None) => Ok(eine.clone()),
        (_, None) => Err(Error::NotPossible(format!(
            "Mehrere Sites freigegeben ({}) – in config.toml unter [atlassian] `site` setzen",
            names()
        ))),
    }
}

/// Die Basis für Jira über das Atlassian-Gateway: `<api>/ex/jira/<cloudId>`.
pub fn jira_base(e: &Endpoints, cloud_id: &str) -> Result<String, Error> {
    let ok = !cloud_id.is_empty()
        && cloud_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-');
    if !ok {
        return Err(Error::Config("Ungültige Site-Id".into()));
    }
    Ok(format!(
        "{}/ex/jira/{cloud_id}",
        crate::http::base_url(&e.atlassian_api)?
    ))
}

// ───────────────────────── laufende Anmeldungen ─────────────────────────

/// So lange gilt ein `state`.
pub const STATE_VALID_MS: u64 = 10 * 60 * 1000;
/// Mehr offene Anmeldungen gleichzeitig gibt es nicht (lokal, eine Person).
const MAX_OPEN: usize = 16;

struct Open {
    service: Service,
    verifier: String,
    ab_ms: u64,
}

/// Warum ein Callback nicht zu einer Anmeldung passt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StateError {
    /// Kein `state` oder unbekannt – auch ein zweiter Aufruf desselben Callbacks.
    Unknown,
    Expired,
    /// Der `state` gehört zu einer Anmeldung bei einem anderen Dienst.
    WrongService,
}

impl StateError {
    pub fn text(&self) -> &'static str {
        match self {
            StateError::Unknown => {
                "Die Anmeldung ist unbekannt oder wurde schon verwendet – bitte neu starten"
            }
            StateError::Expired => "Die Anmeldung hat zu lange gedauert – bitte neu starten",
            StateError::WrongService => "Die Anmeldung gehört zu einem anderen Dienst",
        }
    }
}

/// Offene Anmeldungen: `state` → PKCE-Verifier. Einmalig (wird beim Entnehmen gelöscht) und
/// kurzlebig. Nur im Speicher, nie auf der Platte.
#[derive(Default)]
pub struct SignIns {
    open: HashMap<String, Open>,
}

impl SignIns {
    /// Beginnt eine Anmeldung: liefert `state` und die PKCE-Prüfsumme.
    pub fn begin(&mut self, service: Service, now_ms: u64) -> Result<(String, String), Error> {
        self.open
            .retain(|_, o| now_ms.saturating_sub(o.ab_ms) <= STATE_VALID_MS);
        while self.open.len() >= MAX_OPEN {
            // Die älteste verwerfen.
            let oldest = self
                .open
                .iter()
                .min_by_key(|(_, o)| o.ab_ms)
                .map(|(k, _)| k.clone());
            match oldest {
                Some(k) => self.open.remove(&k),
                None => break,
            };
        }
        let state = random(32)?;
        let verifier = random(48)?;
        let challenge = pkce_challenge(&verifier);
        self.open.insert(
            state.clone(),
            Open {
                service,
                verifier,
                ab_ms: now_ms,
            },
        );
        Ok((state, challenge))
    }

    /// Entnimmt den Verifier zum `state`. Danach ist er verbraucht – auch bei einem Fehler.
    pub fn extract(
        &mut self,
        state: &str,
        service: Service,
        now_ms: u64,
    ) -> Result<String, StateError> {
        let o = self.open.remove(state).ok_or(StateError::Unknown)?;
        if now_ms.saturating_sub(o.ab_ms) > STATE_VALID_MS {
            return Err(StateError::Expired);
        }
        if o.service != service {
            return Err(StateError::WrongService);
        }
        Ok(o.verifier)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_per_rfc_7636() {
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
    fn sign_in_url_for_github_and_atlassian() {
        let e = Endpoints::default();
        let s = Secret::new("geheim");
        let u = sign_in_url(&e, Service::GitHub, &app(&s), "st", "ch").unwrap();
        assert!(u.starts_with("https://github.com/login/oauth/authorize?"));
        assert!(u.contains("client_id=id123"));
        assert!(u.contains("state=st"));
        assert!(u.contains("code_challenge=ch"));
        assert!(u.contains("code_challenge_method=S256"));
        assert!(u.contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A8787"));
        assert!(!u.contains("geheim"));
        assert!(!u.contains("scope"), "GitHub Apps haben keine Scopes");

        let u = sign_in_url(&e, Service::Atlassian, &app(&s), "st", "ch").unwrap();
        assert!(u.starts_with("https://auth.atlassian.com/authorize?"));
        assert!(u.contains("audience=api.atlassian.com"));
        assert!(
            u.contains("scope=read%3Ajira-work+write%3Ajira-work+read%3Ajira-user+offline_access")
        );
        assert!(u.contains("response_type=code"));
        assert!(!u.contains("geheim"));
    }

    #[test]
    fn tokens_roundtrip_and_expiry() {
        let t = Tokens {
            access: Secret::new("a"),
            refresh: Some(Secret::new("r")),
            expires_in_s: Some(1000),
        };
        assert_eq!(Tokens::from_text(&t.as_text()), Some(t.clone()));
        assert!(!t.needs_refresh(800));
        assert!(t.needs_refresh(900), "kurz vor Ablauf");
        assert!(t.needs_refresh(2000));
        let forever = Tokens {
            expires_in_s: None,
            ..t
        };
        assert!(!forever.needs_refresh(u64::MAX / 2));
        assert_eq!(Tokens::from_text("kaputt"), None);
        assert_eq!(Tokens::from_text(r#"{"access":""}"#), None);
        assert!(format!("{forever:?}").contains('a'));
        assert!(!format!("{forever:?}").contains("\"r\""));
    }

    #[test]
    fn token_response_error_and_success() {
        let ok = tokens_from(
            r#"{"access_token":"A","refresh_token":"R","expires_in":3600}"#,
            100,
            None,
            &[],
        )
        .unwrap();
        assert_eq!(ok.access.expose(), "A");
        assert_eq!(ok.refresh.unwrap().expose(), "R");
        assert_eq!(ok.expires_in_s, Some(3700));

        // Ohne neues Refresh-Token bleibt das alte.
        let alt = Secret::new("alt");
        let t = tokens_from(r#"{"access_token":"A"}"#, 0, Some(&alt), &[]).unwrap();
        assert_eq!(t.refresh.unwrap().expose(), "alt");
        assert_eq!(t.expires_in_s, None);

        // GitHub: 200 mit Fehler.
        let f = tokens_from(
            r#"{"error":"bad_verification_code","error_description":"Der Code ist abgelaufen GEHEIMNIS123"}"#,
            0,
            None,
            &["GEHEIMNIS123"],
        )
        .unwrap_err();
        assert!(
            matches!(&f, Error::Rejected(m) if m.contains("abgelaufen") && !m.contains("GEHEIMNIS123"))
        );
        assert!(matches!(
            tokens_from("{}", 0, None, &[]),
            Err(Error::Response(_))
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
    fn site_choice() {
        let eine = [site("abc-1", "firma")];
        assert_eq!(site_choose(&eine, None).unwrap().id, "abc-1");
        let two = [site("abc-1", "firma"), site("def-2", "privat")];
        let f = site_choose(&two, None).unwrap_err().to_string();
        assert!(
            f.contains("firma") && f.contains("privat") && f.contains("site"),
            "{f}"
        );
        for requested in ["privat", "PRIVAT", "def-2", "https://privat.atlassian.net/"] {
            assert_eq!(
                site_choose(&two, Some(requested)).unwrap().id,
                "def-2",
                "{requested}"
            );
        }
        assert!(site_choose(&two, Some("fremd")).is_err());
        assert!(
            site_choose(&[], None)
                .unwrap_err()
                .to_string()
                .contains("keine Jira-Site")
        );
        // Leerer Wunsch zählt als keiner.
        assert!(site_choose(&eine, Some(" ")).is_ok());
    }

    #[test]
    fn jira_base_checks_the_id() {
        let e = Endpoints::default();
        assert_eq!(
            jira_base(&e, "1324a887-451a").unwrap(),
            "https://api.atlassian.com/ex/jira/1324a887-451a"
        );
        for bad in ["", "a/b", "a?b", "../x"] {
            assert!(jira_base(&e, bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn state_is_single_use_short_lived_and_service_bound() {
        let mut a = SignIns::default();
        let (state, challenge) = a.begin(Service::GitHub, 1000).unwrap();
        assert!(state.len() >= 40, "genug Zufall");
        let verifier = a.extract(&state, Service::GitHub, 2000).unwrap();
        assert_eq!(pkce_challenge(&verifier), challenge);
        // Zweiter Callback mit demselben state.
        assert_eq!(
            a.extract(&state, Service::GitHub, 2000),
            Err(StateError::Unknown)
        );
        // Abgelaufen.
        let (s2, _) = a.begin(Service::GitHub, 0).unwrap();
        assert_eq!(
            a.extract(&s2, Service::GitHub, STATE_VALID_MS + 1),
            Err(StateError::Expired)
        );
        // Anderer Dienst – und der state ist trotzdem verbraucht.
        let (s3, _) = a.begin(Service::Atlassian, 0).unwrap();
        assert_eq!(
            a.extract(&s3, Service::GitHub, 1),
            Err(StateError::WrongService)
        );
        assert_eq!(
            a.extract(&s3, Service::Atlassian, 1),
            Err(StateError::Unknown)
        );
        assert_eq!(
            a.extract("", Service::GitHub, 1),
            Err(StateError::Unknown)
        );
    }

    #[test]
    fn open_sign_ins_are_limited() {
        let mut a = SignIns::default();
        for i in 0..40 {
            a.begin(Service::GitHub, i).unwrap();
        }
        assert!(a.open.len() <= MAX_OPEN);
    }

    #[test]
    fn service_ids() {
        for d in [Service::GitHub, Service::Atlassian] {
            assert_eq!(Service::from_id(d.id()), Some(d));
        }
        assert_eq!(Service::from_id("gitlab"), None);
    }
}
