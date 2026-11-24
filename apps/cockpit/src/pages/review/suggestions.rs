use super::*;
use cockpit_core::language::LangSource;

/// Right: suggestions of the agent, own comment, verdict and sending.
pub(super) fn agent_pane(store: Store, r: Review, panel: Panel) -> impl IntoView {
    let number = StoredValue::new(r.key());
    let undecided = r.undecided();
    let summary = t!(
        ReviewSuggestionsSummary,
        approved = r.approved(),
        declined = r.declined(),
        open = r.undecided()
    );
    let send_label = if r.sent {
        t!(ReviewSuggestionsSendDone)
    } else {
        t!(ReviewSuggestionsSend, n = r.comment_count())
    };
    let (can_send, sent) = (r.can_send(), r.sent);
    let verdict = r.verdict.id();
    let suggestions = r
        .suggestions
        .iter()
        .cloned()
        .map(|s| {
            let rows = code_rows(&r, &s);
            suggestion(store, number, s, panel, rows)
        })
        .collect_view();
    let summary_text = r.agent_summary.clone();

    // Only you start the run; the agent reads, nothing goes to GitHub.
    let review_run = move |_| {
        let _ = store.dispatch(Action::StartPrReview {
            language: panel.review_language.get_untracked(),
            repo_language: None,
            review: number.get_value(),
        });
    };
    let run_busy = Signal::derive(move || {
        store
            .ws
            .with(|w| w.active_followup(number.get_value()).is_some())
    });
    let run_state = move || {
        store.ws.with(|w| {
            w.runs
                .iter()
                .find(|run| matches!(&run.kind, RunKind::ReviewPr { review } if number.get_value().same(review)))
                .cloned()
        })
    };
    let blocked = Signal::derive(move || store.agent_blocked().is_some());
    let send = move |_| {
        let own = panel.own_comment.get_untracked();
        let sent = store.dispatch(Action::SendReview {
            review: number.get_value(),
            comment: own,
        });
        if sent.is_ok() {
            panel.own_comment.set(String::new());
        }
    };

    let intro = if !r.agent_reviewed {
        t!(ReviewSuggestionsIntroReal)
    } else if r.suggestions.is_empty() {
        t!(ReviewSuggestionsIntroClean)
    } else {
        t!(ReviewSuggestionsIntroDrafts)
    };

    view! {
        <Pane label=t!(ReviewSuggestionsPane)>
            <PaneHeader>
                <div class="ui-hstack" style="--spacing: 10px; flex-wrap: nowrap">
                    <Avatar agent=true large=true />
                    <div class="ui-vstack" style="--spacing: 0">
                        <h2 class="t-title3 t-bold">{t!(ReviewSuggestionsPane)}</h2>
                        <span class="t-footnote t-secondary">{summary}</span>
                    </div>
                </div>
                <p class="t-subheadline t-secondary">{intro}</p>
            </PaneHeader>

            <PaneBody>
                <div class="ui-vstack" style="--spacing: 6px">
                    {segmented(
                        vec![
                            ("auto", t!(ReviewSuggestionsLangAuto)),
                            ("de", "DE".to_string()),
                            ("en", "EN".to_string()),
                        ],
                        move || panel.review_language.get().map_or("auto", |l| l.code()),
                        move |id| panel.review_language.set(cockpit_i18n::Lang::from_code(id)),
                        t!(ReviewSuggestionsLangLabel),
                        false,
                    )}
                    <span class="t-footnote t-secondary" style="padding-inline: 4px">
                        {move || match panel.review_language.get() {
                            Some(l) => t!(ReviewSuggestionsLangNote, lang = lang_name(l), why = lang_why(LangSource::Chosen)),
                            None => t!(ReviewSuggestionsLangAutoHint),
                        }}
                    </span>
                </div>
                <Button
                    variant=ButtonStyle::BorderedProminent
                    size=ControlSize::Large
                    disabled=Signal::derive(move || run_busy.get() || blocked.get())
                    on_click=review_run
                >
                    <Icon path=icons::SPARKLES />
                    {move || if run_busy.get() { t!(ReviewSuggestionsReading) } else { t!(ReviewSuggestionsReviewPr) }}
                </Button>
                {move || {
                    run_state().map(|run| {
                        let used = run.options.review_language.map(|c| {
                            view! {
                                <p class="t-footnote t-secondary">
                                    {t!(ReviewSuggestionsLangUsed, lang = lang_name(c.lang), why = lang_why(c.source))}
                                </p>
                            }
                        });
                        let state = if run.is_active() {
                            let key = run.key();
                            view! {
                                <div class="ui-hstack" style="--spacing: 8px">
                                    <Status tone=StatusTone::Blue>{t!(ReviewStateWorking)}</Status>
                                    <span class="ui-spacer"></span>
                                    <a class="ui-link t-footnote" href=Route::Agent(Some(key)).hash()>{t!(ReviewRunView)}</a>
                                </div>
                            }
                            .into_any()
                        } else if let Some(reason) = run.ended.clone() {
                            view! {
                                <p class="t-footnote t-secondary">
                                    {t!(ReviewSuggestionsLastRunEnded, reason = reason)}
                                </p>
                            }
                            .into_any()
                        } else {
                            ().into_any()
                        };
                        view! { {used} {state} }
                    })
                }}
                {(!summary_text.is_empty())
                    .then(|| {
                        let text = StoredValue::new(summary_text.clone());
                        view! {
                            <div class="ui-vstack" style="--spacing: 8px" role="region" aria-label=t!(ReviewSuggestionsSummaryRegion)>
                                <span class="t-footnote t-secondary" style="padding-inline: 4px">
                                    {t!(ReviewSuggestionsSummaryNote)}
                                </span>
                                <Markdown text=text.get_value() class="t-subheadline" />
                                <Button
                                    variant=ButtonStyle::Borderless
                                    size=ControlSize::Small
                                    on_click=move |_| {
                                        panel.own_comment.update(|c| {
                                            if !c.trim().is_empty() {
                                                c.push_str("\n\n");
                                            }
                                            c.push_str(&text.get_value());
                                        });
                                    }
                                >
                                    {t!(ReviewSuggestionsTakeOver)}
                                </Button>
                            </div>
                        }
                    })}
                <div class="ui-vstack" style="--spacing: 10px">
                    {suggestions}
                </div>

                <div class="ui-vstack" style="--spacing: 8px">
                    <span class="t-footnote t-secondary" style="padding-inline: 4px">
                        {t!(ReviewSuggestionsOwnNote)}
                    </span>
                    <Textarea
                        value=panel.own_comment
                        label=t!(ReviewSuggestionsOwnLabel)
                        placeholder=t!(ReviewSuggestionsOwnPlaceholder)
                        min_height="72px"
                    />
                </div>

                <div class="ui-vstack" style="--spacing: 12px">
                    {segmented(
                        vec![
                            ("comment", t!(ReviewSuggestionsVerdictComment)),
                            ("approve", t!(ReviewSuggestionsVerdictApprove)),
                            ("changes", t!(ReviewSuggestionsVerdictChanges)),
                        ],
                        move || verdict,
                        move |id| {
                            if let Some(verdict) = Verdict::from_id(id) {
                                let _ = store.dispatch(Action::SetVerdict { review: number.get_value(), verdict });
                            }
                        },
                        t!(ReviewSuggestionsVerdict),
                        true,
                    )}
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
                                    {t!(ReviewSuggestionsDecideFirst)}
                                </p>
                            }
                        })}
                    {sent
                        .then(|| {
                            view! {
                                <div class="ui-hstack" style="justify-content: center">
                                    <Status tone=StatusTone::Green>{t!(ReviewSuggestionsReviewSent)}</Status>
                                </div>
                            }
                        })}
                </div>

                {followup_section(store, &r)}
                {checks_section(store, &r, panel)}
                {merge_section(store, &r, panel)}
            </PaneBody>
        </Pane>
    }
}

