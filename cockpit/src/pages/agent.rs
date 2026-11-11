//! Ein Agent setzt ein Ticket um: Auftrag, Ablauf in fünf Schritten und Aktivität.

use cockpit_core::{AgentOptions, agent::STEPS};
use feature_ui::*;
use leptos::prelude::*;

use crate::{route::Route, sidebar::AppSidebar, store::use_store};

fn step_state(state: cockpit_core::StepState) -> StepState {
    match state {
        cockpit_core::StepState::Pending => StepState::Pending,
        cockpit_core::StepState::Active => StepState::Active,
        cockpit_core::StepState::Done => StepState::Done,
    }
}

#[component]
pub fn AgentPage(ticket: Option<String>) -> impl IntoView {
    let store = use_store();

    // Ohne Ticket in der Adresse: ein laufender Agent, sonst das erste Ticket ohne
    // Pull Request, sonst der letzte Lauf.
    let key = ticket.or_else(|| {
        store.ws.with_untracked(|w| {
            let running = w
                .runs
                .iter()
                .find(|r| r.is_active())
                .map(|r| r.ticket_key.clone());
            let open = w
                .items
                .iter()
                .find(|i| i.is_ticket() && i.pr.is_none())
                .map(|i| i.key.clone());
            running
                .or(open)
                .or(w.runs.last().map(|r| r.ticket_key.clone()))
        })
    });
    let key = StoredValue::new(key.unwrap_or_default());
    let item = Memo::new(move |_| store.ws.with(|w| w.item_by_key(&key.get_value()).cloned()));

    // Optionen des Auftrags. Läuft der Agent schon, gelten die des Laufs.
    let saved = store
        .ws
        .with_untracked(|w| w.run(&key.get_value()))
        .filter(|r| !r.is_idle());
    let options = saved.map(|r| r.options).unwrap_or_default();
    let run_tests = RwSignal::new(options.run_tests);
    let draft_pr = RwSignal::new(options.draft_pr);
    let hint = RwSignal::new(options.hint);
    let plan_first = RwSignal::new(options.plan_first);
    // Anmerkung für einen neuen Plan.
    let note = RwSignal::new(String::new());
    let tests_allowed = Signal::derive(move || store.conn.get().may_run_tests);
    let chosen = move || AgentOptions {
        run_tests: run_tests.get() && tests_allowed.get(),
        draft_pr: draft_pr.get(),
        hint: hint.get(),
        plan_first: plan_first.get(),
        ..Default::default()
    };

    // Der Lauf, wie er jetzt aussieht. Vor dem Start zeigt er schon die gewählten Optionen.
    let run = Memo::new(move |_| {
        let mut run = store.ws.with(|w| w.run(&key.get_value()))?;
        if run.is_idle() {
            run.options = chosen();
        }
        Some(run)
    });
    let steps = Memo::new(move |_| run.get().map(|r| r.steps()).unwrap_or_default());
    let idle = Signal::derive(move || run.get().is_some_and(|r| r.is_idle()));
    let running = Signal::derive(move || run.get().is_some_and(|r| r.is_running()));
    let awaiting = Signal::derive(move || run.get().is_some_and(|r| r.is_awaiting()));
    let plan = Memo::new(move |_| run.get().and_then(|r| r.plan()));
    let done = Signal::derive(move || run.get().is_some_and(|r| r.is_done()));
    let locked = Signal::derive(move || !idle.get());
    let blocked = Signal::derive(move || store.conn.get().agent_blocked());

    let start = move |_| {
        let options = chosen();
        store.ws.update(|w| {
            let _ = w.start_agent(&key.get_value(), options);
        });
    };
    let cancel = move |_| store.ws.update(|w| w.cancel_agent(&key.get_value()));
    let approve = move |_| {
        store.ws.update(|w| {
            w.approve_plan(&key.get_value());
        });
        note.set(String::new());
    };
    let reject = move |_| {
        store.ws.update(|w| {
            w.reject_plan(&key.get_value());
        });
        note.set(String::new());
    };
    let note_empty = Signal::derive(move || note.with(|n| n.trim().is_empty()));
    let revise = move |_| {
        let text = note.get_untracked();
        let mut ok = false;
        store
            .ws
            .update(|w| ok = w.revise_plan(&key.get_value(), &text));
        if ok {
            note.set(String::new());
        }
    };
    let to_ticket = move |_| {
        if let Some(item) = item.get_untracked() {
            store.open_item(&item.id);
        }
    };

    // Läufe für die Sidebar. Das gewählte Ticket steht dabei, auch wenn es noch keinen Lauf hat.
    let runs = move || {
        let current = key.get_value();
        let mut rows: Vec<(String, String, &'static str)> = store.ws.with(|w| {
            w.runs
                .iter()
                .map(|r| {
                    let state = if r.is_awaiting() {
                        "wartet"
                    } else if r.is_running() {
                        "läuft"
                    } else if r.is_done() {
                        "fertig"
                    } else {
                        "bereit"
                    };
                    (r.ticket_key.clone(), r.ticket_title.clone(), state)
                })
                .collect()
        });
        if !rows.iter().any(|(k, _, _)| *k == current)
            && let Some(item) = item.get()
        {
            rows.insert(0, (item.key, item.title, "neu"));
        }
        rows.into_iter()
            .map(|(k, title, state)| {
                view! {
                    <SidebarItem
                        href=Route::Agent(Some(k.clone())).hash()
                        label=format!("{k} {title}")
                        current=k == current
                        count=state
                    />
                }
            })
            .collect_view()
    };

    let timeline = move || {
        (0..STEPS)
        .map(|i| {
            let step = move || steps.get().get(i).cloned();
            view! {
                <TimelineStep
                    number=i + 1
                    state=Signal::derive(move || step().map(|s| step_state(s.state)).unwrap_or_default())
                    title=Signal::derive(move || step().map(|s| s.title).unwrap_or_default())
                    text=Signal::derive(move || step().map(|s| s.text).unwrap_or_default())
                />
            }
        })
        .collect_view()
    };

    let status = move || {
        if done.get() {
            ("Fertig", StatusTone::Green)
        } else if awaiting.get() {
            ("Wartet auf Freigabe", StatusTone::Orange)
        } else if running.get() {
            ("Agent arbeitet", StatusTone::Blue)
        } else {
            ("Bereit", StatusTone::Gray)
        }
    };

    // Ergebnis-Karte, sobald der Pull Request steht.
    let result = move || {
        let run = run.get().filter(|r| r.is_done())?;
        let number = run.pr_number?;
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
    };

    view! {
        <SplitView layout=SplitLayout::DetailWide>
            <AppSidebar>
                <SidebarSection label="Agenten-Läufe">{runs}</SidebarSection>
            </AppSidebar>

            <Show
                when=move || run.get().is_some()
                fallback=|| {
                    view! {
                        <Pane label="Agent-Auftrag">
                            <PaneHeader>
                                <h1 class="t-title1 t-bold">"Kein Ticket für den Agenten"</h1>
                                <p class="t-subheadline t-secondary">
                                    "Agenten setzen Jira-Tickets um. Wähle im Dashboard ein Ticket und beauftrage den Agenten dort."
                                </p>
                                <div class="ui-hstack">
                                    <Button variant=ButtonStyle::GlassProminent size=ControlSize::Large href="#/">
                                        "Zum Dashboard"
                                    </Button>
                                </div>
                            </PaneHeader>
                        </Pane>
                    }
                }
            >
                <Pane label="Agent-Auftrag">
                    <PaneHeader>
                        <div class="ui-hstack" style="--spacing: 10px">
                            <Badge variant=BadgeVariant::Neutral>"Jira"</Badge>
                            <span class="t-subheadline t-mono t-secondary">{key.get_value()}</span>
                            <span class="ui-spacer"></span>
                            <Status tone=Signal::derive(move || status().1)>{move || status().0}</Status>
                        </div>
                        <h1 class="t-title1 t-bold">{move || item.get().map(|i| i.title)}</h1>
                        <Progress
                            value=Signal::derive(move || run.get().map_or(0, |r| r.completed))
                            max=STEPS
                            label="Fortschritt"
                        />
                    </PaneHeader>

                    <PaneBody>
                        <div class="ui-list__section">
                            <p class="ui-list__header">"Auftrag"</p>
                            <div class="ui-vstack" style="--spacing: 10px">
                                <Textarea
                                    value=hint
                                    label="Hinweise für den Agenten"
                                    placeholder="Zusätzliche Hinweise für den Agenten (optional), z. B.: Auch den Slider-Thumb prüfen."
                                    min_height="72px"
                                    disabled=locked
                                />
                                <List>
                                    <ListSection nested=true>
                                        <ListRow title="Basis-Branch">
                                            <Chip>"main"</Chip>
                                        </ListRow>
                                        <ListRow title="Tests vor dem Pull Request ausführen" toggle_row=true>
                                            <Show when=move || !tests_allowed.get()>
                                                <span class="t-footnote t-secondary">"In den Einstellungen aus"</span>
                                            </Show>
                                            <Toggle
                                                checked=run_tests
                                                disabled=Signal::derive(move || locked.get() || !tests_allowed.get())
                                            />
                                        </ListRow>
                                        <ListRow title="Pull Request als Entwurf anlegen" toggle_row=true>
                                            <Toggle checked=draft_pr disabled=locked />
                                        </ListRow>
                                        <ListRow title="Plan vor dem Start zeigen" toggle_row=true>
                                            <Toggle checked=plan_first disabled=locked />
                                        </ListRow>
                                    </ListSection>
                                </List>
                            </div>
                        </div>

                        <div class="ui-hstack" style="--spacing: 10px">
                            <Show when=move || idle.get()>
                                <Button
                                    variant=ButtonStyle::GlassProminent
                                    size=ControlSize::Large
                                    disabled=Signal::derive(move || blocked.get().is_some())
                                    on_click=start
                                >
                                    <Icon path=icons::SPARKLES />
                                    "Agent starten"
                                </Button>
                            </Show>
                            <Show when=move || awaiting.get()>
                                <Button variant=ButtonStyle::GlassProminent size=ControlSize::Large on_click=approve>
                                    "Plan freigeben"
                                </Button>
                                <Button size=ControlSize::Large destructive=true on_click=reject>
                                    "Plan ablehnen"
                                </Button>
                            </Show>
                            <Show when=move || running.get()>
                                <Button size=ControlSize::Large destructive=true on_click=cancel>
                                    "Abbrechen"
                                </Button>
                            </Show>
                            {move || {
                                let number = run.get().filter(|r| r.is_done())?.pr_number?;
                                Some(
                                    view! {
                                        <Button
                                            variant=ButtonStyle::GlassProminent
                                            size=ControlSize::Large
                                            href=Route::Review(Some(number)).hash()
                                        >
                                            "Pull Request prüfen"
                                        </Button>
                                    },
                                )
                            }}
                            <Button variant=ButtonStyle::Borderless size=ControlSize::Large on_click=to_ticket>
                                "Zum Ticket"
                            </Button>
                        </div>
                        {move || {
                            blocked
                                .get()
                                .filter(|_| idle.get())
                                .map(|reason| {
                                    view! {
                                        <p class="t-footnote t-secondary">
                                            {reason} " "
                                            <a class="ui-link" href=Route::Setup.hash()>"Einstellungen öffnen"</a>
                                        </p>
                                    }
                                })
                        }}

                        {move || {
                            let plan = plan.get()?;
                            let waiting = awaiting.get();
                            Some(
                                view! {
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
                                },
                            )
                        }}

                        <div class="ui-list__section">
                            <p class="ui-list__header">"Ablauf"</p>
                            <Timeline>{timeline()}</Timeline>
                        </div>
                    </PaneBody>
                </Pane>

                <Pane label="Aktivität">
                    <PaneHeader>
                        <h2 class="t-title3 t-bold">"Aktivität"</h2>
                    </PaneHeader>
                    <PaneBody>
                        <Log text=Signal::derive(move || run.get().map(|r| r.log()).unwrap_or_default()) />
                        {move || {
                            // Sobald der Agent an den Dateien arbeitet, erscheinen sie hier.
                            let run = run.get().filter(|r| r.completed >= 2)?;
                            let files = store.ws.with(|w| {
                                run.pr_number.and_then(|n| w.review(n)).map(|r| r.files.clone())
                            });
                            let files = files.unwrap_or_else(|| cockpit_core::demo::agent_changes(&run.ticket_key));
                            Some(
                                view! {
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
                                },
                            )
                        }}
                        {result}
                    </PaneBody>
                </Pane>
            </Show>
        </SplitView>
    }
}
