use super::*;

/// Before/after of a suggested change as a small diff.
pub(super) fn suggestion_view(diff: &SuggestionDiff) -> impl IntoView {
    let mut rows = vec![DiffRow {
        no: diff.no,
        kind: DiffKind::Del,
        text: diff.before.clone(),
    }];
    rows.extend(diff.after.iter().enumerate().map(|(i, text)| DiffRow {
        no: diff.no + i as u32,
        kind: DiffKind::Add,
        text: text.clone(),
    }));
    view! { <Diff rows=rows label=t!(ReviewSuggestion) /> }
}

/// Switch and field for the replacement code. When switching on, the old line is in it.
pub(super) fn suggestion_editor(
    on: RwSignal<bool>,
    code: RwSignal<String>,
    original: String,
) -> impl IntoView {
    let original = StoredValue::new(original);
    view! {
        <div class="ui-hstack" style="--spacing: 8px">
            <Button
                variant=ButtonStyle::Borderless
                size=ControlSize::Small
                on_click=move |_| {
                    if !on.get_untracked() {
                        code.set(original.get_value());
                    }
                    on.update(|v| *v = !*v);
                }
            >
                {move || if on.get() { t!(ReviewDiffSuggestOff) } else { t!(ReviewDiffSuggestOn) }}
            </Button>
        </div>
        {move || {
            on.get()
                .then(|| {
                    view! {
                        <Textarea
                            value=code
                            label=t!(ReviewDiffCodeLabel)
                            placeholder=t!(ReviewDiffCodePlaceholder)
                            min_height="64px"
                        />
                    }
                })
        }}
    }
}

/// Content under a diff line: own comments and, when selected, the input field.
pub(super) fn line_notes(
    store: Store,
    r: &Review,
    file: &str,
    index: usize,
    ui: LineUi,
) -> AnyView {
    let (number, sent) = (StoredValue::new(r.key()), r.sent);
    let original = r
        .files
        .iter()
        .find(|f| f.name == file)
        .and_then(|f| f.lines.get(index))
        .map(|l| l.text.clone())
        .unwrap_or_default();
    let comments = r
        .line_comments_at(file, index)
        .into_iter()
        .map(|c| {
            let (id, text) = (StoredValue::new(c.id.clone()), StoredValue::new(c.text.clone()));
            let code = StoredValue::new(c.suggestion.clone());
            // An implemented suggestion is already in the diff, the before/after is dropped.
            let diff = r.suggestion_diff(c).filter(|_| c.is_open());
            let original = original.clone();
            // Comments by others cannot be changed, finished ones neither.
            let (own, open) = (c.is_own(), c.is_open());
            let resolved = c.resolved.clone();
            let author = c
                .author
                .as_ref()
                .map_or_else(|| t!(ReviewDiffYourComment), |p| t!(ReviewDiffCommentBy, name = p.name));
            let label = if diff.is_some() { t!(ReviewDiffWithSuggestion, author = author) } else { author };
            let editing = move || ui.editing.get().is_some_and(|e| e == id.get_value());
            view! {
                <div class="ui-diff__note" role="group" aria-label=t!(ReviewDiffOwnComment)>
                    {move || {
                        if editing() && !sent && own && open {
                            view! {
                                <Textarea value=ui.edit_text label=t!(ReviewEditComment) min_height="64px" />
                                {suggestion_editor(ui.edit_suggest, ui.edit_code, original.clone())}
                                <div class="ui-hstack" style="--spacing: 8px">
                                    <Button
                                        variant=ButtonStyle::BorderedProminent
                                        size=ControlSize::Small
                                        disabled=Signal::derive(move || {
                                            ui.edit_text.get().trim().is_empty()
                                                && !(ui.edit_suggest.get() && !ui.edit_code.get().trim().is_empty())
                                        })
                                        on_click=move |_| {
                                            let new_text = ui.edit_text.get_untracked();
                                            let suggestion = ui
                                                .edit_suggest
                                                .get_untracked()
                                                .then(|| ui.edit_code.get_untracked());
                                            let saved = store.dispatch(Action::EditLineComment {
                                                review: number.get_value(),
                                                comment: id.get_value(),
                                                text: new_text,
                                                suggestion,
                                            });
                                            // On an error (e.g. replacement = old line) the field stays open.
                                            if saved.is_ok() {
                                                ui.editing.set(None);
                                            }
                                        }
                                    >
                                        {t!(ReviewSave)}
                                    </Button>
                                    <Button
                                        variant=ButtonStyle::Borderless
                                        size=ControlSize::Small
                                        on_click=move |_| ui.editing.set(None)
                                    >
                                        {t!(ReviewCancel)}
                                    </Button>
                                </div>
                            }
                                .into_any()
                        } else {
                            view! {
                                <span class="ui-hstack t-footnote t-secondary" style="--spacing: 8px">
                                    {label.clone()}
                                    {resolved
                                        .clone()
                                        .map(|run| {
                                            view! {
                                                <Status tone=StatusTone::Green>{t!(ReviewDone)}</Status>
                                                <a class="ui-link" href=Route::Agent(Some(run.clone())).hash()>
                                                    {t!(ReviewDiffRun, run = run)}
                                                </a>
                                            }
                                        })}
                                </span>
                                {(!text.with_value(String::is_empty))
                                    .then(|| view! { <Markdown text=text.get_value() class="t-subheadline" /> })}
                                {diff.as_ref().map(suggestion_view)}
                                {(!sent && own && open)
                                    .then(|| {
                                        view! {
                                            <div class="ui-hstack" style="--spacing: 8px">
                                                <Button
                                                    variant=ButtonStyle::Borderless
                                                    size=ControlSize::Small
                                                    on_click=move |_| {
                                                        ui.edit_text.set(text.get_value());
                                                        let existing = code.get_value();
                                                        ui.edit_suggest.set(existing.is_some());
                                                        ui.edit_code.set(existing.unwrap_or_default());
                                                        ui.editing.set(Some(id.get_value()));
                                                    }
                                                >
                                                    {t!(ReviewEdit)}
                                                </Button>
                                                <Button
                                                    variant=ButtonStyle::Borderless
                                                    size=ControlSize::Small
                                                    destructive=true
                                                    on_click=move |_| {
                                                        let _ = store.dispatch(Action::DeleteLineComment {
                                                            review: number.get_value(),
                                                            comment: id.get_value(),
                                                        });
                                                    }
                                                >
                                                    {t!(ReviewDiffDelete)}
                                                </Button>
                                            </div>
                                        }
                                    })}
                            }
                                .into_any()
                        }
                    }}
                </div>
            }
        })
        .collect_view();

    let file = StoredValue::new(file.to_string());
    let original = StoredValue::new(original);
    let composer = move || {
        let here = ui.open.with(|o| {
            o.as_ref()
                .is_some_and(|(f, i)| *f == file.get_value() && *i == index)
        });
        (here && !sent).then(|| {
            view! {
                <div class="ui-diff__note">
                    <Textarea
                        value=ui.draft
                        label=t!(ReviewDiffLineComment)
                        placeholder=t!(ReviewDiffLinePlaceholder)
                        min_height="64px"
                    />
                    {suggestion_editor(ui.suggest, ui.code, original.get_value())}
                    <div class="ui-hstack" style="--spacing: 8px">
                        <Button
                            variant=ButtonStyle::BorderedProminent
                            size=ControlSize::Small
                            disabled=Signal::derive(move || {
                                ui.draft.get().trim().is_empty()
                                    && !(ui.suggest.get() && !ui.code.get().trim().is_empty())
                            })
                            on_click=move |_| {
                                let text = ui.draft.get_untracked();
                                let suggestion = ui.suggest.get_untracked().then(|| ui.code.get_untracked());
                                let added = store.dispatch(Action::AddLineComment {
                                    review: number.get_value(),
                                    file: file.get_value(),
                                    line: index,
                                    text,
                                    suggestion,
                                });
                                if added.is_ok() {
                                    ui.draft.set(String::new());
                                    ui.suggest.set(false);
                                    ui.open.set(None);
                                }
                            }
                        >
                            {t!(ReviewDiffAdd)}
                        </Button>
                        <Button
                            variant=ButtonStyle::Borderless
                            size=ControlSize::Small
                            on_click=move |_| ui.open.set(None)
                        >
                            {t!(ReviewCancel)}
                        </Button>
                    </div>
                </div>
            }
        })
    };
    let drafts = super::draft_notes(store, r, file.get_value().as_str(), index);
    view! {
        {drafts}
        {comments}
        {composer}
    }
    .into_any()
}

