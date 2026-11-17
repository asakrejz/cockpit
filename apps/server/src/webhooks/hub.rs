//! The hub: rate limit, replay protection and debounce per source.

use super::*;

/// What to do with a trigger.
#[derive(Debug, PartialEq, Eq)]
pub enum Admit {
    /// A sync is already scheduled; this webhook is merged into it.
    Merged,
    /// Sync after this long (zero = now).
    After(Duration),
}

/// Merges triggers: one sync at once (leading edge), later ones within `debounce` into a single
/// one after it (trailing edge).
#[derive(Debug, Default)]
pub struct Gate {
    last_run: Option<Instant>,
    scheduled: bool,
}

impl Gate {
    pub fn admit(&mut self, now: Instant, debounce: Duration) -> Admit {
        if self.scheduled {
            return Admit::Merged;
        }
        self.scheduled = true;
        let wait = self.last_run.map_or(Duration::ZERO, |l| {
            (l + debounce).saturating_duration_since(now)
        });
        Admit::After(wait)
    }

    pub fn started(&mut self, now: Instant) {
        self.last_run = Some(now);
        self.scheduled = false;
    }
}

#[derive(Default)]
pub(super) struct PerSource {
    /// When a valid request last arrived (wall clock, for display) and its event type.
    last_received: Option<(u64, String)>,
    /// Last valid delivery of a relevant event (not ping/test): the webhooks "work".
    last_active: Option<Instant>,
    /// When the sync last asked this source.
    last_fetch: Option<Instant>,
    gate: Gate,
    rate: VecDeque<Instant>,
    pub(super) seen: VecDeque<(Instant, String)>,
}

impl PerSource {
    /// Counts a request; `false` if there were too many in the window.
    pub(super) fn rate_ok(&mut self, now: Instant) -> bool {
        while self
            .rate
            .front()
            .is_some_and(|t| now.saturating_duration_since(*t) > RATE_WINDOW)
        {
            self.rate.pop_front();
        }
        if self.rate.len() >= RATE_LIMIT {
            return false;
        }
        self.rate.push_back(now);
        true
    }

    /// `true` if one of the keys (delivery ID, body hash) was seen recently; remembers them.
    pub(super) fn seen_before(&mut self, now: Instant, keys: &[String]) -> bool {
        while self
            .seen
            .front()
            .is_some_and(|(t, _)| now.saturating_duration_since(*t) > REPLAY_TTL)
        {
            self.seen.pop_front();
        }
        if keys.iter().any(|k| self.seen.iter().any(|(_, s)| s == k)) {
            return true;
        }
        for k in keys {
            if self.seen.len() >= REPLAY_MAX {
                self.seen.pop_front();
            }
            self.seen.push_back((now, k.clone()));
        }
        false
    }
}

/// Everything the webhooks need to remember, in memory only.
pub struct Hub {
    settings: RwLock<WebhookSettings>,
    sources: Mutex<[PerSource; 2]>,
}

pub(super) fn idx(source: Source) -> usize {
    match source {
        Source::GitHub => 0,
        Source::Jira => 1,
    }
}

impl Hub {
    pub fn new(settings: WebhookSettings) -> Self {
        Self {
            settings: RwLock::new(settings),
            sources: Mutex::new(Default::default()),
        }
    }

