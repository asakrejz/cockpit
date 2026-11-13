use super::*;

const NOW: u64 = 1_800_000_000_000;

fn state(problem: Option<Problem>) -> SourceState {
    SourceState {
        source: Source::GitHub,
        problem,
        last_ok_ms: Some(NOW - 10 * 60_000),
        last_try_ms: NOW,
        since_ms: Some(NOW - 60_000),
        warnings: Vec::new(),
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
    assert!(n.title.contains("Token abgelehnt"));
    assert!(n.detail.contains("Einrichten"));
    assert!(n.detail.contains("vor 10 Min."));
}

#[test]
fn forbidden_is_not_a_token_problem() {
    let n = source_notice(&state(Some(Problem::NoPermission)), NOW).unwrap();
    assert_eq!(n.action, NoticeAction::OpenSetup);
    assert!(n.title.contains("Berechtigung"));
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
    assert!(n.detail.contains("in 2 Min."), "{}", n.detail);
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
    assert_eq!(n.title, "GitHub nicht erreichbar");
    assert!(n.detail.contains("Letzter Stand: vor 10 Min."));
    assert!(!n.detail.contains(".."), "{}", n.detail);
    let mut never = state(Some(Problem::Offline));
    never.last_ok_ms = None;
    let n = source_notice(&never, NOW).unwrap();
    assert!(n.detail.contains("noch keinen abgeglichenen Stand"));
}

#[test]
fn server_error_keeps_the_message() {
    let p = Problem::Other("Unerwarteter Status vom Dienst: 502".into());
    let n = source_notice(&state(Some(p)), NOW).unwrap();
    assert!(n.detail.contains("502"));
    assert_eq!(n.action, NoticeAction::RetryNow);
}

#[test]
fn times_are_coarse_and_rounded_up_for_waits() {
    assert_eq!(ago(NOW, NOW), "gerade eben");
    assert_eq!(ago(NOW - 2 * 3_600_000, NOW), "vor 2 Std.");
    assert_eq!(ago(NOW - 3 * 86_400_000, NOW), "vor 3 Tagen");
    assert_eq!(wait_text(NOW + 20_000, NOW), "in 20 Sek.");
    assert_eq!(wait_text(NOW + 61_000, NOW), "in 2 Min.");
    assert_eq!(wait_text(NOW, NOW), "gleich");
}

#[test]
fn http_status_texts() {
    assert!(http_failure_text(401, None).contains("Einrichten"));
    assert_eq!(
        http_failure_text(403, Some("Nur du darfst mergen")),
        "Nur du darfst mergen"
    );
    assert!(http_failure_text(429, None).contains("warten"));
    assert!(http_failure_text(500, None).contains("500"));
    assert_eq!(http_failure_text(422, Some("Ungültig")), "Ungültig");
    assert!(http_failure_text(418, None).contains("418"));
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
