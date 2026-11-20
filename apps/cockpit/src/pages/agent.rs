//! An agent implements a ticket: job, course in five steps and activity.

use feature_ui::*;
use leptos::prelude::*;

use crate::route::Route;

mod actions;
mod activity;
mod branch;
mod cleanup;
mod context;
mod controls;
mod decisions;
mod header;
mod job;
mod labels;
mod progress;
mod pull_request;
mod repo_choice;
mod repos;
mod sidebar;
mod texts;

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
    // Finished runs whose pull request is merged or closed stay out of the list until asked for.
    let show_closed = RwSignal::new(false);

    view! {
        <SplitView
            layout=SplitLayout::DetailWide
            level=level
            back_label=t!(AgentRuns)
            back_href=Route::Agent(None).hash()
        >
            <PickerPane label=t!(AgentRuns) title=t!(AgentRuns)>
                {sidebar::closed_switch(ctx, show_closed)}
                <ul class="ui-sidebar__nav">{move || sidebar::sidebar_runs(ctx, show_closed.get())}</ul>
                <div class="ck-cleanup">
                    <cleanup::CleanupButton />
                </div>
            </PickerPane>

            <Show
                when=move || run.get().is_some()
                fallback=|| {
                    view! {
                        <Pane label=t!(AgentJobLabel)>
                            <PaneBody>
                                <EmptyState
                                    title=t!(AgentEmptyTitle)
                                    text=t!(AgentEmptyText)
                                    icon=icons::SPARKLES
                                >
                                    <Button
                                        variant=ButtonStyle::GlassProminent
                                        size=ControlSize::Small
                                        href=crate::route::Route::TASKS_HASH
                                    >
                                        {t!(AgentToInbox)}
                                    </Button>
                                </EmptyState>
                            </PaneBody>
                        </Pane>
                    }
                }
            >
                <Pane label=t!(AgentJobLabel)>
                    {header::header(ctx)}

                    <PaneBody>
                        {job::job(ctx)}

                        {controls::controls(ctx)}
                        {move || controls::blocked_hint(ctx)}
                        <repos::RunRepos run=run />
                        <pull_request::PullRequestPanel run=run />
                        <branch::BranchPanel run=run />
                        {move || controls::ended_banner(ctx)}

                        {move || controls::problem_alert(ctx)}

                        {move || decisions::diagnosis(ctx)}

                        {move || decisions::stopped_card(ctx)}

                        {move || decisions::question(ctx)}

                        {decisions::steer_card(ctx)}

                        {move || decisions::plan_card(ctx)}

                        {move || progress::usage(ctx)}

                        <div class="ui-list__section">
                            <p class="ui-list__header">{t!(AgentCourse)}</p>
                            <Timeline>{progress::timeline_steps(ctx)}</Timeline>
                        </div>
                    </PaneBody>
                </Pane>

                {activity::pane(ctx)}
            </Show>
        </SplitView>
    }
}
