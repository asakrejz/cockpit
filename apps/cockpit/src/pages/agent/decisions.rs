//! The cards that wait for a decision of the user: diagnosis, limit, question, hint and plan.

use crate::route::Route;
use feature_ui::*;
use leptos::prelude::*;

use super::context::AgentCtx;

/// The diagnosis of a failed check, waiting for a decision.
pub(super) fn diagnosis(ctx: AgentCtx) -> Option<impl IntoView> {
    let AgentCtx { run, .. } = ctx;
    let d = run.get().filter(|r| r.is_diagnosed())?.diagnosis?;
    let check_badge = t!(AgentDecisionsCheckBadge, name = d.check);
    Some(view! {
        <div class="ui-list__section" role="region" aria-label=t!(AgentDecisionsDiagnosisLabel)>
            <p class="ui-list__header">{t!(AgentDecisionsDiagnosisHeader)}</p>
            <SuggestionCard state=SuggestionState::Pending>
                <div class="ui-hstack" style="--spacing: 8px">
                    <Badge variant=BadgeVariant::Tint>{check_badge}</Badge>
                    {d
                        .location()
                        .map(|l| view! { <span class="t-footnote t-mono t-secondary">{l.render(lang())}</span> })}
                </div>
                <p class="t-headline">{d.cause_message().render(lang())}</p>
                <p class="t-footnote t-secondary">{t!(AgentDecisionsSuggestion, text = d.remedy_message().render(lang()))}</p>
                {d
                    .patch
                    .clone()
                    .map(|p| {
                        view! {
                            <Diff
                                rows=crate::view::diff_rows(&p)
                                label=t!(AgentDecisionsFixFor, name = p.name)
                            />
                        }
                    })}
                <p class="t-footnote t-secondary">
                    {t!(AgentDecisionsBranchUnchanged)}
                </p>
            </SuggestionCard>
        </div>
    })
}

/// A limit has halted the run: raise it and resume, or cancel.
pub(super) fn stopped_card(ctx: AgentCtx) -> Option<impl IntoView> {
    let AgentCtx { store, run, .. } = ctx;
    let cancel = move |_| ctx.cancel();
    let raise = move |extra: u64| ctx.raise(extra);
    // A limit has halted the run: raise it and resume, or cancel.
    let run = run.get().filter(|r| r.is_stopped())?;
    let reason = run.stop_reason()?;
    let (limits, month) = store
        .ws
        .with(|w| (w.limits.clone(), w.spent_month(store.now.get()).cost_cents));
    let detail = limits.describe(reason, &run, month);
    let options = reason.raise_options();
    Some(view! {
        <div class="ui-list__section" role="region" aria-label=t!(AgentDecisionsStoppedLabel)>
            <p class="ui-list__header">{t!(AgentDecisionsStoppedHeader)}</p>
            <SuggestionCard state=SuggestionState::Pending>
                <p class="t-headline">{super::labels::stop_label(reason)}</p>
                <p class="t-subheadline t-secondary">{detail}</p>
                <div class="ui-hstack" style="--spacing: 8px; flex-wrap: wrap">
                    {options
                        .into_iter()
                        .map(|extra| {
                            view! {
                                <Button
                                    variant=ButtonStyle::GlassProminent
                                    size=ControlSize::Small
                                    on_click=move |_| raise(extra)
                                >
                                    {t!(
                                        AgentDecisionsRaiseResume,
                                        amount = super::labels::raise_amount(reason, extra)
                                    )}
                                </Button>
                            }
                        })
                        .collect_view()}
                    <Button size=ControlSize::Small destructive=true on_click=cancel>
                        {t!(AgentDecisionsCancel)}
                    </Button>
                </div>
                <p class="t-footnote t-secondary">
                    {t!(AgentDecisionsRaises, what = super::labels::raises_label(reason))}
                </p>
            </SuggestionCard>
        </div>
    })
}

/// A question of the agent with suggestions and a free answer.
pub(super) fn question(ctx: AgentCtx) -> Option<impl IntoView> {
    let AgentCtx {
        run,
        answer,
        answer_empty,
        ..
    } = ctx;
    let send_answer = move |text: String| ctx.send_answer(text);
    let question = run.get().filter(|r| r.is_asking())?.question?;
    Some(view! {
        <div class="ui-list__section" role="region" aria-label=t!(AgentDecisionsQuestionLabel)>
            <p class="ui-list__header">{t!(AgentDecisionsQuestionHeader)}</p>
            <SuggestionCard state=SuggestionState::Pending>
                <p class="t-headline">{question.text.clone()}</p>
                <div class="ui-hstack" style="--spacing: 8px">
                    {question
                        .suggestions
                        .iter()
                        .cloned()
                        .map(|text| {
                            let label = text.clone();
                            view! {
                                <Button size=ControlSize::Small on_click=move |_| send_answer(text.clone())>
                                    {label}
                                </Button>
                            }
                        })
                        .collect_view()}
                </div>
            </SuggestionCard>
            <div class="ui-vstack" style="--spacing: 10px">
                <Textarea
                    value=answer
                    label=t!(AgentDecisionsYourAnswer)
                    placeholder=t!(AgentDecisionsAnswerPlaceholder)
                    min_height="72px"
                />
                <div class="ui-hstack">
                    <Button
                        variant=ButtonStyle::GlassProminent
                        disabled=answer_empty
                        on_click=move |_| send_answer(answer.get_untracked())
                    >
                        {t!(AgentDecisionsAnswer)}
                    </Button>
                </div>
            </div>
        </div>
    })
}

