//! The „Aktivität“ pane: conversation, log, changed files and result.

use cockpit_core::{MessageKind, RunMessage, Speaker};
use feature_ui::*;
use leptos::prelude::*;

use super::context::AgentCtx;

/// A line of the history as a speech bubble.
fn bubble(m: RunMessage) -> impl IntoView {
    let own = m.from == Speaker::User;
    let who = match (m.from, m.kind) {
        (Speaker::Agent, MessageKind::Question) => "Agent fragt",
        (Speaker::Agent, _) => "Agent",
        (Speaker::User, MessageKind::Answer) => "Du antwortest",
        (Speaker::User, MessageKind::Hint) => "Du schickst einen Hinweis",
        (Speaker::User, _) => "Du",
    };
    let time = match m.step {
        0 => "Start".to_string(),
        n => format!("nach Schritt {n}"),
    };
    view! {
        <CommentBubble
            who=who
            initials=if own { "AS" } else { "AG" }
            time=time
            text=m.text
            own=own
            agent=!own
        />
    }
}

/// Conversation history: agent messages, questions, your answers and hints.
pub(super) fn history(ctx: AgentCtx) -> AnyView {
    let AgentCtx { run, .. } = ctx;
    let messages = run.get().map(|r| r.messages).unwrap_or_default();
    if messages.is_empty() {
        return view! { <p class="t-footnote t-secondary">"Noch nichts gesagt."</p> }.into_any();
    }
    messages.into_iter().map(bubble).collect_view().into_any()
}

/// As soon as the agent works on the files, they appear here.
pub(super) fn changed_files(ctx: AgentCtx) -> Option<impl IntoView> {
    let AgentCtx { store, run, .. } = ctx;
    // As soon as the agent works on the files, they appear here.
    let run = run.get().filter(|r| r.completed >= 2)?;
    let files = store.ws.with(|w| {
        run.pr_number
            .and_then(|n| w.review(n))
            .map(|r| r.files.clone())
    });
    let files = files.unwrap_or_else(|| cockpit_core::demo::agent_changes(&run.ticket_key));
    Some(view! {
        <List>
            <ListSection header="Geänderte Dateien" nested=true>
                {files
                    .into_iter()
                    .map(|f| {
                        view! {
                            <li>
                                <div class="ui-list__row">
                                    <span class="ui-list__title t-mono t-subheadline">
                                        {f.name.clone()}
                                    </span>
                                    <span class="ui-list__detail t-footnote t-mono">
                                        {f.stat()}
                                    </span>
                                </div>
                            </li>
                        }
                    })
                    .collect_view()}
            </ListSection>
        </List>
    })
}

/// Result card as soon as the pull request exists.
pub(super) fn result_card(ctx: AgentCtx) -> Option<impl IntoView> {
    let AgentCtx { run, .. } = ctx;
    let run = run.get().filter(|r| r.is_done())?;
    let number = run.pr_number?;
    if run.is_followup() {
        return Some(view! {
            <SuggestionCard state=SuggestionState::Approved>
                <div class="ui-hstack" style="--spacing: 8px">
                    <Badge variant=BadgeVariant::Neutral>"GitHub"</Badge>
                    <span class="t-footnote t-mono t-secondary">{format!("#{number}")}</span>
                    <span class="ui-spacer"></span>
                    <Status tone=StatusTone::Green>"Gepusht"</Status>
                </div>
                <p class="t-headline">{run.label()}</p>
                <span style="align-self: flex-start; max-width: 100%">
                    <Chip>{run.branch()}</Chip>
                </span>
                <p class="t-footnote t-secondary">
                    "Kein neuer Pull Request, der Agent merged nie. Das Review liegt wieder bei dir."
                </p>
            </SuggestionCard>
        });
    }
    Some(view! {
        <SuggestionCard state=SuggestionState::Approved>
            <div class="ui-hstack" style="--spacing: 8px">
                <Badge variant=BadgeVariant::Neutral>"GitHub"</Badge>
                <span class="t-footnote t-mono t-secondary">{format!("#{number}")}</span>
                <span class="ui-spacer"></span>
                <Status tone=StatusTone::Green>
                    {if run.options.draft_pr { "Entwurf erstellt" } else { "Erstellt" }}
                </Status>
            </div>
            <p class="t-headline">{run.ticket_title.clone()}</p>
            <span style="align-self: flex-start; max-width: 100%">
                <Chip>{run.branch()}</Chip>
            </span>
            <p class="t-footnote t-secondary">"Der Agent führt keinen Merge aus. Das Review liegt bei dir."</p>
        </SuggestionCard>
    })
}

/// The „Aktivität“ pane.
pub(super) fn pane(ctx: AgentCtx) -> impl IntoView {
    let AgentCtx { run, .. } = ctx;
    view! {
        <Pane label="Aktivität">
            <PaneHeader>
                <h2 class="t-title3 t-bold">"Aktivität"</h2>
            </PaneHeader>
            <PaneBody>
                <div class="ui-list__section" role="region" aria-label="Verlauf">
                    <p class="ui-list__header">"Verlauf"</p>
                    <div class="ck-thread">{move || history(ctx)}</div>
                </div>
                <Log text=Signal::derive(move || run.get().map(|r| r.log()).unwrap_or_default()) />
                {move || changed_files(ctx)}
                {move || result_card(ctx)}
            </PaneBody>
        </Pane>
    }
}
