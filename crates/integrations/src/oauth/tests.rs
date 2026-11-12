use super::*;

#[test]
fn pkce_per_rfc_7636() {
    // Example from RFC 7636, Appendix B.
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
    assert!(!u.contains("scope"), "GitHub Apps have no scopes");

    let u = sign_in_url(&e, Service::Atlassian, &app(&s), "st", "ch").unwrap();
    assert!(u.starts_with("https://auth.atlassian.com/authorize?"));
    assert!(u.contains("audience=api.atlassian.com"));
    assert!(u.contains("scope=read%3Ajira-work+write%3Ajira-work+read%3Ajira-user+offline_access"));
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

    // Without a new refresh token the old one stays.
    let old = Secret::new("alt");
    let t = tokens_from(r#"{"access_token":"A"}"#, 0, Some(&old), &[]).unwrap();
    assert_eq!(t.refresh.unwrap().expose(), "alt");
    assert_eq!(t.expires_in_s, None);

    // GitHub: 200 with error.
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
    // An empty request counts as none.
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
    // Second callback with the same state.
    assert_eq!(
        a.extract(&state, Service::GitHub, 2000),
        Err(StateError::Unknown)
    );
    // Expired.
    let (s2, _) = a.begin(Service::GitHub, 0).unwrap();
    assert_eq!(
        a.extract(&s2, Service::GitHub, STATE_VALID_MS + 1),
        Err(StateError::Expired)
    );
    // Another service – and the state is used up anyway.
    let (s3, _) = a.begin(Service::Atlassian, 0).unwrap();
    assert_eq!(
        a.extract(&s3, Service::GitHub, 1),
        Err(StateError::WrongService)
    );
    assert_eq!(
        a.extract(&s3, Service::Atlassian, 1),
        Err(StateError::Unknown)
    );
    assert_eq!(a.extract("", Service::GitHub, 1), Err(StateError::Unknown));
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
