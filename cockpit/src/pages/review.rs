//! Pull-Request-Review: in der Mitte der Diff, rechts die Kommentare, die der Agent
//! vorbereitet hat. Gesendet wird erst, wenn über jeden Vorschlag entschieden ist.

use std::time::Duration;

use cockpit_core::{Action, Decision, Level, Review, Suggestion, Verdict};
use feature_ui::*;
use leptos::prelude::*;

use crate::{
    route::Route,
    sidebar::AppSidebar,
    store::{Store, use_store},
    view,
};

/// So lange „liest“ der Demo-Agent einen Pull Request.
const REVIEW_TIME: Duration = Duration::from_millis(1200);

#[component]
pub fn ReviewPage(number: Option<u32>) -> impl IntoView {
    let store = use_store();
    // Ohne Nummer: das erste Review, das noch nicht gesendet ist.
    let number = number.or_else(|| {
        store.ws.with_untracked(|w| {
            w.reviews
                .iter()
                .find(|r| !r.sent)
                .or(w.reviews.first())
                .map(|r| r.number)
        })
    });
    let review = Memo::new(move |_| number.and_then(|n| store.ws.with(|w| w.review(n).cloned())));

    // Zustand der Zeilenkommentare im Diff.
    let lines = LineUi {
        open: RwSignal::new(None),
        draft: RwSignal::new(String::new()),
        editing: RwSignal::new(None),
        edit_text: RwSignal::new(String::new()),
    };

    // Zustand der rechten Spalte. Er überlebt, wenn sich das Review ändert.
    let panel = Panel {
        own_comment: RwSignal::new(String::new()),
        editing: RwSignal::new(None),
        edit_text: RwSignal::new(String::new()),
        agent_reading: RwSignal::new(false),
    };

    let list = move || {
        store.ws.with(|w| {
            w.reviews
                .iter()
                .map(|r| {
                    view! {
                        <SidebarItem
                            href=Route::Review(Some(r.number)).hash()
                            label=format!("#{} {}", r.number, r.title)
                            current=Some(r.number) == number
                            count=if r.sent { "gesendet" } else { "" }
                        />
                    }
                })
                .collect_view()
        })
    };

    view! {
        <SplitView layout=SplitLayout::DetailWide>
            <AppSidebar>
                <SidebarSection label="Reviews">{list}</SidebarSection>
            </AppSidebar>
            {move || match review.get() {
                Some(r) => view! { {diff_pane(store, r.clone(), lines)} {agent_pane(store, r, panel)} }.into_any(),
                None => {
                    view! {
                        <Pane label="Pull Request">
                            <PaneHeader>
                                <h1 class="t-title1 t-bold">"Kein Pull Request"</h1>
                                <p class="t-subheadline t-secondary">
                                    "Diesen Pull Request gibt es nicht. Wähle links einen anderen."
                                </p>
                            </PaneHeader>
                        </Pane>
                    }
                        .into_any()
                }
            }}
        </SplitView>
    }
}

/// Eingaben der rechten Spalte.
#[derive(Clone, Copy)]
struct Panel {
    own_comment: RwSignal<String>,
    /// ID des Vorschlags, dessen Text gerade bearbeitet wird.
    editing: RwSignal<Option<String>>,
    edit_text: RwSignal<String>,
    agent_reading: RwSignal<bool>,
}

/// Eingaben der Zeilenkommentare im Diff.
#[derive(Clone, Copy)]
struct LineUi {
    /// Datei und Position der Zeile, an der gerade ein Kommentar entsteht.
    open: RwSignal<Option<(String, usize)>>,
    draft: RwSignal<String>,
    /// ID des eigenen Kommentars, der gerade bearbeitet wird.
    editing: RwSignal<Option<String>>,
    edit_text: RwSignal<String>,
}

/// Inhalt unter einer Diff-Zeile: eigene Kommentare und, bei Auswahl, das Eingabefeld.
fn line_notes(store: Store, r: &Review, file: &str, index: usize, ui: LineUi) -> AnyView {
    let (number, sent) = (r.number, r.sent);
    let comments = r
        .line_comments_at(file, index)
        .into_iter()
        .map(|c| {
            let (id, text) = (StoredValue::new(c.id.clone()), StoredValue::new(c.text.clone()));
            let editing = move || ui.editing.get().is_some_and(|e| e == id.get_value());
            view! {
                <div class="ui-diff__note" role="group" aria-label="Eigener Kommentar">
                    {move || {
                        if editing() && !sent {
                            view! {
                                <Textarea value=ui.edit_text label="Kommentar bearbeiten" min_height="64px" />
                                <div class="ui-hstack" style="--spacing: 8px">
                                    <Button
                                        variant=ButtonStyle::BorderedProminent
                                        size=ControlSize::Small
                                        disabled=Signal::derive(move || ui.edit_text.get().trim().is_empty())
                                        on_click=move |_| {
                                            let new_text = ui.edit_text.get_untracked();
                                            ui.editing.set(None);
                                            let _ = store.dispatch(Action::EditLineComment {
                                                review: number,
                                                comment: id.get_value(),
                                                text: new_text,
                                            });
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
                                <span class="t-footnote t-secondary">"Dein Kommentar"</span>
                                <p class="t-subheadline">{text.get_value()}</p>
                                {(!sent)
                                    .then(|| {
                                        view! {
                                            <div class="ui-hstack" style="--spacing: 8px">
                                                <Button
                                                    variant=ButtonStyle::Borderless
                                                    size=ControlSize::Small
                                                    on_click=move |_| {
                                                        ui.edit_text.set(text.get_value());
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
                    <div class="ui-hstack" style="--spacing: 8px">
                        <Button
                            variant=ButtonStyle::BorderedProminent
                            size=ControlSize::Small
                            disabled=Signal::derive(move || ui.draft.get().trim().is_empty())
                            on_click=move |_| {
                                let text = ui.draft.get_untracked();
                                let added = store.dispatch(Action::AddLineComment {
                                    review: number,
                                    file: file.get_value(),
                                    line: index,
                                    text,
                                });
                                if added.is_ok() {
                                    ui.draft.set(String::new());
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
fn diff_pane(store: Store, r: Review, ui: LineUi) -> impl IntoView {
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

/// Rechts: Vorschläge des Agenten, eigener Kommentar, Urteil und Senden.
fn agent_pane(store: Store, r: Review, panel: Panel) -> impl IntoView {
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
                // Die Seite kann inzwischen verlassen worden sein.
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
            </PaneBody>
        </Pane>
    }
}

/// Ein Vorschlag: freigeben, ablehnen, bearbeiten – oder die Entscheidung zurücknehmen.
fn suggestion(store: Store, number: u32, s: Suggestion, panel: Panel) -> impl IntoView {
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
