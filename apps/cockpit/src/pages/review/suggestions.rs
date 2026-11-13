use super::*;

/// Right: suggestions of the agent, own comment, verdict and sending.
pub(super) fn agent_pane(store: Store, r: Review, panel: Panel) -> impl IntoView {
    let number = r.number;
    let undecided = r.undecided();
    let (summary, send_label, can_send, sent) = (r.summary(), r.send_label(), r.can_send(), r.sent);
    let (agent_reviewed, verdict) = (r.agent_reviewed, r.verdict.id());
    let suggestions = r
        .suggestions
        .iter()
        .cloned()
        .map(|s| suggestion(store, number, s, panel))
        .collect_view();

    let request_review = move |_| {
        panel.agent_reading.set(true);
        set_timeout(
            move || {
                let _ = store.dispatch(Action::RequestAgentReview { review: number });
                // The page may have been left in the meantime.
                let _ = panel.agent_reading.try_set(false);
            },
            REVIEW_TIME,
        );
    };
    let send = move |_| {
        let own = panel.own_comment.get_untracked();
        let sent = store.dispatch(Action::SendReview {
            review: number,
            comment: own,
        });
        if sent.is_ok() {
            panel.own_comment.set(String::new());
        }
    };

    let intro = if !r.agent_reviewed {
        "Der Agent hat diesen Pull Request noch nicht durchgesehen."
    } else if r.suggestions.is_empty() {
        "Der Agent hat nichts zu beanstanden."
    } else {
        "Der Agent hat Kommentare vorbereitet. Nichts wird gesendet, bevor du es freigibst."
    };

    view! {
        <Pane label="Agent-Review">
            <PaneHeader>
                <div class="ui-hstack" style="--spacing: 10px; flex-wrap: nowrap">
                    <Avatar agent=true large=true />
                    <div class="ui-vstack" style="--spacing: 0">
                        <h2 class="t-title3 t-bold">"Agent-Review"</h2>
                        <span class="t-footnote t-secondary">{summary}</span>
                    </div>
                </div>
                <p class="t-subheadline t-secondary">{intro}</p>
            </PaneHeader>

            <PaneBody>
                {(!agent_reviewed)
                    .then(|| {
                        view! {
                            <Button
                                size=ControlSize::Large
                                disabled=panel.agent_reading
                                on_click=request_review
                            >
                                <Icon path=icons::SPARKLES />
                                {move || {
                                    if panel.agent_reading.get() { "Agent liest …" } else { "Agent-Review anfordern" }
                                }}
                            </Button>
                        }
                    })}
                <div class="ui-vstack" style="--spacing: 10px">
                    {suggestions}
                </div>

                <div class="ui-vstack" style="--spacing: 8px">
                    <span class="t-footnote t-secondary" style="padding-inline: 4px">
                        "Eigener Kommentar (optional)"
                    </span>
                    <Textarea
                        value=panel.own_comment
                        label="Eigener Kommentar"
                        placeholder="Zusammenfassung für das Review …"
                        min_height="72px"
                    />
                </div>

                <div class="ui-vstack" style="--spacing: 12px">
                    <SegmentedPicker
                        options=Verdict::OPTIONS
                        value=verdict
                        on_change=move |id: &'static str| {
                            if let Some(verdict) = Verdict::from_id(id) {
                                let _ = store.dispatch(Action::SetVerdict { review: number, verdict });
                            }
                        }
                        aria_label="Ergebnis"
                        stretch=true
                    />
                    <Button
                        variant=ButtonStyle::GlassProminent
                        size=ControlSize::Large
                        disabled=!can_send
                        on_click=send
                    >
                        {send_label}
                    </Button>
                    {(undecided > 0)
                        .then(|| {
                            view! {
                                <p class="t-footnote t-secondary" style="text-align: center">
                                    "Entscheide zuerst über alle Vorschläge."
                                </p>
                            }
                        })}
                    {sent
                        .then(|| {
                            view! {
                                <div class="ui-hstack" style="justify-content: center">
                                    <Status tone=StatusTone::Green>"Review auf GitHub gesendet"</Status>
                                </div>
                            }
                        })}
                </div>

                {if store.agent_real() {
                    followup_section(store, &r).into_any()
                } else {
                    comments_section(store, &r, panel).into_any()
                }}
                {checks_section(store, &r, panel)}
                {merge_section(store, &r, panel)}
            </PaneBody>
        </Pane>
    }
}

