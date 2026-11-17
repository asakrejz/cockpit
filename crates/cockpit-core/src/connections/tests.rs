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
    assert!(c.agent_blocked().unwrap().contains("Pull Requests"));
    c.may_branch = false;
    assert!(c.agent_blocked().unwrap().contains("Branches"));
    c.may_branch = true;
    c.may_open_pr = true;
    c.may_run_tests = false;
    assert_eq!(c.agent_blocked(), None, "Tests sind optional");
}
