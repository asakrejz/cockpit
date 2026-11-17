use super::*;

// ---- debounce, interval --------------------------------------------------------------------

#[test]
fn gate_merges_triggers() {
    let t0 = Instant::now();
    let d = Duration::from_secs(5);
    let mut g = Gate::default();
    // The first one runs at once; the next ones merge until it has started.
    assert_eq!(g.admit(t0, d), Admit::After(Duration::ZERO));
    assert_eq!(g.admit(t0, d), Admit::Merged);
    g.started(t0);
    // Within the debounce time: one run after the rest of it, further ones merge.
    let t1 = t0 + Duration::from_secs(2);
    assert_eq!(g.admit(t1, d), Admit::After(Duration::from_secs(3)));
    assert_eq!(g.admit(t1, d), Admit::Merged);
    g.started(t0 + Duration::from_secs(5));
    // Long afterwards: at once again.
    let t2 = t0 + Duration::from_secs(60);
    assert_eq!(g.admit(t2, d), Admit::After(Duration::ZERO));
}

#[test]
fn rate_limit_and_replay_window() {
    let t0 = Instant::now();
    let mut p = PerSource::default();
    for _ in 0..RATE_LIMIT {
        assert!(p.rate_ok(t0));
    }
    assert!(!p.rate_ok(t0));
    assert!(p.rate_ok(t0 + RATE_WINDOW + Duration::from_secs(1)));

    let keys = vec!["id:1".to_string()];
    assert!(!p.seen_before(t0, &keys));
    assert!(p.seen_before(t0 + Duration::from_secs(5), &keys));
    assert!(!p.seen_before(t0 + REPLAY_TTL + Duration::from_secs(1), &keys));
    // The memory stays bounded.
    for i in 0..(REPLAY_MAX * 2) {
        p.seen_before(t0, &[format!("id:x{i}")]);
    }
    assert!(p.seen.len() <= REPLAY_MAX);
}

#[test]
fn poll_interval_follows_the_webhooks() {
    let base = Duration::from_secs(120);
    let fallback = Duration::from_secs(600);
    let s = Duration::from_secs;
    // Webhooks off, or none arrived yet: every tick polls.
    assert!(poll_due(false, Some(s(1)), Some(s(1)), base, fallback));
    assert!(poll_due(true, None, Some(s(1)), base, fallback));
    // Arriving regularly: only the safety net polls ...
    assert!(!poll_due(true, Some(s(100)), Some(s(120)), base, fallback));
    assert!(!poll_due(true, Some(s(100)), Some(s(480)), base, fallback));
    // ... every `fallback` (with half an interval of slack for the tick).
    assert!(poll_due(true, Some(s(100)), Some(s(540)), base, fallback));
    assert!(poll_due(true, Some(s(100)), None, base, fallback));
    // The last webhook is older than two intervals: back to the normal interval.
    assert!(poll_due(true, Some(s(241)), Some(s(120)), base, fallback));
    assert!(!poll_due(true, Some(s(240)), Some(s(120)), base, fallback));
}