/// Hints to a working agent.
pub(super) fn steer_card(ctx: AgentCtx) -> impl IntoView {
    let AgentCtx {
        working,
        steer,
        steer_empty,
        ..
    } = ctx;
    let send_steer = move |_| ctx.send_steer();
    view! {
        <Show when=move || working.get()>
            <div class="ui-list__section" role="region" aria-label=t!(AgentDecisionsSteerLabel)>
                <p class="ui-list__header">{t!(AgentDecisionsSteerHeader)}</p>
                <div class="ui-vstack" style="--spacing: 10px">
                    <Textarea
                        value=steer
                        label=t!(AgentDecisionsSteerField)
                        placeholder=t!(AgentDecisionsSteerPlaceholder)
                        min_height="72px"
                    />
                    <div class="ui-hstack">
                        <Button disabled=steer_empty on_click=send_steer>
                            {t!(AgentDecisionsSendHint)}
                        </Button>
                    </div>
                </div>
            </div>
        </Show>
    }
}

/// The plan of the agent, with the note for a new plan.
pub(super) fn plan_card(ctx: AgentCtx) -> Option<impl IntoView> {
    let AgentCtx {
        note,
        awaiting,
        plan,
        note_empty,
        ..
    } = ctx;
    let revise = move |_| ctx.revise();
    let plan = plan.get()?;
    let waiting = awaiting.get();
    Some(view! {
        <div class="ui-list__section" role="region" aria-label=t!(AgentDecisionsPlanLabel)>
            <p class="ui-list__header">
                {if waiting {
                    t!(AgentDecisionsPlanWaiting)
                } else {
                    t!(AgentDecisionsPlanApproved)
                }}
            </p>
            <SuggestionCard state=SuggestionState::Pending>
                <p class="t-subheadline">{plan.summary.clone()}</p>
                <p class="t-footnote t-secondary">{t!(AgentDecisionsFiles)}</p>
                <ul class="ck-plan">
                    {plan
                        .files
                        .iter()
                        .map(|f| view! { <li class="t-mono t-footnote">{f.clone()}</li> })
                        .collect_view()}
                </ul>
                <p class="t-footnote t-secondary">{t!(AgentDecisionsSteps)}</p>
                <ol class="ck-plan">
                    {plan
                        .steps
                        .iter()
                        .map(|s| view! { <li class="t-subheadline">{s.clone()}</li> })
                        .collect_view()}
                </ol>
                {(!plan.rules.is_empty())
                    .then(|| {
                        view! {
                            <p class="t-footnote t-secondary">{t!(AgentDecisionsRulesApply)}</p>
                            <ul class="ck-plan" aria-label=t!(AgentDecisionsRulesApply)>
                                {plan
                                    .rules
                                    .iter()
                                    .map(|r| view! { <li class="t-subheadline">{r.clone()}</li> })
                                    .collect_view()}
                            </ul>
                        }
                    })}
                {(!plan.violations.is_empty())
                    .then(|| {
                        view! {
                            <p class="t-footnote ck-problem">
                                {t!(AgentDecisionsNotApprovable)}
                            </p>
                            <ul class="ck-plan" aria-label=t!(AgentDecisionsViolationsLabel)>
                                {plan
                                    .violations
                                    .iter()
                                    .map(|v| view! { <li class="t-subheadline">{v.reason()}</li> })
                                    .collect_view()}
                            </ul>
                            <p class="t-footnote t-secondary">
                                {t!(AgentDecisionsChangeRulesBefore)} " "
                                <a class="ui-link" href=Route::Limits.hash()>{t!(AgentDecisionsLimitsLink)}</a>
                                " " {t!(AgentDecisionsChangeRulesAfter)}
                            </p>
                        }
                    })}
            </SuggestionCard>
            <Show when=move || awaiting.get()>
                <div class="ui-vstack" style="--spacing: 10px">
                    <Textarea
                        value=note
                        label=t!(AgentDecisionsNoteLabel)
                        placeholder=t!(AgentDecisionsNotePlaceholder)
                        min_height="72px"
                    />
                    <div class="ui-hstack">
                        <Button disabled=note_empty on_click=revise>
                            {t!(AgentDecisionsNewPlan)}
                        </Button>
                    </div>
                </div>
            </Show>
        </div>
    })
}
