use super::*;

#[test]
fn service_status_reads_old_and_partial_json() {
    let s: ServiceStatus = serde_json::from_str("{}").unwrap();
    assert_eq!(s, ServiceStatus::default());
    let s: ServiceStatus =
        serde_json::from_str(r#"{"connected":true,"kind":"oauth","site":"firma"}"#).unwrap();
    assert_eq!(s.kind, AuthKind::OAuth);
    assert_eq!(
        serde_json::to_string(&AuthKind::Token).unwrap(),
        "\"token\""
    );
}

fn status(connected: bool, kind: AuthKind, expired: bool, site: Option<&str>) -> ServiceStatus {
    ServiceStatus {
        connected,
        kind,
        expired,
        site: site.map(Into::into),
        ..Default::default()
    }
}

#[test]
fn the_summary_has_one_tone_and_sentence_per_state() {
    use AuthKind::{None as Nothing, OAuth, Token};
    let summary = |s: ServiceStatus| {
        let (tone, text) = s.summary("Atlassian");
        (tone, text.render(cockpit_i18n::Lang::En))
    };
    assert_eq!(
        summary(status(true, OAuth, false, None)),
        (
            Tone::Good,
            "Connected. You are signed in through Atlassian.".to_string()
        )
    );
    // The site belongs to the sign-in and is named in the text.
    assert_eq!(
        summary(status(true, OAuth, false, Some("acme"))),
        (
            Tone::Good,
            "Connected. You are signed in through Atlassian (site acme).".to_string()
        )
    );
    assert_eq!(
        summary(status(true, Token, false, None)),
        (Tone::Good, "Connected, with an API token.".to_string())
    );
    // A token that takes over from an expired sign-in still counts as connected.
    assert_eq!(summary(status(true, Token, true, None)).0, Tone::Good);
    assert_eq!(
        summary(status(false, Nothing, false, None)),
        (Tone::Open, "Disconnected".to_string())
    );
    // An expired sign-in without a token is a problem and says what to do, not "disconnected".
    assert_eq!(
        summary(status(false, Nothing, true, None)),
        (
            Tone::Problem,
            "The sign-in has run out. Sign in again.".to_string()
        )
    );
}

#[test]
fn only_a_settled_state_needs_no_hint_about_the_setup() {
    use AuthKind::{None as Nothing, Token};
    assert!(status(true, Token, false, None).is_settled());
    assert!(status(false, Nothing, true, None).is_settled());
    assert!(!status(false, Nothing, false, None).is_settled());
}

fn resolved(token: bool, oauth: OAuthTokens) -> (bool, AuthKind, bool) {
    let s = ServiceStatus::resolve(None, token, oauth, true, Some("acme".into()));
    (s.connected, s.kind, s.expired)
}

#[test]
fn one_rule_for_every_combination_of_token_and_sign_in() {
    use OAuthTokens::*;
    // Only a token.
    assert_eq!(resolved(true, Missing), (true, AuthKind::Token, false));
    // Only the sign-in.
    assert_eq!(resolved(false, Valid), (true, AuthKind::OAuth, false));
    // Both: the sign-in goes first.
    assert_eq!(resolved(true, Valid), (true, AuthKind::OAuth, false));
    // Nothing.
    assert_eq!(resolved(false, Missing), (false, AuthKind::None, false));
    // Expired without a token: not connected, and it says why.
    assert_eq!(resolved(false, Expired), (false, AuthKind::None, true));
    // Expired with a token: the token keeps GitHub connected.
    assert_eq!(resolved(true, Expired), (true, AuthKind::Token, true));
}

#[test]
fn the_site_only_shows_with_a_working_sign_in() {
    let on = ServiceStatus::resolve(None, false, OAuthTokens::Valid, true, Some("acme".into()));
    assert_eq!(on.site.as_deref(), Some("acme"));
    let token = ServiceStatus::resolve(None, true, OAuthTokens::Missing, true, Some("acme".into()));
    assert_eq!(token.site, None);
}

#[test]
fn the_choice_decides_which_credentials_count() {
    use AuthChoice::{OAuth, Token};
    use OAuthTokens::*;
    let kind = AuthChoice::pick;
    // Token chosen: the sign-in is kept but not used, even if it works.
    assert_eq!(kind(Some(Token), true, Valid), AuthKind::Token);
    assert_eq!(kind(Some(Token), false, Valid), AuthKind::None);
    assert_eq!(kind(Some(Token), true, Expired), AuthKind::Token);
    // Sign-in chosen: the token is kept but not used.
    assert_eq!(kind(Some(OAuth), true, Valid), AuthKind::OAuth);
    assert_eq!(kind(Some(OAuth), true, Missing), AuthKind::None);
    assert_eq!(kind(Some(OAuth), true, Expired), AuthKind::None);
    // No choice: a valid sign-in wins, an expired one falls back to the token.
    assert_eq!(kind(None, true, Valid), AuthKind::OAuth);
    assert_eq!(kind(None, true, Expired), AuthKind::Token);
    assert_eq!(kind(None, false, Missing), AuthKind::None);
}

#[test]
fn status_reports_choice_and_a_stored_sign_in_that_is_not_used() {
    let s = ServiceStatus::resolve(
        Some(AuthChoice::Token),
        true,
        OAuthTokens::Valid,
        true,
        Some("acme".into()),
    );
    assert_eq!((s.connected, s.kind), (true, AuthKind::Token));
    assert!(s.oauth_stored && !s.expired);
    assert_eq!(s.choice, Some(AuthChoice::Token));
    assert_eq!(s.site, None);
    assert_eq!(
        serde_json::to_string(&AuthChoice::OAuth).unwrap(),
        "\"oauth\""
    );
}
