//! Benachrichtigungen: Welche Ereignisse sind der Rede wert, und wohin gehen sie?
//!
//! Alles hier ist rein (ohne Browser, ohne Uhr, ohne Netzwerk):
//!
//! * [`events`] vergleicht den Workspace vor und nach einer Aktion und leitet die
//!   [`Event`]s ab: Review-Anfrage, Erwähnung, Agent wartet, Agent fertig, Checks rot,
//!   Ticket zugewiesen.
//! * [`NotifySettings`] hält pro Grund die Kanäle ([`Channel`]) und die Ruhezeiten.
//!   Die Einstellungen liegen im [`Workspace`](crate::Workspace) und ändern sich nur über
//!   Aktionen.
//! * [`deliveries`] sagt, welches Ereignis über welchen Kanal gemeldet wird.
//!
//! **Browser** versendet die App selbst. **E-Mail** und **Slack** sind nur vorbereitet: Die Typen
//! sind `serde`-fähig, damit das Backend sie später verwenden kann; Versandcode gibt es nicht.
//! Adressen und Webhook-URLs stehen nie in diesen Typen – sie gehören ins Backend.

use serde::{Deserialize, Serialize};

use crate::agent::AgentRun;
use crate::model::{Checks, Item, Person, ReasonKind, Scope};
use crate::workspace::Workspace;

/// Minuten eines Tages.
const DAY: u16 = 24 * 60;

/// Worüber man benachrichtigt werden kann.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NotifyReason {
    ReviewRequested,
    Mention,
    AgentWaiting,
    AgentFinished,
    ChecksFailed,
    Assigned,
}

impl NotifyReason {
    pub const ALL: [NotifyReason; 6] = [
        NotifyReason::ReviewRequested,
        NotifyReason::Mention,
        NotifyReason::AgentWaiting,
        NotifyReason::AgentFinished,
        NotifyReason::ChecksFailed,
        NotifyReason::Assigned,
    ];

    pub fn id(self) -> &'static str {
        match self {
            NotifyReason::ReviewRequested => "review",
            NotifyReason::Mention => "erwaehnung",
            NotifyReason::AgentWaiting => "agent-wartet",
            NotifyReason::AgentFinished => "agent-fertig",
            NotifyReason::ChecksFailed => "checks",
            NotifyReason::Assigned => "zugewiesen",
        }
    }

    pub fn from_id(id: &str) -> Option<NotifyReason> {
        Self::ALL.into_iter().find(|r| r.id() == id)
    }

    pub fn label(self) -> &'static str {
        match self {
            NotifyReason::ReviewRequested => "Review-Anfrage an mich",
            NotifyReason::Mention => "Erwähnung",
            NotifyReason::AgentWaiting => "Agent wartet auf mich",
            NotifyReason::AgentFinished => "Agent fertig",
            NotifyReason::ChecksFailed => "Checks fehlgeschlagen",
            NotifyReason::Assigned => "Ticket mir zugewiesen",
        }
    }

    /// Ein Satz dazu, wann es meldet.
    pub fn hint(self) -> &'static str {
        match self {
            NotifyReason::ReviewRequested => "Jemand bittet dich um ein Review",
            NotifyReason::Mention => "Dein Name oder dein Kürzel steht in einem Kommentar (@Du)",
            NotifyReason::AgentWaiting => {
                "Ein Plan liegt zur Freigabe vor oder der Agent hat eine Rückfrage"
            }
            NotifyReason::AgentFinished => "Der Agent hat einen Pull Request erstellt",
            NotifyReason::ChecksFailed => "Die Checks deines Pull Requests werden rot",
            NotifyReason::Assigned => "Dir wird ein Ticket zugewiesen",
        }
    }
}

/// Wohin eine Benachrichtigung gehen kann.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Channel {
    /// Systemmeldung des Browsers (Notification-API) und Hinweis in der App.
    Browser,
    /// Wird später vom Backend versendet.
    Email,
    /// Wird später vom Backend versendet.
    Slack,
}

impl Channel {
    pub const ALL: [Channel; 3] = [Channel::Browser, Channel::Email, Channel::Slack];

    pub fn id(self) -> &'static str {
        match self {
            Channel::Browser => "browser",
            Channel::Email => "email",
            Channel::Slack => "slack",
        }
    }

    pub fn from_id(id: &str) -> Option<Channel> {
        Self::ALL.into_iter().find(|c| c.id() == id)
    }

    pub fn label(self) -> &'static str {
        match self {
            Channel::Browser => "Browser",
            Channel::Email => "E-Mail",
            Channel::Slack => "Slack",
        }
    }

    /// Versendet das Backend (noch nicht gebaut), nicht der Browser?
    pub fn via_backend(self) -> bool {
        self != Channel::Browser
    }
}

