use super::*;

/// Short state of a single check.
pub(super) fn check_state(status: Checks) -> (&'static str, StatusTone) {
    let label = match status {
        Checks::Passed => "Bestanden",
        Checks::Failed => "Fehlgeschlagen",
        Checks::Running => "Läuft",
    };
    (label, view::tone(status.tone()))
}

/// The checks of the pull request: name, state, duration; log to expand, restart red ones.
pub(super) fn checks_section(store: Store, r: &Review, panel: Panel) -> impl IntoView + use<> {
    let number = StoredValue::new(r.key());
    let merged = r.merged.is_some();
    let rows = r
        .check_runs
        .iter()
        .map(|c| {
            let name = StoredValue::new(c.name.clone());
            let log = StoredValue::new(c.log.clone());
            let (label, tone) = check_state(c.status);
            let open = move || panel.open_check.get().is_some_and(|n| n == name.get_value());
            let restartable = c.status == Checks::Failed && !merged;
            view! {
                <div class="ui-vstack" style="--spacing: 8px" role="group" aria-label=c.name.clone()>
                    <div class="ui-hstack" style="--spacing: 8px; justify-content: space-between">
                        <span class="t-subheadline t-semibold">{c.name.clone()}</span>
                        <span class="t-footnote t-secondary t-mono">{c.duration_label()}</span>
                    </div>
                    <div class="ui-hstack" style="--spacing: 8px">
                        <Status tone=tone>{label}</Status>
                        <span class="ui-spacer"></span>
                        <Button
                            variant=ButtonStyle::Borderless
                            size=ControlSize::Small
                            on_click=move |_| {
                                let now_open = open();
                                panel.open_check.set((!now_open).then(|| name.get_value()));
                            }
                        >
                            {move || if open() { "Log ausblenden" } else { "Log ansehen" }}
                        </Button>
                        {restartable
                            .then(|| {
                                view! {
                                    <Button
                                        size=ControlSize::Small
                                        on_click=move |_| {
                                            let _ = store.dispatch(Action::RestartCheck {
                                                review: number.get_value(),
                                                check: name.get_value(),
                                            });
                                        }
                                    >
                                        "Neu starten"
                                    </Button>
                                }
                            })}
                    </div>
                    {move || open().then(|| view! { <Log text=log.get_value() /> })}
                    {(c.status == Checks::Failed && !merged)
                        .then(|| investigation(store, number, name.get_value(), !c.log.trim().is_empty()))}
                </div>
            }
        })
        .collect_view();
    let empty = r.check_runs.is_empty();
    view! {
        <div class="ui-vstack" style="--spacing: 10px">
            <h3 class="t-headline">"Checks"</h3>
            {empty
                .then(|| {
                    view! { <p class="t-footnote t-secondary">"Zu diesem Pull Request gibt es keine Details."</p> }
                })}
            {rows}
        </div>
    }
}

/// The follow-up job to this check, if there is one.
pub(super) fn diagnose_run(
    store: Store,
    number: StoredValue<PrKey>,
    check: &str,
) -> Option<AgentRun> {
    store.ws.with(|w| {
        w.followups(number.get_value())
            .into_iter()
            .find(|r| matches!(&r.kind, RunKind::Diagnose { check: c, .. } if c == check))
            .cloned()
    })
}

/// Under a red check: "Agent untersuchen lassen", the state of the investigation and the
/// diagnosis with "Fix übernehmen" and "Verwerfen". Nothing is adopted without you.
pub(super) fn investigation(
    store: Store,
    number: StoredValue<PrKey>,
    check: String,
    has_log: bool,
) -> impl IntoView {
    let check = StoredValue::new(check);
    let blocked = Signal::derive(move || store.conn.get().agent_blocked().is_some());
    move || {
        let run = diagnose_run(store, number, &check.get_value()).filter(|r| !r.is_idle());
        let key = run.as_ref().map(AgentRun::key);
        match run {
            Some(r) if r.is_diagnosed() => {
                let key = StoredValue::new(key.unwrap_or_default());
                let d = r.diagnosis.clone();
                let changes = d.as_ref().is_some_and(|d| d.changes_code());
                view! {
                    <SuggestionCard state=SuggestionState::Pending>
                        <div class="ui-hstack" style="--spacing: 8px">
                            <Badge variant=BadgeVariant::Tint>"Diagnose"</Badge>
                            <span class="ui-spacer"></span>
                            <a class="ui-link t-footnote" href=Route::Agent(Some(key.get_value())).hash()>
                                "Lauf ansehen"
                            </a>
                        </div>
                        <p class="t-subheadline">
                            {d.as_ref().map(|d| d.cause.clone()).unwrap_or_default()}
                        </p>
                        {d
                            .as_ref()
                            .and_then(|d| d.location())
                            .map(|l| view! { <span class="t-footnote t-mono t-secondary">{l}</span> })}
                        <p class="t-footnote t-secondary">
                            "Vorschlag: " {d.as_ref().map(|d| d.remedy.clone()).unwrap_or_default()}
                        </p>
                        {d
                            .and_then(|d| d.patch)
                            .map(|p| view! { <Diff rows=view::diff_rows(&p) label=format!("Fix für {}", p.name) /> })}
                        <div class="ui-hstack" style="--spacing: 8px">
                            <Button
                                variant=ButtonStyle::BorderedProminent
                                size=ControlSize::Small
                                on_click=move |_| {
                                    let _ = store.dispatch(Action::ApplyFix { run: key.get_value() });
                                }
                            >
                                {if changes { "Fix übernehmen" } else { "Check neu starten" }}
                            </Button>
                            <Button
                                size=ControlSize::Small
                                destructive=true
                                on_click=move |_| {
                                    let _ = store.dispatch(Action::DiscardFix { run: key.get_value() });
                                }
                            >
                                "Verwerfen"
                            </Button>
                        </div>
                    </SuggestionCard>
                }
                .into_any()
            }
            Some(r) if r.is_active() => {
                let key = key.unwrap_or_default();
                view! {
                    <p class="t-footnote t-secondary">
                        "Der Agent untersucht den Check … "
                        <a class="ui-link" href=Route::Agent(Some(key)).hash()>"Lauf ansehen"</a>
                    </p>
                }
                .into_any()
            }
            _ if has_log => view! {
                <div class="ui-hstack">
                    <Button
                        size=ControlSize::Small
                        disabled=blocked
                        on_click=move |_| {
                            let _ = store.dispatch(Action::InvestigateCheck {
                                review: number.get_value(),
                                check: check.get_value(),
                            });
                        }
                    >
                        <Icon path=icons::SPARKLES />
                        "Agent untersuchen lassen"
                    </Button>
                </div>
            }
            .into_any(),
            _ => ().into_any(),
        }
    }
}
