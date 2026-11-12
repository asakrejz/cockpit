//! Einstellungen: Benachrichtigungen je Grund und Kanal, Ruhezeiten und die Erlaubnis des
//! Browsers. Gespeichert wird im Workspace über Aktionen (`SetNotifyChannel`,
//! `SetQuietHours`). Was wann gemeldet wird, steht in `cockpit_core::notify`.
//!
//! E-Mail und Slack sind nur vorbereitet: Die Auswahl wird gespeichert, versendet wird
//! später über das Backend. Adressen und Webhook-URLs gibt es im Browser nicht.

use cockpit_core::{Action, Channel, NotifyEvent, NotifyReason, QuietHours};
use feature_ui::*;
use leptos::prelude::*;

use crate::{
    notify::{self as browser, Permission},
    sidebar::AppSidebar,
    store::use_store,
    view,
};

/// Ein Schalter für einen Grund und einen Kanal. Ändert den Workspace nur über die Aktion.
#[component]
fn ChannelToggle(reason: NotifyReason, channel: Channel) -> impl IntoView {
    let store = use_store();
    let from_ws = move || store.ws.with(|w| w.notifications.enabled(reason, channel));
    let on = RwSignal::new(
        store
            .ws
            .with_untracked(|w| w.notifications.enabled(reason, channel)),
    );
    // Schalter → Aktion.
    Effect::new(move |_| {
        let want = on.get();
        if want
            != store
                .ws
                .with_untracked(|w| w.notifications.enabled(reason, channel))
        {
            let _ = store.dispatch(Action::SetNotifyChannel {
                reason,
                channel,
                on: want,
            });
        }
    });
    // Workspace → Schalter (z. B. nach dem Zurücksetzen).
    Effect::new(move |_| {
        let now = from_ws();
        if on.get_untracked() != now {
            on.set(now);
        }
    });
    let name = format!("{} über {}", reason.label(), channel.label());
    view! { <Toggle checked=on aria_label=name /> }
}

/// Ruhezeiten: an/aus und von–bis. Ungültige Zeiten ändern nichts und werden rot markiert.
#[component]
fn Quiet() -> impl IntoView {
    let store = use_store();
    let quiet = store.ws.with_untracked(|w| w.notifications.quiet);
    let enabled = RwSignal::new(quiet.enabled);
    let from = RwSignal::new(QuietHours::format(quiet.from));
    let to = RwSignal::new(QuietHours::format(quiet.to));

    let parsed = move || {
        Some(QuietHours {
            enabled: enabled.get(),
            from: QuietHours::parse(&from.get())?,
            to: QuietHours::parse(&to.get())?,
        })
    };
    let invalid = Signal::derive(move || parsed().is_none());

    Effect::new(move |_| {
        if let Some(wanted) = parsed()
            && wanted != store.ws.with_untracked(|w| w.notifications.quiet)
        {
            let _ = store.dispatch(Action::SetQuietHours { quiet: wanted });
        }
    });
    Effect::new(move |_| {
        let now = store.ws.with(|w| w.notifications.quiet);
        if enabled.get_untracked() != now.enabled {
            enabled.set(now.enabled);
        }
        if QuietHours::parse(&from.get_untracked()) != Some(now.from) {
            from.set(QuietHours::format(now.from));
        }
        if QuietHours::parse(&to.get_untracked()) != Some(now.to) {
            to.set(QuietHours::format(now.to));
        }
    });

    view! {
        <List>
            <ListSection
                header="Ruhezeiten"
                footer="In dieser Zeit meldet der Browser nichts. Die Zeiten dürfen über Mitternacht reichen (22:00 bis 07:00)."
            >
                <ListRow title="Ruhezeiten verwenden" toggle_row=true>
                    <Toggle checked=enabled />
                </ListRow>
            </ListSection>
        </List>
        <div class="ck-quiet">
            <TextField
                label="Von"
                value=from
                input_type="time"
                error=invalid
                disabled=Signal::derive(move || !enabled.get())
            />
            <TextField
                label="Bis"
                value=to
                input_type="time"
                error=invalid
                disabled=Signal::derive(move || !enabled.get())
                hint="Format 24 Stunden, z. B. 07:00."
            />
        </div>
    }
}