/// Middle: head of the pull request and the diff.
pub(super) fn diff_pane(store: Store, r: Review, ui: LineUi, panel: PanelUi) -> impl IntoView {
    let ticket = r.ticket_key.clone().map(|key| {
        // If the ticket is in the inbox, the link leads there.
        let id = store
            .ws
            .with_untracked(|w| w.item_by_key(&key).map(|i| i.id.clone()));
        match id {
            Some(id) => view! {
                <a
                    class="ui-link t-subheadline"
                    href="#/"
                    on:click=move |ev| {
                        ev.prevent_default();
                        store.open_item(&id);
                    }
                >
                    {t!(ReviewDiffOpenTicket, key = key)}
                </a>
            }
            .into_any(),
            None => {
                view! { <span class="t-subheadline t-secondary">{t!(ReviewDiffTicket, key = key)}</span> }
                    .into_any()
            }
        }
    });
    let body = panel_body(store, r.clone(), ui, panel);
    view! {
        <Pane label=t!(ReviewPane)>
            <PaneHeader>
                <div class="ui-hstack" style="--spacing: 10px">
                    <Badge variant=BadgeVariant::Neutral>"GitHub"</Badge>
                    <span class="t-subheadline t-mono t-secondary">{r.key().to_string()}</span>
                    <Status tone=view::tone(r.checks.tone())>{checks_label(r.checks)}</Status>
                    {r
                        .merged
                        .map(|m| {
                            view! {
                                <Status tone=StatusTone::Indigo>{t!(ReviewMerged, method = m.label())}</Status>
                            }
                        })}
                    <span class="ui-spacer"></span>
                    <span class="ui-hstack" style="--spacing: 8px; flex-wrap: nowrap">
                        <Avatar initials=r.author.initials.clone() agent=r.author.is_agent() />
                        <span class="t-subheadline">{r.author.name.clone()}</span>
                    </span>
                </div>
                <h1 class="t-title1 t-bold">{r.title.clone()}</h1>
                <div class="ui-hstack" style="--spacing: 8px">
                    <Chip>
                        <Icon path=icons::GIT_BRANCH size=13 stroke=2.2 />
                        {r.branch.clone()}
                    </Chip>
                    <span style="display: inline-flex; color: var(--sys-color-secondary-label)">
                        <Icon path=icons::ARROW_RIGHT size=14 stroke=2.4 />
                    </span>
                    <Chip>{r.base.clone()}</Chip>
                    <span class="ui-spacer"></span>
                    {ticket}
                </div>
                <Markdown
                    text=r.description_msg.as_ref().map_or_else(|| r.description.clone(), |m| m.render(lang()))
                    class="t-body t-secondary"
                />
            </PaneHeader>
            <PaneBody>{body}</PaneBody>
        </Pane>
    }
}
