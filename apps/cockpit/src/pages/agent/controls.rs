//! The buttons of the run and the notes that go with them.

use crate::route::Route;
use feature_ui::*;
use leptos::prelude::*;

use super::context::{AgentCtx, OptionSignals};

/// The buttons for the current state of the run.
pub(super) fn controls(ctx: AgentCtx) -> impl IntoView {
    let AgentCtx {
        run,
        idle,
        running,
        awaiting,
        paused,
        followup,
        working,
        violations,
        blocked,
        ..
    } = ctx;
    let OptionSignals { repo, .. } = ctx.form;
    let start = move |_| ctx.start();
    let cancel = move |_| ctx.cancel();
    let approve = move |_| ctx.approve();
    let reject = move |_| ctx.reject();
    let pause = move |_| ctx.pause();
    let resume = move |_| ctx.resume();
    let to_ticket = move |_| ctx.to_ticket();
    view! {
        <div class="ui-hstack ui-gap-10">
            <Show when=move || idle.get() && !followup.get()>
                <Button
                    variant=ButtonStyle::GlassProminent
                    size=ControlSize::Large
                    disabled=Signal::derive(move || blocked.get().is_some() || repo.get().is_empty())
                    on_click=start
                >
                    <Icon path=icons::SPARKLES />
                    {t!(AgentControlsStart)}
                </Button>
            </Show>
            <Show when=move || awaiting.get()>
                <Button
                    variant=ButtonStyle::GlassProminent
                    size=ControlSize::Large
                    disabled=Signal::derive(move || !violations.get().is_empty())
                    on_click=approve
                >
                    {t!(AgentControlsApprove)}
                </Button>
                <Button size=ControlSize::Large destructive=true on_click=reject>
                    {t!(AgentControlsReject)}
                </Button>
            </Show>
            <Show when=move || running.get()>
                <Button size=ControlSize::Large on_click=pause>
                    {t!(AgentControlsPause)}
                </Button>
            </Show>
            <Show when=move || paused.get()>
                <Button variant=ButtonStyle::GlassProminent size=ControlSize::Large on_click=resume>
                    {t!(AgentControlsResume)}
                </Button>
            </Show>
            <Show when=move || working.get()>
                <Button size=ControlSize::Large destructive=true on_click=cancel>
                    {t!(AgentControlsCancel)}
                </Button>
            </Show>
            {move || {
                let number = run.get().filter(|r| r.is_done() || r.is_followup())?.pr_key()?;
                Some(
                    view! {
                        <Button
                            variant=ButtonStyle::GlassProminent
                            size=ControlSize::Large
                            href=Route::Review(Some(number)).hash()
                        >
                            {move || if followup.get() { t!(AgentControlsToReview) } else { t!(AgentControlsCheckPr) }}
                        </Button>
                    },
                )
            }}
            <Button variant=ButtonStyle::Borderless size=ControlSize::Large on_click=to_ticket>
                {move || if followup.get() { t!(AgentControlsToEntry) } else { t!(AgentControlsToTicket) }}
            </Button>
        </div>
    }
}

/// Why the agent cannot start, with the link to the setup.
pub(super) fn blocked_hint(ctx: AgentCtx) -> Option<impl IntoView> {
    let AgentCtx {
        store,
        idle,
        blocked,
        ..
    } = ctx;
    blocked.get().filter(|_| idle.get()).map(|reason| {
        // The server says exactly what is missing and where to set it up.
        let exactly = store.health.with(|h| {
            h.as_ref()
                .map(|h| h.agent.missing.join(" "))
                .filter(|t| !t.is_empty())
        });
        view! {
            <p class="t-footnote t-secondary">
                {exactly.unwrap_or_else(|| reason.render(lang()))} " "
                <a class="ui-link" href=Route::Setup.hash()>{t!(AgentControlsToSetup)}</a>
            </p>
        }
    })
}

/// Why the last run ended without a result (error, timeout, restart).
pub(super) fn ended_banner(ctx: AgentCtx) -> Option<impl IntoView> {
    let AgentCtx { run, .. } = ctx;
    // Why the last run ended without a result (error, timeout, restart).
    run.get()
        .filter(|r| r.is_idle())
        .and_then(|r| r.ended)
        .map(|text| {
            view! {
                <StatusBanner
                    tone=BannerTone::Error
                    title=t!(AgentControlsEndedTitle)
                    text=text
                >
                    <Button size=ControlSize::Small href=Route::Setup.hash()>
                        {t!(AgentControlsCheckSetup)}
                    </Button>
                </StatusBanner>
            }
        })
}

/// Message of the last action that did not work.
pub(super) fn problem_alert(ctx: AgentCtx) -> Option<impl IntoView> {
    let AgentCtx { problem, .. } = ctx;
    problem.get().map(|text| {
        view! { <p class="t-footnote ck-problem" role="alert">{text}</p> }
    })
}
