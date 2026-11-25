use super::*;

/// Short state of a single check.
pub(super) fn check_state(status: Checks) -> (String, StatusTone) {
    let label = match status {
        Checks::Passed => t!(ReviewChecksPassed),
        Checks::Failed => t!(ReviewChecksFailed),
        Checks::Running => t!(ReviewChecksRunning),
        Checks::Unknown => t!(ReviewChecksUnknown),
    };
    (label, view::tone(status.tone()))
}

/// State of all checks together.
pub(super) fn checks_label(status: Checks) -> String {
    match status {
        Checks::Passed => t!(ReviewChecksAllPassed),
        Checks::Failed => t!(ReviewChecksAllFailed),
        Checks::Running => t!(ReviewChecksAllRunning),
        Checks::Unknown => t!(ReviewChecksAllUnknown),
    }
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
                <div class="ui-vstack ui-gap-8" role="group" aria-label=c.name.clone()>
                    <div class="ui-hstack ui-gap-8 ui-justify-between">
                        <span class="t-subheadline t-semibold">{c.name.clone()}</span>
                        <span class="t-footnote t-secondary t-mono">{c.duration_msg().render(lang())}</span>
                    </div>
                    <div class="ui-hstack ui-gap-8">
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
                            {move || if open() { t!(ReviewChecksHideLog) } else { t!(ReviewChecksShowLog) }}
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
                                        {t!(ReviewChecksRestart)}
                                    </Button>
                                }
                            })}
                    </div>
                    {move || open().then(|| view! { <Log text=log.get_value() /> })}
                </div>
            }
        })
        .collect_view();
    let empty = r.check_runs.is_empty();
    let unknown = r.checks == Checks::Unknown;
    view! {
        <div class="ui-vstack ui-gap-10">
            <h3 class="t-headline">{t!(ReviewChecksTitle)}</h3>
            {empty
                .then(|| {
                    let text = if unknown { t!(ReviewChecksUnknownHint) } else { t!(ReviewChecksNoDetails) };
                    view! { <p class="t-footnote t-secondary">{text}</p> }
                })}
            {rows}
        </div>
    }
}