/// A suggestion: approve, reject, edit – or take the decision back.
pub(super) fn suggestion(store: Store, number: u32, s: Suggestion, panel: Panel) -> impl IntoView {
    let id = StoredValue::new(s.id.clone());
    let text = StoredValue::new(s.text.clone());
    let editing = Signal::derive(move || panel.editing.get().is_some_and(|e| e == id.get_value()));
    let decide = move |decision: Option<Decision>| {
        move |_| {
            let _ = store.dispatch(Action::Decide {
                review: number,
                suggestion: id.get_value(),
                decision,
            });
        }
    };

    let (state, badge) = (
        match s.decision {
            None => SuggestionState::Pending,
            Some(Decision::Approved) => SuggestionState::Approved,
            Some(Decision::Declined) => SuggestionState::Declined,
        },
        match s.level {
            Level::Note => BadgeVariant::Neutral,
            Level::Important => BadgeVariant::Tint,
        },
    );

    let editor = move || {
        view! {
            <Textarea value=panel.edit_text label="Kommentar bearbeiten" min_height="72px" />
            <div class="ui-hstack" style="--spacing: 8px">
                <Button
                    variant=ButtonStyle::BorderedProminent
                    size=ControlSize::Small
                    disabled=Signal::derive(move || panel.edit_text.get().trim().is_empty())
                    on_click=move |_| {
                        let new_text = panel.edit_text.get_untracked();
                        panel.editing.set(None);
                        let _ = store.dispatch(Action::EditSuggestion {
                            review: number,
                            suggestion: id.get_value(),
                            text: new_text,
                        });
                    }
                >
                    "Sichern"
                </Button>
                <Button
                    variant=ButtonStyle::Borderless
                    size=ControlSize::Small
                    on_click=move |_| panel.editing.set(None)
                >
                    "Abbrechen"
                </Button>
            </div>
        }
    };

    let actions = move || {
        match s.decision {
        None => view! {
            <div class="ui-hstack" style="--spacing: 8px">
                <Button
                    variant=ButtonStyle::BorderedProminent
                    size=ControlSize::Small
                    on_click=decide(Some(Decision::Approved))
                >
                    "Freigeben"
                </Button>
                <Button size=ControlSize::Small destructive=true on_click=decide(Some(Decision::Declined))>
                    "Ablehnen"
                </Button>
                <Button
                    variant=ButtonStyle::Borderless
                    size=ControlSize::Small
                    on_click=move |_| {
                        panel.edit_text.set(text.get_value());
                        panel.editing.set(Some(id.get_value()));
                    }
                >
                    "Bearbeiten"
                </Button>
            </div>
        }
        .into_any(),
        Some(decision) => {
            let (label, tone) = match decision {
                Decision::Approved => ("Freigegeben – wird gesendet", StatusTone::Green),
                Decision::Declined => ("Abgelehnt – wird verworfen", StatusTone::Gray),
            };
            view! {
                <div class="ui-hstack" style="justify-content: space-between">
                    <Status tone=tone>{label}</Status>
                    <Button variant=ButtonStyle::Borderless size=ControlSize::Small on_click=decide(None)>
                        "Rückgängig"
                    </Button>
                </div>
            }
            .into_any()
        }
    }
    };

    view! {
        <SuggestionCard state=state>
            <div class="ui-hstack" style="--spacing: 8px">
                <span class="t-footnote t-mono t-secondary">{s.location.clone()}</span>
                <span class="ui-spacer"></span>
                <Badge variant=badge>{s.level.label()}</Badge>
            </div>
            {move || {
                if editing.get() {
                    editor().into_any()
                } else {
                    view! {
                        <p class="t-subheadline">{text.get_value()}</p>
                        {actions()}
                    }
                        .into_any()
                }
            }}
        </SuggestionCard>
    }
}