/// Welche Kanäle für einen Grund an sind.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Channels {
    #[serde(default)]
    pub browser: bool,
    #[serde(default)]
    pub email: bool,
    #[serde(default)]
    pub slack: bool,
}

impl Channels {
    pub fn get(self, channel: Channel) -> bool {
        match channel {
            Channel::Browser => self.browser,
            Channel::Email => self.email,
            Channel::Slack => self.slack,
        }
    }

    fn set(&mut self, channel: Channel, on: bool) {
        match channel {
            Channel::Browser => self.browser = on,
            Channel::Email => self.email = on,
            Channel::Slack => self.slack = on,
        }
    }
}

/// Ruhezeiten: in dieser Zeit meldet der Browser nichts. Angaben in Minuten seit Mitternacht
/// (Ortszeit). Liegt `to` vor `from`, reichen die Ruhezeiten über Mitternacht.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(default)]
pub struct QuietHours {
    pub enabled: bool,
    pub from: u16,
    pub to: u16,
}

impl Default for QuietHours {
    fn default() -> Self {
        Self {
            enabled: false,
            from: 22 * 60,
            to: 7 * 60,
        }
    }
}

impl QuietHours {
    /// Ist um diese Uhrzeit (Minuten seit Mitternacht) Ruhe?
    pub fn contains(&self, minute: u16) -> bool {
        if !self.enabled || self.from == self.to {
            return false;
        }
        let minute = minute % DAY;
        if self.from < self.to {
            (self.from..self.to).contains(&minute)
        } else {
            minute >= self.from || minute < self.to
        }
    }

    /// Beide Zeiten liegen innerhalb eines Tages.
    pub fn is_valid(&self) -> bool {
        self.from < DAY && self.to < DAY
    }

    /// `22:00`
    pub fn format(minute: u16) -> String {
        format!("{:02}:{:02}", (minute % DAY) / 60, minute % 60)
    }

    /// Liest `HH:MM`. Ungültiges gibt `None`.
    pub fn parse(text: &str) -> Option<u16> {
        let (h, m) = text.trim().split_once(':')?;
        let (h, m): (u16, u16) = (h.parse().ok()?, m.parse().ok()?);
        (h < 24 && m < 60).then_some(h * 60 + m)
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
struct Rule {
    reason: NotifyReason,
    #[serde(default)]
    channels: Channels,
}

/// Einstellungen je Grund und Kanal, dazu die Ruhezeiten.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct NotifySettings {
    /// Nur abweichende Gründe; fehlt einer, gilt [`NotifySettings::default_channels`].
    #[serde(default)]
    rules: Vec<Rule>,
    #[serde(default)]
    pub quiet: QuietHours,
}

impl NotifySettings {
    /// Standard: alles im Browser an, E-Mail und Slack aus.
    pub fn default_channels() -> Channels {
        Channels {
            browser: true,
            email: false,
            slack: false,
        }
    }

    pub fn channels(&self, reason: NotifyReason) -> Channels {
        self.rules
            .iter()
            .find(|r| r.reason == reason)
            .map_or_else(Self::default_channels, |r| r.channels)
    }

    pub fn enabled(&self, reason: NotifyReason, channel: Channel) -> bool {
        self.channels(reason).get(channel)
    }

    /// Schaltet einen Kanal für einen Grund. `false`, wenn sich nichts ändert.
    pub fn set(&mut self, reason: NotifyReason, channel: Channel, on: bool) -> bool {
        let mut channels = self.channels(reason);
        if channels.get(channel) == on {
            return false;
        }
        channels.set(channel, on);
        match self.rules.iter_mut().find(|r| r.reason == reason) {
            Some(rule) => rule.channels = channels,
            None => self.rules.push(Rule { reason, channels }),
        }
        true
    }

    /// Setzt die Ruhezeiten. `false`, wenn sie ungültig sind (ab 24:00) oder sich nichts ändert.
    pub fn set_quiet(&mut self, quiet: QuietHours) -> bool {
        if !quiet.is_valid() || self.quiet == quiet {
            return false;
        }
        self.quiet = quiet;
        true
    }
}

/// Ein Ereignis, über das man benachrichtigt werden kann.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Event {
    pub reason: NotifyReason,
    pub title: String,
    pub text: String,
    /// Ziel als Adresse der App, z. B. `#/review/87`.
    pub route: String,
}

