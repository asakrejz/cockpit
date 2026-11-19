use super::*;

#[test]
fn server_reports_the_kind() {
    let mut c = Connections::default();
    let oauth = ServiceStatus {
        connected: true,
        kind: AuthKind::OAuth,
        ..Default::default()
    };
    c.apply_services(&oauth, &ServiceStatus::default());
    assert!(c.github && !c.jira);
    assert_eq!(
        (c.github_auth, c.jira_auth),
        (AuthKind::OAuth, AuthKind::None)
    );
}

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

#[test]
fn agent_needs_connection_and_permissions() {
    let mut c = Connections::default();
    assert!(c.agent_blocked().is_some());
    c.agent = true;
    assert_eq!(c.agent_blocked(), None);
    c.may_open_pr = false;
    assert!(c.agent_blocked().unwrap().de().contains("Pull Requests"));
    c.may_branch = false;
    assert!(c.agent_blocked().unwrap().de().contains("Branches"));
    c.may_branch = true;
    c.may_open_pr = true;
    c.may_run_tests = false;
    assert_eq!(c.agent_blocked(), None, "tests are optional");
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
    // No choice: the older rule.
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
