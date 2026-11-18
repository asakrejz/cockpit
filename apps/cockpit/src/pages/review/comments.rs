use super::*;

/// Implement review comments: choose open comments and commission the agent.
/// Plus the state of a running follow-up job and what has already been implemented.
pub(super) fn comments_section(store: Store, r: &Review, panel: Panel) -> impl IntoView + use<> {
    let number = StoredValue::new(r.key());
    let merged = r.merged.is_some();
    move || {
        let (review, active, done) = store.ws.with(|w| {
            let runs = w.followups(number.get_value());
            (
                w.review(number.get_value()).cloned(),
                w.active_followup(number.get_value()).cloned(),
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
                (t!(ReviewStateAwaiting), StatusTone::Orange)
            } else if run.is_asking() {
                (t!(ReviewStateAsking), StatusTone::Orange)
            } else if run.is_diagnosed() {
                (t!(ReviewStateDiagnosed), StatusTone::Orange)
            } else if run.is_paused() {
                (t!(ReviewStatePaused), StatusTone::Gray)
            } else {
                (t!(ReviewStateWorking), StatusTone::Blue)
            };
            let key = run.key();
            view! {
                <div class="ui-hstack" style="--spacing: 8px">
                    <Status tone=tone>{label}</Status>
                    <span class="t-footnote t-secondary">{run.label_msg().render(lang())}</span>
                    <span class="ui-spacer"></span>
                    <a class="ui-link t-footnote" href=Route::Agent(Some(key)).hash()>{t!(ReviewRunView)}</a>
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
                        <Status tone=StatusTone::Green>{t!(ReviewDone)}</Status>
                        <span class="t-footnote t-secondary">{run.label_msg().render(lang())}</span>
                        <span class="ui-spacer"></span>
                        <a class="ui-link t-footnote" href=Route::Agent(Some(key)).hash()>{t!(ReviewRunView)}</a>
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
                let who = c.author.as_ref().map_or_else(|| t!(ReviewYou), |p| p.name.clone());
                let place = review
                    .files
                    .iter()
                    .find(|f| f.name == c.file)
                    .and_then(|f| f.lines.get(c.line))
                    .map(|l| t!(ReviewFileLine, file = c.file, n = l.no))
                    .unwrap_or_else(|| c.file.clone());
                let text = if c.text.is_empty() { t!(ReviewSuggestion) } else { c.text.clone() };
                view! {
                    <label class="ui-hstack ck-pick" style="--spacing: 10px; align-items: flex-start">
                        <Toggle checked=on disabled=busy aria_label=t!(ReviewCommentsToggle, id = id) />
                        <span class="ui-vstack" style="--spacing: 2px; min-width: 0">
                            <span class="t-footnote t-secondary t-mono">{t!(ReviewWhoPlace, who = who, place = place)}</span>
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
                    review: number.get_value(),
                    comments: chosen,
                    options,
                });
                // The plan is waiting for approval on the agent page.
                if started.is_ok() {
                    crate::store::navigate(&Route::Agent(Some(format!(
                        "review-{}",
                        number.get_value().slug()
                    ))));
                }
            }
        };
        let can_start = !open.is_empty() && !merged;
        Some(view! {
            <div class="ui-vstack" style="--spacing: 10px" role="region" aria-label=t!(ReviewCommentsTitle)>
                <h3 class="t-headline">{t!(ReviewCommentsTitle)}</h3>
                {state}
                {finished}
                {(can_start && !open.is_empty())
                    .then(|| {
                        view! {
                            <p class="t-footnote t-secondary">
                                {t!(ReviewCommentsIntro)}
                            </p>
                            <div class="ui-vstack" style="--spacing: 8px">{rows}</div>
                            <Button
                                variant=ButtonStyle::BorderedProminent
                                size=ControlSize::Large
                                disabled=Signal::derive(move || busy || blocked.get() || count.get() == 0)
                                on_click=start
                            >
                                <Icon path=icons::SPARKLES />
                                {move || t!(ReviewCommentsStart, n = count.get())}
                            </Button>
                        }
                    })}
            </div>
        })
    }
}