/// The name of a review language in the language of the interface.
fn lang_name(lang: cockpit_i18n::Lang) -> String {
    match lang {
        cockpit_i18n::Lang::De => t!(ReviewSuggestionsLangDe),
        cockpit_i18n::Lang::En => t!(ReviewSuggestionsLangEn),
    }
}

/// Why a review language was chosen, in words.
fn lang_why(source: LangSource) -> String {
    match source {
        LangSource::Chosen => t!(ReviewSuggestionsLangWhyChosen),
        LangSource::PrText => t!(ReviewSuggestionsLangWhyPr),
        LangSource::Ticket => t!(ReviewSuggestionsLangWhyTicket),
        LangSource::Repo => t!(ReviewSuggestionsLangWhyRepo),
        LangSource::Default => t!(ReviewSuggestionsLangWhyDefault),
    }
}

/// The importance of a suggestion in words.
fn level_label(level: Level) -> String {
    match level {
        Level::Note => t!(ReviewSuggestionsLevelNote),
        Level::Important => t!(ReviewSuggestionsLevelImportant),
    }
}

/// A suggestion: approve, reject, edit – or take the decision back.
pub(super) fn suggestion(
    store: Store,
    number: StoredValue<PrKey>,
    s: Suggestion,
    panel: Panel,
    code: Vec<DiffRow>,
) -> impl IntoView {
    let id = StoredValue::new(s.id.clone());
    let text = StoredValue::new(s.text.clone());
    let editing = Signal::derive(move || panel.editing.get().is_some_and(|e| e == id.get_value()));
    let decide = move |decision: Option<Decision>| {
        move |_| {
            let _ = store.dispatch(Action::Decide {
                review: number.get_value(),
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
            <Textarea value=panel.edit_text label=t!(ReviewEditComment) min_height="72px" />
            <div class="ui-hstack" style="--spacing: 8px">
                <Button
                    variant=ButtonStyle::BorderedProminent
                    size=ControlSize::Small
                    disabled=Signal::derive(move || panel.edit_text.get().trim().is_empty())
                    on_click=move |_| {
                        let new_text = panel.edit_text.get_untracked();
                        panel.editing.set(None);
                        let _ = store.dispatch(Action::EditSuggestion {
                            review: number.get_value(),
                            suggestion: id.get_value(),
                            text: new_text,
                        });
                    }
                >
                    {t!(ReviewSave)}
                </Button>
                <Button
                    variant=ButtonStyle::Borderless
                    size=ControlSize::Small
                    on_click=move |_| panel.editing.set(None)
                >
                    {t!(ReviewCancel)}
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
                    {t!(ReviewApprove)}
                </Button>
                <Button size=ControlSize::Small destructive=true on_click=decide(Some(Decision::Declined))>
                    {t!(ReviewDiscard)}
                </Button>
                <Button
                    variant=ButtonStyle::Borderless
                    size=ControlSize::Small
                    on_click=move |_| {
                        panel.edit_text.set(text.get_value());
                        panel.editing.set(Some(id.get_value()));
                    }
                >
                    {t!(ReviewEdit)}
                </Button>
            </div>
        }
        .into_any(),
        Some(decision) => {
            let (label, tone) = match decision {
                Decision::Approved => (t!(ReviewSuggestionsApproved), StatusTone::Green),
                Decision::Declined => (t!(ReviewSuggestionsDeclined), StatusTone::Gray),
            };
            view! {
                <div class="ui-hstack" style="justify-content: space-between">
                    <Status tone=tone>{label}</Status>
                    <Button variant=ButtonStyle::Borderless size=ControlSize::Small on_click=decide(None)>
                        {t!(ReviewUndo)}
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
                <span class="t-footnote t-mono t-secondary">{location_label(&s)}</span>
                <span class="ui-spacer"></span>
                <Badge variant=badge>{level_label(s.level)}</Badge>
            </div>
            {move || {
                if editing.get() {
                    editor().into_any()
                } else {
                    view! {
                        <p class="t-subheadline">{text.get_value()}</p>
                        {(!code.is_empty())
                            .then(|| view! { <Diff rows=code.clone() label=t!(ReviewSuggestion) /> })}
                        {actions()}
                    }
                        .into_any()
                }
            }}
        </SuggestionCard>
    }
}