#[component]
pub fn NotificationsPage() -> impl IntoView {
    let store = use_store();
    // Die Erlaubnis kann sich in den Browser-Einstellungen geändert haben.
    store.permission.set(browser::permission());

    let ask = move |_| browser::request(move |state| store.permission.set(state));
    let test = move |_| {
        store.announce(
            NotifyEvent {
                reason: NotifyReason::Assigned,
                title: "Test-Benachrichtigung".into(),
                text: "So sieht eine Meldung von Cockpit aus. Ein Klick öffnet diese Seite.".into(),
                route: browser::settings_route(),
            },
            true,
        );
    };
    let tone = Signal::derive(move || match store.permission.get() {
        Permission::Granted => StatusTone::Green,
        Permission::Denied => StatusTone::Red,
        Permission::Ask => StatusTone::Orange,
        Permission::Unsupported => StatusTone::Gray,
    });

    view! {
        <SplitView>
            <AppSidebar>{""}</AppSidebar>

            <Pane label="Benachrichtigungen">
                <PaneHeader>
                    <h1 class="t-large-title t-bold">"Benachrichtigungen"</h1>
                    <p class="t-subheadline t-secondary">
                        "Wähle je Anlass, worüber du Bescheid bekommst."
                    </p>
                </PaneHeader>
                <PaneBody>
                    <div class="ck-settings">
                        <List>
                            <ListSection
                                header="Browser"
                                footer="Cockpit fragt nur, wenn du auf den Knopf drückst. Ist der Tab im Vordergrund, erscheint ein Hinweis in der App; sonst eine Systemmeldung, deren Klick das Ziel öffnet."
                            >
                                <ListRow title="Systemmeldungen" wrap=true>
                                    <Status tone=tone>{move || store.permission.get().label()}</Status>
                                </ListRow>
                                <ListRow title="Erlaubnis" wrap=true>
                                    <HStack>
                                        <Button
                                            variant=ButtonStyle::GlassProminent
                                            disabled=Signal::derive(move || store.permission.get() != Permission::Ask)
                                            on_click=ask
                                        >
                                            "Erlaubnis anfragen"
                                        </Button>
                                        <Button on_click=test>"Test-Benachrichtigung"</Button>
                                    </HStack>
                                </ListRow>
                            </ListSection>
                        </List>

                        <p class="t-footnote t-secondary" style="padding-inline: 4px">
                            "E-Mail und Slack werden über das Backend versendet (kommt später). Die Auswahl wird schon gespeichert; Adressen und Webhook-URLs gehören nicht in den Browser und werden dort nicht abgelegt."
                        </p>
                        {NotifyReason::ALL
                            .into_iter()
                            .map(|reason| {
                                view! {
                                    <List>
                                        <ListSection header=reason.label() footer=reason.hint()>
                                            {Channel::ALL
                                                .into_iter()
                                                .map(|channel| {
                                                    if channel.via_backend() {
                                                        view! {
                                                            <ListRow
                                                                title=channel.label()
                                                                subtitle="Wird über das Backend versendet (kommt später)"
                                                                toggle_row=true
                                                            >
                                                                <ChannelToggle reason=reason channel=channel />
                                                            </ListRow>
                                                        }
                                                            .into_any()
                                                    } else {
                                                        view! {
                                                            <ListRow title=channel.label() toggle_row=true>
                                                                <ChannelToggle reason=reason channel=channel />
                                                            </ListRow>
                                                        }
                                                            .into_any()
                                                    }
                                                })
                                                .collect_view()}
                                        </ListSection>
                                    </List>
                                }
                            })
                            .collect_view()}

                        <Quiet />
                    </div>
                </PaneBody>
            </Pane>
        </SplitView>
    }
}
