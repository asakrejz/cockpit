use super::*;

/// Review-Kommentare umsetzen: offene Kommentare auswählen und den Agenten beauftragen.
/// Dazu der Stand eines laufenden Folgeauftrags und das, was schon umgesetzt wurde.
pub(super) fn comments_section(store: Store, r: &Review, panel: Panel) -> impl IntoView + use<> {
    let number = r.number;
    let merged = r.merged.is_some();
    move || {
        let (review, active, done) = store.ws.with(|w| {
            let runs = w.followups(number);
            (
                w.review(number).cloned(),
                w.active_followup(number).cloned(),
                runs.into_iter()
                    .filter(|r| r.is_done())
                    .cloned()
                    .collect::<Vec<_>>(),
            )
        });
        let review = review?;
        let open: Vec<_> = review.open_comments().into_iter().cloned().collect();
        if open.is_empty() && active.is_none() && done.is_empty() {
            return None;
        }
        let state = active.map(|run| {
            let (label, tone) = if run.is_awaiting() {
                ("Wartet auf Freigabe des Plans", StatusTone::Orange)
            } else if run.is_asking() {
                ("Wartet auf deine Antwort", StatusTone::Orange)
            } else if run.is_diagnosed() {
                ("Wartet auf deine Entscheidung", StatusTone::Orange)
            } else if run.is_paused() {
                ("Pausiert", StatusTone::Gray)
            } else {
                ("Agent arbeitet", StatusTone::Blue)
            };
            let key = run.key();
            view! {
                <div class="ui-hstack" style="--spacing: 8px">
                    <Status tone=tone>{label}</Status>
                    <span class="t-footnote t-secondary">{run.label()}</span>
                    <span class="ui-spacer"></span>
                    <a class="ui-link t-footnote" href=Route::Agent(Some(key)).hash()>"Lauf ansehen"</a>
                </div>
            }
        });
        let busy = state.is_some();
        let finished = done
            .into_iter()
            .map(|run| {
                let key = run.key();
                view! {
                    <div class="ui-hstack" style="--spacing: 8px">
                        <Status tone=StatusTone::Green>"Erledigt"</Status>
                        <span class="t-footnote t-secondary">{run.label()}</span>
                        <span class="ui-spacer"></span>
                        <a class="ui-link t-footnote" href=Route::Agent(Some(key)).hash()>"Lauf ansehen"</a>
                    </div>
                }
            })
            .collect_view();
        let rows = open
            .iter()
            .map(|c| {
                let id = c.id.clone();
                let on = RwSignal::new(!panel.deselected.with_untracked(|d| d.contains(&id)));
                let id2 = id.clone();
                Effect::new(move |prev: Option<()>| {
                    let checked = on.get();
                    if prev.is_some() {
                        panel.deselected.update(|d| {
                            d.retain(|x| *x != id2);
                            if !checked {
                                d.push(id2.clone());
                            }
                        });
                    }
                });
                let who = c.author.as_ref().map_or("Du".to_string(), |p| p.name.clone());
                let place = review.line_location(c).unwrap_or_else(|| c.file.clone());
                let text = if c.text.is_empty() { "Änderungsvorschlag".to_string() } else { c.text.clone() };
                view! {
                    <label class="ui-hstack ck-pick" style="--spacing: 10px; align-items: flex-start">
                        <Toggle checked=on disabled=busy aria_label=format!("Kommentar {id} umsetzen") />
                        <span class="ui-vstack" style="--spacing: 2px; min-width: 0">
                            <span class="t-footnote t-secondary t-mono">{format!("{who} · {place}")}</span>
                            <span class="t-subheadline">{text}</span>
                        </span>
                    </label>
                }
            })
            .collect_view();
        let ids: Vec<String> = open.iter().map(|c| c.id.clone()).collect();
        let count = Signal::derive({
            let ids = ids.clone();
            move || {
                panel
                    .deselected
                    .with(|d| ids.iter().filter(|i| !d.contains(*i)).count())
            }
        });
        let blocked = Signal::derive(move || store.conn.get().agent_blocked().is_some());
        let start = {
            let ids = ids.clone();
            move |_| {
                let chosen: Vec<String> = panel
                    .deselected
                    .with_untracked(|d| ids.iter().filter(|i| !d.contains(*i)).cloned().collect());
                let options = AgentOptions {
                    run_tests: store.conn.get_untracked().may_run_tests,
                    ..Default::default()
                };
                let started = store.dispatch(Action::AddressReview {
                    review: number,
                    comments: chosen,
                    options,
                });
                // Der Plan liegt zur Freigabe auf der Agent-Seite.
                if started.is_ok() {
                    crate::store::navigate(&Route::Agent(Some(format!("review-{number}"))));
                }
            }
        };
        let can_start = !open.is_empty() && !merged;
        Some(view! {
            <div class="ui-vstack" style="--spacing: 10px" role="region" aria-label="Review-Kommentare umsetzen">
                <h3 class="t-headline">"Review-Kommentare umsetzen"</h3>
                {state}
                {finished}
                {(can_start && !open.is_empty())
                    .then(|| {
                        view! {
                            <p class="t-footnote t-secondary">
                                "Der Agent setzt die gewählten Kommentare um und pusht auf denselben Branch – ohne neuen Pull Request."
                            </p>
                            <div class="ui-vstack" style="--spacing: 8px">{rows}</div>
                            <Button
                                variant=ButtonStyle::BorderedProminent
                                size=ControlSize::Large
                                disabled=Signal::derive(move || busy || blocked.get() || count.get() == 0)
                                on_click=start
                            >
                                <Icon path=icons::SPARKLES />
                                {move || format!("Agent: {} umsetzen", match count.get() {
                                    1 => "1 Kommentar".to_string(),
                                    n => format!("{n} Kommentare"),
                                })}
                            </Button>
                        }
                    })}
            </div>
        })
    }
}
