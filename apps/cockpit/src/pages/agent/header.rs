//! The header of the run pane.

use cockpit_core::agent::STEPS;
use feature_ui::*;
use leptos::prelude::*;

use super::context::AgentCtx;

/// The state of the run in one phrase and its colour.
pub(super) fn status_of(ctx: AgentCtx) -> (&'static str, StatusTone) {
    let AgentCtx {
        running,
        awaiting,
        asking,
        paused,
        diagnosed,
        stopped,
        done,
        run,
        ..
    } = ctx;
    if let Some(closure) = run.with(|r| r.as_ref().and_then(|r| r.closure)) {
        (closure.label(), StatusTone::Gray)
    } else if done.get() {
        ("Fertig", StatusTone::Green)
    } else if diagnosed.get() {
        ("Wartet auf Entscheidung", StatusTone::Orange)
    } else if awaiting.get() {
        ("Wartet auf Freigabe", StatusTone::Orange)
    } else if asking.get() {
        ("Wartet auf Antwort", StatusTone::Orange)
    } else if paused.get() {
        ("Pausiert", StatusTone::Gray)
    } else if stopped.get() {
        ("Angehalten", StatusTone::Red)
    } else if running.get() {
        ("Agent arbeitet", StatusTone::Blue)
    } else {
        ("Bereit", StatusTone::Gray)
    }
}

/// Title, state and progress of the run.
pub(super) fn header(ctx: AgentCtx) -> impl IntoView {
    let AgentCtx {
        key,
        item,
        run,
        followup,
        ..
    } = ctx;
    view! {
        <PaneHeader>
            <div class="ui-hstack" style="--spacing: 10px">
                <Badge variant=BadgeVariant::Neutral>
                    {move || if followup.get() { "GitHub" } else { "Jira" }}
                </Badge>
                <span class="t-subheadline t-mono t-secondary">{key.get_value()}</span>
                <span class="ui-spacer"></span>
                <Status tone=Signal::derive(move || status_of(ctx).1)>{move || status_of(ctx).0}</Status>
            </div>
            <h1 class="t-title1 t-bold">
                {move || {
                    match run.get().filter(|r| r.is_followup()) {
                        Some(r) => format!("{} · {}", r.label(), r.ticket_title),
                        None => item.get().map(|i| i.title).unwrap_or_default(),
                    }
                }}
            </h1>
            <Progress
                value=Signal::derive(move || run.get().map_or(0, |r| r.completed))
                max=STEPS
                label="Fortschritt"
            />
        </PaneHeader>
    }
}