/// Ein Ereignis auf einem Kanal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Delivery {
    pub channel: Channel,
    pub event: Event,
}

/// Welche Ereignisse über welche Kanäle gehen. `minute` ist die Ortszeit in Minuten seit
/// Mitternacht; in den Ruhezeiten geht nichts raus.
pub fn deliveries(events: &[Event], settings: &NotifySettings, minute: u16) -> Vec<Delivery> {
    if settings.quiet.contains(minute) {
        return Vec::new();
    }
    events
        .iter()
        .flat_map(|event| {
            let channels = settings.channels(event.reason);
            Channel::ALL
                .into_iter()
                .filter(move |c| channels.get(*c))
                .map(|channel| Delivery {
                    channel,
                    event: event.clone(),
                })
        })
        .collect()
}

/// Steht `@Name` oder `@Kürzel` der Person im Text? Groß- und Kleinschreibung egal.
pub fn mentions(text: &str, who: &Person) -> bool {
    text.split('@').skip(1).any(|rest| {
        let word: String = rest
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
            .collect();
        !word.is_empty()
            && (word.eq_ignore_ascii_case(&who.name) || word.eq_ignore_ascii_case(&who.initials))
    })
}

fn route_of(item: &Item) -> String {
    match &item.pr {
        Some(pr) => format!("#/review/{}", pr.number),
        None => "#/".into(),
    }
}

/// Der Eintrag hat diesen Grund neu bekommen (war vorher nicht da, anders oder erledigt).
fn fresh(before: Option<&Item>, after: &Item, reason: ReasonKind) -> bool {
    after.reason == reason && !after.read && !before.is_some_and(|b| b.reason == reason && !b.read)
}

fn run_of<'a>(ws: &'a Workspace, key: &str) -> Option<&'a AgentRun> {
    ws.runs.iter().find(|r| r.ticket_key == key)
}

