use super::*;

/// Vorher/Nachher eines Änderungsvorschlags als kleiner Diff.
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
    view! { <Diff rows=rows label="Änderungsvorschlag" /> }
}

/// Schalter und Feld für den Ersatz-Code. Beim Einschalten steht die alte Zeile darin.
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
                {move || if on.get() { "Ohne Änderungsvorschlag" } else { "Änderung vorschlagen" }}
            </Button>
        </div>
        {move || {
            on.get()
                .then(|| {
                    view! {
                        <Textarea
                            value=code
                            label="Ersatz-Code für diese Zeile"
                            placeholder="So soll die Zeile aussehen …"
                            min_height="64px"
                        />
                    }
                })
        }}
    }
}

/// Inhalt unter einer Diff-Zeile: eigene Kommentare und, bei Auswahl, das Eingabefeld.
pub(super) fn line_notes(
    store: Store,
    r: &Review,
    file: &str,
    index: usize,
    ui: LineUi,
) -> AnyView {
    let (number, sent) = (r.number, r.sent);
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
            // Ein umgesetzter Vorschlag steht schon im Diff, der Vorher/Nachher entfällt.
            let diff = r.suggestion_diff(c).filter(|_| c.is_open());
            let original = original.clone();
            // Fremde Kommentare lassen sich nicht ändern, erledigte auch nicht.
            let (own, open) = (c.is_own(), c.is_open());
            let resolved = c.resolved.clone();
            let author = c
                .author
                .as_ref()
                .map_or_else(|| "Dein Kommentar".to_string(), |p| format!("Kommentar von {}", p.name));
            let label = if diff.is_some() { format!("{author} · Änderungsvorschlag") } else { author };
            let editing = move || ui.editing.get().is_some_and(|e| e == id.get_value());
            view! {
                <div class="ui-diff__note" role="group" aria-label="Eigener Kommentar">
                    {move || {
                        if editing() && !sent && own && open {
                            view! {
                                <Textarea value=ui.edit_text label="Kommentar bearbeiten" min_height="64px" />
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
                                                review: number,
                                                comment: id.get_value(),
                                                text: new_text,
                                                suggestion,
                                            });
                                            // Bei einem Fehler (z. B. Ersatz = alte Zeile) bleibt das Feld offen.
                                            if saved.is_ok() {
                                                ui.editing.set(None);
                                            }
                                        }
                                    >
                                        "Sichern"
                                    </Button>
                                    <Button
                                        variant=ButtonStyle::Borderless
                                        size=ControlSize::Small
                                        on_click=move |_| ui.editing.set(None)
                                    >
                                        "Abbrechen"
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
                                                <Status tone=StatusTone::Green>"Erledigt"</Status>
                                                <a class="ui-link" href=Route::Agent(Some(run.clone())).hash()>
                                                    {format!("Lauf {run}")}
                                                </a>
                                            }
                                        })}
                                </span>
                                {(!text.with_value(String::is_empty))
                                    .then(|| view! { <p class="t-subheadline">{text.get_value()}</p> })}
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
                                                    "Bearbeiten"
                                                </Button>
                                                <Button
                                                    variant=ButtonStyle::Borderless
                                                    size=ControlSize::Small
                                                    destructive=true
                                                    on_click=move |_| {
                                                        let _ = store.dispatch(Action::DeleteLineComment {
                                                            review: number,
                                                            comment: id.get_value(),
                                                        });
                                                    }
                                                >
                                                    "Löschen"
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
                        label="Kommentar zu dieser Zeile"
                        placeholder="Kommentar zu dieser Zeile …"
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
                                    review: number,
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
                            "Hinzufügen"
                        </Button>
                        <Button
                            variant=ButtonStyle::Borderless
                            size=ControlSize::Small
                            on_click=move |_| ui.open.set(None)
                        >
                            "Abbrechen"
                        </Button>
                    </div>
                </div>
            }
        })
    };
    view! {
        {comments}
        {composer}
    }
    .into_any()
}

/// Mitte: Kopf des Pull Requests und der Diff.
pub(super) fn diff_pane(store: Store, r: Review, ui: LineUi) -> impl IntoView {
    let ticket = r.ticket_key.clone().map(|key| {
        // Liegt das Ticket in der Inbox, führt der Link dorthin.
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
                    {format!("Ticket {key} öffnen")}
                </a>
            }
            .into_any(),
            None => {
                view! { <span class="t-subheadline t-secondary">{format!("Ticket {key}")}</span> }
                    .into_any()
            }
        }
    });
    let sent = r.sent;
    let files = r
        .files
        .iter()
        .map(|file| {
            let name = StoredValue::new(file.name.clone());
            let selected = Signal::derive(move || {
                ui.open.with(|o| {
                    o.as_ref()
                        .filter(|(f, _)| *f == name.get_value())
                        .map(|(_, i)| *i)
                })
            });
            // Nach dem Senden ist nichts mehr zu kommentieren; die Kommentare bleiben sichtbar.
            let on_select = (!sent).then(|| {
                Callback::new(move |index: usize| {
                    ui.editing.set(None);
                    ui.suggest.set(false);
                    let same = ui.open.with_untracked(|o| {
                        o.as_ref()
                            .is_some_and(|(f, i)| *f == name.get_value() && *i == index)
                    });
                    ui.open.set((!same).then(|| (name.get_value(), index)));
                })
            });
            let notes = {
                let r = r.clone();
                Callback::new(move |index: usize| {
                    line_notes(store, &r, &name.get_value(), index, ui)
                })
            };
            view! {
                <div class="ui-vstack" style="--spacing: 8px">
                    <div class="ui-hstack" style="justify-content: space-between">
                        <span class="t-subheadline t-semibold t-mono">{file.name.clone()}</span>
                        <span class="t-footnote t-secondary t-mono">{file.stat()}</span>
                    </div>
                    <Diff
                        rows=view::diff_rows(file)
                        label=file.name.clone()
                        on_select=on_select
                        selected=selected
                        below=Some(notes)
                    />
                </div>
            }
        })
        .collect_view();

    view! {
        <Pane label="Pull Request">
            <PaneHeader>
                <div class="ui-hstack" style="--spacing: 10px">
                    <Badge variant=BadgeVariant::Neutral>"GitHub"</Badge>
                    <span class="t-subheadline t-mono t-secondary">{format!("#{}", r.number)}</span>
                    <Status tone=view::tone(r.checks.tone())>{r.checks.label()}</Status>
                    {r
                        .merged
                        .map(|m| {
                            view! {
                                <Status tone=StatusTone::Indigo>{format!("Gemergt ({})", m.label())}</Status>
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
                <p class="t-body t-secondary">{r.description.clone()}</p>
            </PaneHeader>
            <PaneBody>{files}</PaneBody>
        </Pane>
    }
}
