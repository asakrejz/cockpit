//! An agent implements a ticket: job, course in five steps and activity.

use feature_ui::*;
use leptos::prelude::*;

use crate::route::Route;

mod actions;
mod activity;
mod context;
mod controls;
mod decisions;
mod header;
mod job;
mod progress;
mod pull_request;
mod sidebar;

use context::AgentCtx;

#[component]
pub fn AgentPage(ticket: Option<String>) -> impl IntoView {
    // On a narrow screen the list of runs is the level without a ticket in the address.
    let level = if ticket.is_some() {
        StackLevel::Detail
    } else {
        StackLevel::List
    };
    let ctx = AgentCtx::new(ticket);
    let run = ctx.run;

    view! {
        <SplitView
            layout=SplitLayout::DetailWide
            level=level
            back_label="Agenten-Läufe"
            back_href=Route::Agent(None).hash()
        >
            <PickerPane label="Agenten-Läufe" title="Agenten-Läufe">
                <ul class="ui-sidebar__nav">{move || sidebar::sidebar_runs(ctx)}</ul>
            </PickerPane>

            <Show
                when=move || run.get().is_some()
                fallback=|| {
                    view! {
                        <Pane label="Agent-Auftrag">
                            <PaneBody>
                                <EmptyState
                                    title="Noch kein Agent-Lauf"
                                    text="Agenten setzen Jira-Tickets um. Wähle im Posteingang ein Ticket und beauftrage den Agenten dort; sein Lauf erscheint dann hier."
                                    icon=icons::SPARKLES
                                >
                                    <Button
                                        variant=ButtonStyle::GlassProminent
                                        size=ControlSize::Small
                                        href="#/"
                                    >
                                        "Zum Posteingang"
                                    </Button>
                                </EmptyState>
                            </PaneBody>
                        </Pane>
                    }
                }
            >
                <Pane label="Agent-Auftrag">
                    {header::header(ctx)}

                    <PaneBody>
                        {job::job(ctx)}

                        {controls::controls(ctx)}
                        {move || controls::blocked_hint(ctx)}
                        <pull_request::PullRequestPanel run=run />
                        {move || controls::ended_banner(ctx)}

                        {move || controls::problem_alert(ctx)}

                        {move || decisions::diagnosis(ctx)}

                        {move || decisions::stopped_card(ctx)}

                        {move || decisions::question(ctx)}

                        {decisions::steer_card(ctx)}

                        {move || decisions::plan_card(ctx)}

                        {move || progress::usage(ctx)}

                        <div class="ui-list__section">
                            <p class="ui-list__header">"Ablauf"</p>
                            <Timeline>{progress::timeline_steps(ctx)}</Timeline>
                        </div>
                    </PaneBody>
                </Pane>

                {activity::pane(ctx)}
            </Show>
        </SplitView>
    }
}
