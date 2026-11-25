use super::*;
use cockpit_i18n::Lang;

const NOW: u64 = 1_800_000_000_000;

fn state(problem: Option<Problem>) -> SourceState {
    SourceState {
        source: Source::GitHub,
        problem,
        last_ok_ms: Some(NOW - 10 * 60_000),
        last_try_ms: NOW,
        since_ms: Some(NOW - 60_000),
        warnings: Vec::new(),
        warning_msgs: Vec::new(),
    }
}

#[test]
fn healthy_source_has_no_notice() {
    assert!(source_notice(&state(None), NOW).is_none());
    assert!(source_notices(&SyncState::default(), NOW).is_empty());
}

#[test]
fn unauthorized_points_to_setup() {
    let n = source_notice(&state(Some(Problem::Unauthorized)), NOW).unwrap();
    assert_eq!(n.tone, NoticeTone::Error);
    assert_eq!(n.action, NoticeAction::OpenSetup);
    assert!(n.title.render(Lang::De).contains("Token abgelehnt"));
    assert!(n.detail.render(Lang::De).contains("Einrichten"));
    assert!(n.detail.render(Lang::De).contains("vor 10 Minuten"));
}

#[test]
fn forbidden_is_not_a_token_problem() {
    let n = source_notice(&state(Some(Problem::NoPermission)), NOW).unwrap();
    assert_eq!(n.action, NoticeAction::OpenSetup);
    assert!(n.title.render(Lang::De).contains("Berechtigung"));
}

#[test]
fn rate_limit_with_time_waits_and_names_the_wait() {
    let p = Problem::RateLimited {
        until_ms: Some(NOW + 90_000),
    };
    let n = source_notice(&state(Some(p)), NOW).unwrap();
    assert_eq!(n.tone, NoticeTone::Info);
    assert_eq!(n.action, NoticeAction::Wait);
    assert_eq!(n.retry_at_ms, Some(NOW + 90_000));
    assert!(
        n.detail.render(Lang::De).contains("in 2 Min."),
        "{}",
        n.detail.render(Lang::De)
    );
}

#[test]
fn rate_limit_without_time_or_in_the_past_retries() {
    for p in [
        Problem::RateLimited { until_ms: None },
        Problem::RateLimited {
            until_ms: Some(NOW - 1),
        },
    ] {
        let n = source_notice(&state(Some(p)), NOW).unwrap();
        assert_eq!(n.action, NoticeAction::RetryNow);
        assert_eq!(n.retry_at_ms, None);
    }
}

#[test]
fn unreachable_shows_the_last_good_state() {
    let n = source_notice(&state(Some(Problem::Offline)), NOW).unwrap();
    assert_eq!(n.title.render(Lang::De), "GitHub nicht erreichbar");
    assert!(
        n.detail
            .render(Lang::De)
            .contains("Letzter Stand: vor 10 Minuten")
    );
    assert!(
        !n.detail.render(Lang::De).contains(".."),
        "{}",
        n.detail.render(Lang::De)
    );
    let mut never = state(Some(Problem::Offline));
    never.last_ok_ms = None;
    let n = source_notice(&never, NOW).unwrap();
    assert!(
        n.detail
            .render(Lang::De)
            .contains("noch keinen abgeglichenen Stand")
    );
}

#[test]
fn server_error_keeps_the_message() {
    let p = Problem::Other(msg!(IntegrationsUnexpectedStatus, code = 502));
    let n = source_notice(&state(Some(p)), NOW).unwrap();
    assert!(
        n.detail
            .render(Lang::De)
            .contains("Unerwarteter Status vom Dienst: 502")
    );
    assert!(
        n.detail
            .render(cockpit_i18n::Lang::En)
            .contains("Unexpected status from the service: 502")
    );
    assert_eq!(n.action, NoticeAction::RetryNow);
}

#[test]
fn times_are_coarse_and_rounded_up_for_waits() {
    assert_eq!(ago_msg(NOW, NOW).render(Lang::De), "gerade eben");
    assert_eq!(
        ago_msg(NOW - 2 * 3_600_000, NOW).render(Lang::De),
        "vor 2 Stunden"
    );
    assert_eq!(
        ago_msg(NOW - 3 * 86_400_000, NOW).render(Lang::De),
        "vor 3 Tagen"
    );
    assert_eq!(wait_msg(NOW + 20_000, NOW).render(Lang::De), "in 20 Sek.");
    assert_eq!(wait_msg(NOW + 61_000, NOW).render(Lang::De), "in 2 Min.");
    assert_eq!(wait_msg(NOW, NOW).render(Lang::De), "gleich");
}

#[test]
fn http_status_texts() {
    let de = |status, own| http_failure_msg(status, own).render(Lang::De);
    assert!(de(401, None).contains("Einrichten"));
    assert_eq!(
        de(403, Some("Nur du darfst mergen")),
        "Nur du darfst mergen"
    );
    assert!(de(429, None).contains("Warte"));
    assert!(de(500, None).contains("500"));
    assert_eq!(de(422, Some("Ungültig")), "Ungültig");
    assert!(de(418, None).contains("418"));
}

#[test]
fn since_stays_while_the_same_problem_repeats_and_resets_on_recovery() {
    let mut s = SyncState::default();
    s.set_state(Source::GitHub, Some(Problem::Offline), NOW, &[]);
    assert_eq!(s.sources[0].since_ms, Some(NOW));
    s.set_state(Source::GitHub, Some(Problem::Offline), NOW + 5_000, &[]);
    assert_eq!(
        s.sources[0].since_ms,
        Some(NOW),
        "same problem: since stays"
    );
    s.set_state(
        Source::GitHub,
        Some(Problem::Unauthorized),
        NOW + 9_000,
        &[],
    );
    assert_eq!(
        s.sources[0].since_ms,
        Some(NOW + 9_000),
        "new kind: since moves"
    );
    s.set_state(Source::GitHub, None, NOW + 20_000, &[]);
    assert_eq!(s.sources[0].since_ms, None);
    assert_eq!(s.sources[0].last_ok_ms, Some(NOW + 20_000));
}

#[test]
fn old_snapshot_without_since_still_loads() {
    let old =
        r#"{"sources":[{"source":"GitHub","problem":"Offline","last_ok_ms":5,"last_try_ms":6}]}"#;
    let s: SyncState = serde_json::from_str(old).unwrap();
    assert_eq!(s.sources[0].since_ms, None);
    assert!(s.sources[0].problem.is_some());
}

#[test]
fn reconnect_backs_off_and_caps() {
    let d: Vec<u32> = (0..7).map(reconnect_delay_s).collect();
    assert_eq!(d, [2, 4, 8, 15, 30, 30, 30]);
}
