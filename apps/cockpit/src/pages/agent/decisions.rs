//! The cards that wait for a decision of the user: diagnosis, limit, question, hint and plan.

use crate::route::Route;
use feature_ui::*;
use leptos::prelude::*;

use super::context::AgentCtx;

/// The diagnosis of a failed check, waiting for a decision.
pub(super) fn diagnosis(ctx: AgentCtx) -> Option<impl IntoView> {
    let AgentCtx { run, .. } = ctx;
    let d = run.get().filter(|r| r.is_diagnosed())?.diagnosis?;
    Some(view! {
        <div class="ui-list__section" role="region" aria-label="Diagnose des Agenten">
            <p class="ui-list__header">"Diagnose – wartet auf deine Entscheidung"</p>
            <SuggestionCard state=SuggestionState::Pending>
                <div class="ui-hstack" style="--spacing: 8px">
                    <Badge variant=BadgeVariant::Tint>{format!("Check {}", d.check)}</Badge>
                    {d
                        .location()
                        .map(|l| view! { <span class="t-footnote t-mono t-secondary">{l}</span> })}
                </div>
                <p class="t-headline">{d.cause.clone()}</p>
                <p class="t-footnote t-secondary">"Vorschlag: " {d.remedy.clone()}</p>
                {d
                    .patch
                    .clone()
                    .map(|p| {
                        view! {
                            <Diff
                                rows=crate::view::diff_rows(&p)
                                label=format!("Fix für {}", p.name)
                            />
                        }
                    })}
                <p class="t-footnote t-secondary">
                    "Bis du entscheidest, bleibt der Branch unverändert."
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
        <div class="ui-list__section" role="region" aria-label="Lauf angehalten">
            <p class="ui-list__header">"Lauf angehalten – wartet auf dich"</p>
            <SuggestionCard state=SuggestionState::Pending>
                <p class="t-headline">{reason.label()}</p>
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
                                    {format!("Um {} erhöhen und fortsetzen", reason.raise_label(extra))}
                                </Button>
                            }
                        })
                        .collect_view()}
                    <Button size=ControlSize::Small destructive=true on_click=cancel>
                        "Abbrechen"
                    </Button>
                </div>
                <p class="t-footnote t-secondary">
                    {format!("Erhöht wird {}.", reason.raises())}
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
        <div class="ui-list__section" role="region" aria-label="Frage des Agenten">
            <p class="ui-list__header">"Frage des Agenten – wartet auf deine Antwort"</p>
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
                    label="Deine Antwort"
                    placeholder="Oder eigene Antwort schreiben …"
                    min_height="72px"
                />
                <div class="ui-hstack">
                    <Button
                        variant=ButtonStyle::GlassProminent
                        disabled=answer_empty
                        on_click=move |_| send_answer(answer.get_untracked())
                    >
                        "Antworten"
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
            <div class="ui-list__section" role="region" aria-label="Agent nachsteuern">
                <p class="ui-list__header">"Nachsteuern"</p>
                <div class="ui-vstack" style="--spacing: 10px">
                    <Textarea
                        value=steer
                        label="Anweisung an den Agenten"
                        placeholder="Anweisung für den nächsten Schritt, z. B.: Bitte auch die Doku anpassen."
                        min_height="72px"
                    />
                    <div class="ui-hstack">
                        <Button disabled=steer_empty on_click=send_steer>
                            "Hinweis senden"
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
        <div class="ui-list__section" role="region" aria-label="Plan des Agenten">
            <p class="ui-list__header">
                {if waiting { "Plan – wartet auf deine Freigabe" } else { "Freigegebener Plan" }}
            </p>
            <SuggestionCard state=SuggestionState::Pending>
                <p class="t-subheadline">{plan.summary.clone()}</p>
                <p class="t-footnote t-secondary">"Dateien"</p>
                <ul class="ck-plan">
                    {plan
                        .files
                        .iter()
                        .map(|f| view! { <li class="t-mono t-footnote">{f.clone()}</li> })
                        .collect_view()}
                </ul>
                <p class="t-footnote t-secondary">"Schritte"</p>
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
                            <p class="t-footnote t-secondary">"Regeln, die gelten"</p>
                            <ul class="ck-plan" aria-label="Regeln, die gelten">
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
                                "Nicht freigebbar – der Plan verstößt gegen die Regeln:"
                            </p>
                            <ul class="ck-plan" aria-label="Regelverstöße">
                                {plan
                                    .violations
                                    .iter()
                                    .map(|v| view! { <li class="t-subheadline">{v.reason()}</li> })
                                    .collect_view()}
                            </ul>
                            <p class="t-footnote t-secondary">
                                "Ändere die Regeln unter "
                                <a class="ui-link" href=Route::Limits.hash()>"Grenzen & Regeln"</a>
                                " oder lehne den Plan ab."
                            </p>
                        }
                    })}
            </SuggestionCard>
            <Show when=move || awaiting.get()>
                <div class="ui-vstack" style="--spacing: 10px">
                    <Textarea
                        value=note
                        label="Anmerkung für einen neuen Plan"
                        placeholder="Anmerkung für einen neuen Plan, z. B.: Bitte auch den Slider-Thumb ändern."
                        min_height="72px"
                    />
                    <div class="ui-hstack">
                        <Button disabled=note_empty on_click=revise>
                            "Neuen Plan anfordern"
                        </Button>
                    </div>
                </div>
            </Show>
        </div>
    })
}
