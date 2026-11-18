//! The header of the run pane.

use cockpit_core::agent::STEPS;
use feature_ui::*;
use leptos::prelude::*;

use super::context::AgentCtx;

/// The state of the run in one phrase and its colour.
pub(super) fn status_of(ctx: AgentCtx) -> (String, StatusTone) {
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
        (
            match closure {
                cockpit_core::Closure::Merged => t!(AgentHeaderClosedMerged),
                cockpit_core::Closure::Closed => t!(AgentHeaderClosedClosed),
            },
            StatusTone::Gray,
        )
    } else if done.get() {
        (t!(AgentHeaderDone), StatusTone::Green)
    } else if diagnosed.get() {
        (t!(AgentHeaderDecision), StatusTone::Orange)
    } else if awaiting.get() {
        (t!(AgentHeaderApproval), StatusTone::Orange)
    } else if asking.get() {
        (t!(AgentHeaderAnswer), StatusTone::Orange)
    } else if paused.get() {
        (t!(AgentHeaderPaused), StatusTone::Gray)
    } else if stopped.get() {
        (t!(AgentHeaderHalted), StatusTone::Red)
    } else if running.get() {
        (t!(AgentHeaderWorking), StatusTone::Blue)
    } else {
        (t!(AgentHeaderReady), StatusTone::Gray)
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
                        Some(r) => format!("{} · {}", super::labels::run_label(&r), r.ticket_title),
                        None => item.get().map(|i| i.title).unwrap_or_default(),
                    }
                }}
            </h1>
            <Progress
                value=Signal::derive(move || run.get().map_or(0, |r| r.completed))
                max=STEPS
                label=t!(AgentHeaderProgress)
            />
        </PaneHeader>
    }
}