/// Die Ereignisse, die zwischen zwei Ständen des Workspace entstanden sind.
///
/// Ohne Vorher gibt es keinen Vergleich – der erste Stand beim Laden meldet also nichts.
pub fn events(before: &Workspace, after: &Workspace) -> Vec<Event> {
    let me = Person::me();
    let mut out = Vec::new();

    for item in &after.items {
        let old = before.item(&item.id);

        if fresh(old, item, ReasonKind::ReviewRequested) {
            out.push(Event {
                reason: NotifyReason::ReviewRequested,
                title: format!("Review angefragt: {}", item.title),
                text: format!("{} · {}", item.key, item.reason_text),
                route: route_of(item),
            });
        }
        if fresh(old, item, ReasonKind::Assigned) {
            out.push(Event {
                reason: NotifyReason::Assigned,
                title: format!("Ticket zugewiesen: {}", item.key),
                text: item.title.clone(),
                route: route_of(item),
            });
        }

        let seen = old.map_or(0, |o| o.comments.len());
        for comment in item.comments.iter().skip(seen) {
            if !comment.own && !comment.author.is_agent() && mentions(&comment.text, &me) {
                out.push(Event {
                    reason: NotifyReason::Mention,
                    title: format!("{} hat dich erwähnt", comment.author.name),
                    text: format!("{}: {}", item.key, comment.text),
                    route: route_of(item),
                });
            }
        }

        let checks_failed = |i: &Item| i.pr.as_ref().is_some_and(|p| p.checks == Checks::Failed);
        if item.scope == Scope::Mine && checks_failed(item) && !old.is_some_and(checks_failed) {
            let number = item.pr.as_ref().map_or(0, |p| p.number);
            out.push(Event {
                reason: NotifyReason::ChecksFailed,
                title: format!("Checks fehlgeschlagen: #{number}"),
                text: item.title.clone(),
                route: format!("#/review/{number}"),
            });
        }
    }

    for run in &after.runs {
        let old = run_of(before, &run.ticket_key);
        if run.is_awaiting() && !old.is_some_and(AgentRun::is_awaiting) {
            out.push(Event {
                reason: NotifyReason::AgentWaiting,
                title: format!("Agent wartet: {}", run.ticket_key),
                text: "Der Plan wartet auf deine Freigabe".into(),
                route: format!("#/agent/{}", run.ticket_key),
            });
        }
        if run.is_asking() && !old.is_some_and(AgentRun::is_asking) {
            out.push(Event {
                reason: NotifyReason::AgentWaiting,
                title: format!("Agent fragt: {}", run.ticket_key),
                text: run
                    .question
                    .as_ref()
                    .map_or_else(|| "Der Agent hat eine Rückfrage".into(), |q| q.text.clone()),
                route: format!("#/agent/{}", run.ticket_key),
            });
        }
        if run.is_done() && !old.is_some_and(AgentRun::is_done) {
            let (text, route) = match run.pr_number {
                Some(n) => (
                    format!(
                        "{} · Pull Request #{n} wartet auf dein Review",
                        run.ticket_title
                    ),
                    format!("#/review/{n}"),
                ),
                None => (
                    run.ticket_title.clone(),
                    format!("#/agent/{}", run.ticket_key),
                ),
            };
            out.push(Event {
                reason: NotifyReason::AgentFinished,
                title: format!("Agent fertig: {}", run.ticket_key),
                text,
                route,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Comment, PrRef};
    use crate::{AgentOptions, demo};

    fn ws() -> Workspace {
        demo::workspace()
    }

    fn only(events: Vec<Event>) -> Event {
        assert_eq!(events.len(), 1, "{events:?}");
        events.into_iter().next().unwrap()
    }

    #[test]
    fn unchanged_workspace_reports_nothing() {
        let w = ws();
        assert!(events(&w, &w).is_empty());
    }

    #[test]
    fn new_review_request_is_reported_once() {
        let before = ws();
        let mut after = before.clone();
        let mut item = after.item("pr87").unwrap().clone();
        item.id = "pr99".into();
        item.key = "#99".into();
        item.title = "Neuer Button".into();
        item.pr = Some(PrRef {
            number: 99,
            draft: false,
            checks: Checks::Passed,
        });
        after.items.push(item);
        let e = only(events(&before, &after));
        assert_eq!(e.reason, NotifyReason::ReviewRequested);
        assert_eq!(e.route, "#/review/99");
        assert!(e.title.contains("Neuer Button"));
        // Danach nicht noch einmal.
        assert!(events(&after, &after).is_empty());
    }

    #[test]
    fn done_item_that_returns_counts_again() {
        let mut before = ws();
        before.mark_done("pr87");
        let mut after = before.clone();
        after
            .items
            .iter_mut()
            .find(|i| i.id == "pr87")
            .unwrap()
            .read = false;
        assert_eq!(
            only(events(&before, &after)).reason,
            NotifyReason::ReviewRequested
        );
    }

    #[test]
    fn assignment_is_reported() {
        let before = ws();
        let mut after = before.clone();
        let item = after.items.iter_mut().find(|i| i.id == "bau142").unwrap();
        item.reason = ReasonKind::Assigned;
        let e = only(events(&before, &after));
        assert_eq!(e.reason, NotifyReason::Assigned);
        assert!(e.title.contains("BAU-142"));
        assert_eq!(e.route, "#/");
    }

    #[test]
    fn mention_needs_a_new_foreign_comment_with_my_name() {
        let before = ws();
        let mut after = before.clone();
        let lena = Person::new("Lena", "LK");
        fn comments(w: &mut Workspace) -> &mut Vec<Comment> {
            &mut w
                .items
                .iter_mut()
                .find(|i| i.id == "bau142")
                .unwrap()
                .comments
        }
        comments(&mut after).push(Comment::new(lena.clone(), "gerade eben", "Ohne Namen."));
        assert!(events(&before, &after).is_empty());
        comments(&mut after).push(Comment::new(lena, "gerade eben", "Was meinst du, @du?"));
        let e = only(events(&before, &after));
        assert_eq!(e.reason, NotifyReason::Mention);
        assert!(e.title.contains("Lena"));
        // Eigene Kommentare und alte Kommentare melden nichts.
        let mut own = before.clone();
        own.reply("bau142", "@Du bin das selbst");
        assert!(events(&before, &own).is_empty());
    }

    #[test]
    fn mention_matching() {
        let me = Person::me();
        assert!(mentions("Hi @Du!", &me));
        assert!(mentions("@as bitte prüfen", &me));
        assert!(mentions("a@b.de und @DU", &me));
        assert!(!mentions("Mail an a@b.de", &me));
        assert!(!mentions("@Dunkel", &me));
        assert!(!mentions("@", &me));
        assert!(!mentions("ohne", &me));
    }

    #[test]
    fn checks_turning_red_on_my_pr() {
        let mut before = ws();
        let (id, number) = {
            let i = before
                .items
                .iter_mut()
                .find(|i| i.pr.is_some() && i.scope == Scope::Mine)
                .expect("ein eigener Pull Request in den Beispieldaten");
            i.pr.as_mut().unwrap().checks = Checks::Running;
            (i.id.clone(), i.pr.as_ref().unwrap().number)
        };
        let mut after = before.clone();
        after
            .items
            .iter_mut()
            .find(|i| i.id == id)
            .unwrap()
            .pr
            .as_mut()
            .unwrap()
            .checks = Checks::Failed;
        let e = only(events(&before, &after));
        assert_eq!(e.reason, NotifyReason::ChecksFailed);
        assert_eq!(e.route, format!("#/review/{number}"));
        // Schon rot gewesen: keine zweite Meldung.
        assert!(events(&after, &after).is_empty());
    }

    #[test]
    fn checks_of_team_prs_are_not_mine() {
        let before = ws();
        let mut after = before.clone();
        if let Some(i) = after
            .items
            .iter_mut()
            .find(|i| i.pr.is_some() && i.scope == Scope::Team)
        {
            i.pr.as_mut().unwrap().checks = Checks::Failed;
            assert!(
                events(&before, &after)
                    .iter()
                    .all(|e| e.reason != NotifyReason::ChecksFailed)
            );
        }
    }

    #[test]
    fn agent_question_is_reported_once() {
        let mut before = ws();
        let direct = AgentOptions {
            plan_first: false,
            ..AgentOptions::default()
        };
        before.start_agent("BAU-142", direct).unwrap();
        let mut after = before.clone();
        let mut found = None;
        for _ in 0..crate::agent::STEPS {
            let prev = after.clone();
            after.advance_agents();
            let list = events(&prev, &after);
            if !list.is_empty() {
                found = Some(only(list));
                break;
            }
        }
        let e = found.expect("Rückfrage wird gemeldet");
        assert_eq!(e.reason, NotifyReason::AgentWaiting);
        assert_eq!(e.route, "#/agent/BAU-142");
        assert!(e.text.contains("deprecated"), "{e:?}");
        // Solange sie offen ist, kommt sie nicht noch einmal.
        assert!(events(&after, &after.clone()).is_empty());
    }

    #[test]
    fn agent_waiting_and_finished() {
        let before = ws();
        let key = before
            .items
            .iter()
            // Ohne Rückfrage in der Demo, damit der Lauf durchläuft.
            .find(|i| i.is_ticket() && i.pr.is_none() && demo::agent_question(&i.key).is_none())
            .unwrap()
            .key
            .clone();

        let mut waiting = before.clone();
        waiting.start_agent(&key, AgentOptions::default()).unwrap();
        let e = only(events(&before, &waiting));
        assert_eq!(e.reason, NotifyReason::AgentWaiting);
        assert_eq!(e.route, format!("#/agent/{key}"));

        let mut running = waiting.clone();
        assert!(running.approve_plan(&key));
        assert!(
            events(&waiting, &running).is_empty(),
            "Freigabe meldet nichts"
        );

        let mut after = running.clone();
        for _ in 0..crate::agent::STEPS - 1 {
            after.advance_agents();
            assert!(events(&running, &after).is_empty(), "unterwegs ist Ruhe");
        }
        let prev = after.clone();
        after.advance_agents();
        let e = only(events(&prev, &after));
        assert_eq!(e.reason, NotifyReason::AgentFinished);
        assert!(e.route.starts_with("#/review/"), "{}", e.route);
        assert!(e.text.contains("Review"));
    }

    #[test]
    fn agent_without_plan_only_reports_when_finished() {
        let before = ws();
        let key = before
            .items
            .iter()
            .find(|i| i.is_ticket() && i.pr.is_none())
            .unwrap()
            .key
            .clone();
        let mut after = before.clone();
        let direct = AgentOptions {
            plan_first: false,
            ..AgentOptions::default()
        };
        after.start_agent(&key, direct).unwrap();
        assert!(events(&before, &after).is_empty());
    }

    #[test]
    fn defaults_are_browser_only() {
        let s = NotifySettings::default();
        for reason in NotifyReason::ALL {
            assert!(s.enabled(reason, Channel::Browser));
            assert!(!s.enabled(reason, Channel::Email));
            assert!(!s.enabled(reason, Channel::Slack));
        }
        assert!(!s.quiet.enabled);
    }

    #[test]
    fn channels_are_set_per_reason() {
        let mut s = NotifySettings::default();
        assert!(s.set(NotifyReason::Mention, Channel::Email, true));
        assert!(
            !s.set(NotifyReason::Mention, Channel::Email, true),
            "nichts ändert sich"
        );
        assert!(s.set(NotifyReason::Mention, Channel::Browser, false));
        assert!(s.enabled(NotifyReason::Mention, Channel::Email));
        assert!(!s.enabled(NotifyReason::Mention, Channel::Browser));
        assert!(
            s.enabled(NotifyReason::Assigned, Channel::Browser),
            "andere bleiben"
        );
    }

    #[test]
    fn quiet_hours_over_midnight_and_within_a_day() {
        let night = QuietHours {
            enabled: true,
            from: 22 * 60,
            to: 7 * 60,
        };
        assert!(night.contains(23 * 60) && night.contains(0) && night.contains(6 * 60 + 59));
        assert!(
            !night.contains(7 * 60) && !night.contains(12 * 60) && !night.contains(21 * 60 + 59)
        );
        assert!(night.contains(22 * 60));
        let lunch = QuietHours {
            enabled: true,
            from: 12 * 60,
            to: 13 * 60,
        };
        assert!(
            lunch.contains(12 * 60 + 30) && !lunch.contains(13 * 60) && !lunch.contains(11 * 60)
        );
        assert!(
            !QuietHours {
                enabled: false,
                ..night
            }
            .contains(0)
        );
        assert!(
            !QuietHours {
                enabled: true,
                from: 60,
                to: 60
            }
            .contains(60),
            "gleiche Zeit: keine Ruhe"
        );
    }

    #[test]
    fn quiet_times_format_and_parse() {
        assert_eq!(QuietHours::format(22 * 60 + 5), "22:05");
        assert_eq!(QuietHours::parse("07:30"), Some(450));
        assert_eq!(QuietHours::parse(" 7:05 "), Some(425));
        for bad in ["", "24:00", "12:60", "abc", "12", "-1:00", "12:xx"] {
            assert_eq!(QuietHours::parse(bad), None, "{bad}");
        }
        for m in [0, 59, 60, 1439] {
            assert_eq!(QuietHours::parse(&QuietHours::format(m)), Some(m));
        }
    }

    #[test]
    fn deliveries_follow_settings_and_quiet_hours() {
        let event = |reason| Event {
            reason,
            title: "t".into(),
            text: "x".into(),
            route: "#/".into(),
        };
        let evs = [event(NotifyReason::Mention), event(NotifyReason::Assigned)];
        let mut s = NotifySettings::default();
        s.set(NotifyReason::Mention, Channel::Slack, true);
        s.set(NotifyReason::Assigned, Channel::Browser, false);
        let got: Vec<_> = deliveries(&evs, &s, 12 * 60)
            .into_iter()
            .map(|d| (d.event.reason, d.channel))
            .collect();
        assert_eq!(
            got,
            [
                (NotifyReason::Mention, Channel::Browser),
                (NotifyReason::Mention, Channel::Slack)
            ]
        );
        s.quiet = QuietHours {
            enabled: true,
            from: 22 * 60,
            to: 7 * 60,
        };
        assert!(deliveries(&evs, &s, 23 * 60).is_empty());
        assert_eq!(deliveries(&evs, &s, 8 * 60).len(), 2);
    }

    #[test]
    fn ids_roundtrip() {
        for r in NotifyReason::ALL {
            assert_eq!(NotifyReason::from_id(r.id()), Some(r));
        }
        for c in Channel::ALL {
            assert_eq!(Channel::from_id(c.id()), Some(c));
        }
        assert_eq!(NotifyReason::from_id("?"), None);
        assert!(Channel::Email.via_backend() && !Channel::Browser.via_backend());
    }

    #[test]
    fn settings_json_is_stable_and_old_data_loads() {
        let mut s = NotifySettings::default();
        s.set(NotifyReason::Mention, Channel::Email, true);
        let text = serde_json::to_string(&s).unwrap();
        assert_eq!(serde_json::from_str::<NotifySettings>(&text).unwrap(), s);
        assert_eq!(
            serde_json::from_str::<NotifySettings>("{}").unwrap(),
            NotifySettings::default()
        );
        assert!(
            !text.contains('@') && !text.contains("http"),
            "keine Adressen"
        );
    }
}
