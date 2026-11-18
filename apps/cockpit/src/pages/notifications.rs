//! Settings: notifications per reason and channel, quiet hours and the permission of the
//! browser. Saved in the workspace through actions (`SetNotifyChannel`,
//! `SetQuietHours`). What is reported when is in `cockpit_core::notify`.
//!
//! Email and Slack are only prepared: the selection is saved, sending will be done
//! later through the backend. Addresses and webhook URLs do not exist in the browser.

use cockpit_core::{Action, Channel, NotifyEvent, NotifyReason, QuietHours};
use feature_ui::*;
use leptos::prelude::*;

use crate::{
    notify::{self as browser, Permission},
    store::use_store,
    view,
};

fn reason_label(reason: NotifyReason) -> String {
    reason.msg().render(lang())
}

fn reason_hint(reason: NotifyReason) -> String {
    reason.hint_msg().render(lang())
}

fn channel_label(channel: Channel) -> String {
    channel.msg().render(lang())
}

fn permission_label(permission: Permission) -> String {
    match permission {
        Permission::Unsupported => t!(NotificationsPermissionUnsupported),
        Permission::Ask => t!(NotificationsPermissionAsk),
        Permission::Granted => t!(NotificationsPermissionGranted),
        Permission::Denied => t!(NotificationsPermissionDenied),
    }
}

/// A switch for a reason and a channel. Changes the workspace only through the action.
#[component]
fn ChannelToggle(reason: NotifyReason, channel: Channel) -> impl IntoView {
    let store = use_store();
    let from_ws = move || store.ws.with(|w| w.notifications.enabled(reason, channel));
    let on = RwSignal::new(
        store
            .ws
            .with_untracked(|w| w.notifications.enabled(reason, channel)),
    );
    // Switch → action.
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
    // Workspace → switch (e.g. after resetting).
    Effect::new(move |_| {
        let now = from_ws();
        if on.get_untracked() != now {
            on.set(now);
        }
    });
    let name = t!(
        NotificationsVia,
        reason = reason_label(reason),
        channel = channel_label(channel)
    );
    view! { <Toggle checked=on aria_label=name /> }
}

/// Quiet hours: on/off and from–to. Invalid times change nothing and are marked red.
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
                header=t!(NotificationsQuiet)
                footer=t!(NotificationsQuietFooter)
            >
                <ListRow title=t!(NotificationsQuietUse) toggle_row=true>
                    <Toggle checked=enabled />
                </ListRow>
            </ListSection>
        </List>
        <div class="ck-quiet">
            <TextField
                label=t!(NotificationsFrom)
                value=from
                input_type="time"
                error=invalid
                disabled=Signal::derive(move || !enabled.get())
            />
            <TextField
                label=t!(NotificationsTo)
                value=to
                input_type="time"
                error=invalid
                disabled=Signal::derive(move || !enabled.get())
                hint=t!(NotificationsTimeHint)
            />
        </div>
    }
}

#[component]
pub fn NotificationsPage() -> impl IntoView {
    let store = use_store();
    // The permission may have changed in the browser settings.
    store.permission.set(browser::permission());

    let ask = move |_| browser::request(move |state| store.permission.set(state));
    let test = move |_| {
        store.announce(
            NotifyEvent {
                reason: NotifyReason::Assigned,
                title: cockpit_i18n::msg!(NotificationsTestTitle),
                text: cockpit_i18n::msg!(NotificationsTestText),
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

            <Pane label=t!(NotificationsTitle)>
                <PaneHeader>
                    <h1 class="t-large-title t-bold">{t!(NotificationsTitle)}</h1>
                    <p class="t-subheadline t-secondary">
                        {t!(NotificationsSubtitle)}
                    </p>
                </PaneHeader>
                <PaneBody>
                    <div class="ck-settings">
                        <List>
                            <ListSection
                                header=t!(NotificationsBrowser)
                                footer=t!(NotificationsBrowserFooter)
                            >
                                <ListRow title=t!(NotificationsSystem) wrap=true>
                                    <Status tone=tone>{move || permission_label(store.permission.get())}</Status>
                                </ListRow>
                                <ListRow title=t!(NotificationsPermission) wrap=true>
                                    <HStack>
                                        <Button
                                            variant=ButtonStyle::GlassProminent
                                            disabled=Signal::derive(move || store.permission.get() != Permission::Ask)
                                            on_click=ask
                                        >
                                            {t!(NotificationsAsk)}
                                        </Button>
                                        <Button on_click=test>{t!(NotificationsTestTitle)}</Button>
                                    </HStack>
                                </ListRow>
                            </ListSection>
                        </List>

                        <p class="t-footnote t-secondary" style="padding-inline: 4px">
                            {t!(NotificationsBackendNote)}
                        </p>
                        {NotifyReason::ALL
                            .into_iter()
                            .map(|reason| {
                                view! {
                                    <List>
                                        <ListSection header=reason_label(reason) footer=reason_hint(reason)>
                                            {Channel::ALL
                                                .into_iter()
                                                .map(|channel| {
                                                    if channel.via_backend() {
                                                        view! {
                                                            <ListRow
                                                                title=channel_label(channel)
                                                                subtitle=t!(NotificationsBackendLater)
                                                                toggle_row=true
                                                            >
                                                                <ChannelToggle reason=reason channel=channel />
                                                            </ListRow>
                                                        }
                                                            .into_any()
                                                    } else {
                                                        view! {
                                                            <ListRow title=channel_label(channel) toggle_row=true>
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