    pub fn settings(&self) -> WebhookSettings {
        self.settings
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Takes over `[server]` after a change in the assistant.
    pub fn apply(&self, enabled: bool, public_url: Option<String>) {
        let mut s = self.settings.write().unwrap_or_else(|e| e.into_inner());
        s.enabled = enabled;
        s.public_url = public_url;
    }

    pub(super) fn with<R>(&self, source: Source, f: impl FnOnce(&mut PerSource) -> R) -> R {
        let mut all = self.sources.lock().unwrap_or_else(|e| e.into_inner());
        f(&mut all[idx(source)])
    }

    pub fn rate_ok(&self, source: Source, now: Instant) -> bool {
        self.with(source, |p| p.rate_ok(now))
    }

    pub fn seen_before(&self, source: Source, now: Instant, keys: &[String]) -> bool {
        self.with(source, |p| p.seen_before(now, keys))
    }

    pub fn received(&self, source: Source, at_ms: u64, event: &str, active: Option<Instant>) {
        self.with(source, |p| {
            p.last_received = Some((at_ms, event.to_string()));
            if active.is_some() {
                p.last_active = active;
            }
        });
    }

    pub fn last_received(&self, source: Source) -> Option<(u64, String)> {
        self.with(source, |p| p.last_received.clone())
    }

    pub fn admit(&self, source: Source, now: Instant) -> Admit {
        let debounce = self.settings().debounce;
        self.with(source, |p| p.gate.admit(now, debounce))
    }

    pub fn started(&self, source: Source, now: Instant) {
        self.with(source, |p| p.gate.started(now));
    }

    pub fn fetched(&self, source: Source, now: Instant) {
        self.with(source, |p| p.last_fetch = Some(now));
    }

    /// Whether the sync tick should ask `source` now.
    pub fn poll_due(&self, source: Source, now: Instant, base: Duration) -> bool {
        let s = self.settings();
        let (active, fetch) = self.with(source, |p| (p.last_active, p.last_fetch));
        poll_due(
            s.enabled,
            active.map(|t| now.saturating_duration_since(t)),
            fetch.map(|t| now.saturating_duration_since(t)),
            base,
            Duration::from_secs(s.fallback_s),
        )
    }
}

/// The poll decision, with elapsed times instead of clocks (so tests need no waiting).
///
/// Without webhooks, or when the last relevant webhook is older than [`HEALTHY_INTERVALS`]
/// intervals (or none arrived yet), every tick polls. While they arrive regularly, a source is
/// polled only as a safety net every `fallback`; `base / 2` of slack keeps tick jitter from
/// skipping a whole round.
pub fn poll_due(
    enabled: bool,
    since_active: Option<Duration>,
    since_fetch: Option<Duration>,
    base: Duration,
    fallback: Duration,
) -> bool {
    let healthy = enabled && since_active.is_some_and(|a| a <= base * HEALTHY_INTERVALS);
    if !healthy {
        return true;
    }
    since_fetch.is_none_or(|f| f + base / 2 >= fallback)
}

impl AppState {
    /// A valid webhook arrived: sync `source` soon, merged with others (see [`Gate`]).
    pub(crate) fn webhook_trigger(&self, source: Source) {
        let hub = &self.inner.webhooks;
        let Admit::After(wait) = hub.admit(source, Instant::now()) else {
            return;
        };
        let state = self.clone();
        tokio::spawn(async move {
            if !wait.is_zero() {
                tokio::time::sleep(wait).await;
            }
            state.inner.webhooks.started(source, Instant::now());
            if let Err(e) = state.sync_sources(&[source]).await {
                tracing::warn!("Sync after webhook: {e:?}");
            }
        });
    }

    /// The sources whose turn it is at this tick of the sync.
    pub(crate) fn sources_due(&self) -> Vec<Source> {
        let base = self.sources().interval;
        let now = Instant::now();
        [Source::GitHub, Source::Jira]
            .into_iter()
            .filter(|s| self.inner.webhooks.poll_due(*s, now, base))
            .collect()
    }

    /// The state of the webhooks for the assistant. Blocking (.env file).
    pub(crate) fn webhooks_info(&self) -> WebhooksInfo {
        let hub = &self.inner.webhooks;
        let s = hub.settings();
        let info = |source: Source, path: &str| {
            let last = hub.last_received(source);
            WebhookSourceInfo {
                path: path.to_string(),
                secret_set: self.token(secret_name(source)).is_some(),
                last_received_ms: last.as_ref().map(|l| l.0),
                last_event: last.map(|l| l.1),
            }
        };
        WebhooksInfo {
            enabled: s.enabled,
            public_url: s.public_url,
            fallback_interval_s: s.fallback_s,
            github: info(Source::GitHub, GITHUB_PATH),
            jira: info(Source::Jira, JIRA_PATH),
        }
    }
}
