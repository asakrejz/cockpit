use super::*;

/// Follow-up jobs of the **real** agent on the pull request: implement the review comments and
/// fix red checks. The backend reads the comments and the check output from GitHub; the agent
/// continues on the same `claude/…` branch and the backend pushes the new commits – no new pull
/// request, no merge. Only for pull requests on a branch of the agent.
pub(super) fn followup_section(store: Store, r: &Review) -> impl IntoView + use<> {
    let number = StoredValue::new(r.key());
    let failed = r
        .check_runs
        .iter()
        .filter(|c| c.status == Checks::Failed)
        .count();
    // The pattern of the repo names the agent's branches (`claude/…` unless it is changed).
    let agent_branch = store.health.with(|h| {
        let repos = h
            .as_ref()
            .map(|h| h.agent.repos.clone())
            .unwrap_or_default();
        cockpit_core::branch::is_agent_branch(
            &repos.branch_pattern(&r.repo),
            &repos.base_branch(&r.repo),
            &r.branch,
        )
    });
    let merged = r.merged.is_some();
    move || {
        let (active, done) = store.ws.with(|w| {
            let jobs = |r: &&AgentRun| matches!(r.kind, RunKind::Followup { .. });
            (
                w.active_followup(number.get_value()).filter(jobs).cloned(),
                w.followups(number.get_value())
                    .into_iter()
                    .filter(|r| r.is_done() && jobs(r))
                    .cloned()
                    .collect::<Vec<_>>(),
            )
        });
        if merged || (!agent_branch && done.is_empty()) {
            return None;
        }
        let busy = active.is_some();
        let blocked = Signal::derive(move || store.conn.get().agent_blocked().is_some());
        let state = active.map(|run| {
            let (label, tone) = if run.is_awaiting() {
                (t!(ReviewStateAwaiting), StatusTone::Orange)
            } else if run.is_asking() {
                (t!(ReviewStateAsking), StatusTone::Orange)
            } else if run.is_paused() {
                (t!(ReviewStatePaused), StatusTone::Gray)
            } else {
                (t!(ReviewStateWorking), StatusTone::Blue)
            };
            let key = run.key();
            view! {
                <div class="ui-hstack" style="--spacing: 8px">
                    <Status tone=tone>{label}</Status>
                    <span class="t-footnote t-secondary">{run.label()}</span>
                    <span class="ui-spacer"></span>
                    <a class="ui-link t-footnote" href=Route::Agent(Some(key)).hash()>{t!(ReviewRunView)}</a>
                </div>
            }
        });
        let finished = done
            .into_iter()
            .map(|run| {
                let key = run.key();
                view! {
                    <div class="ui-hstack" style="--spacing: 8px">
                        <Status tone=StatusTone::Green>{t!(ReviewDone)}</Status>
                        <span class="t-footnote t-secondary">{run.label()}</span>
                        <span class="ui-spacer"></span>
                        <a class="ui-link t-footnote" href=Route::Agent(Some(key)).hash()>{t!(ReviewRunView)}</a>
                    </div>
                }
            })
            .collect_view();
        let options = move || AgentOptions {
            run_tests: store.conn.get_untracked().may_run_tests,
            // Your click is the approval; the agent only pushes to its own branch.
            plan_first: false,
            ..Default::default()
        };
        let threads = move |_| {
            if store
                .dispatch(Action::AddressReviewThreads {
                    review: number.get_value(),
                    options: options(),
                })
                .is_ok()
            {
                crate::store::navigate(&Route::Agent(Some(format!(
                    "threads-{}",
                    number.get_value().slug()
                ))));
            }
        };
        let fixes = move |_| {
            if store
                .dispatch(Action::FixFailedChecks {
                    review: number.get_value(),
                    options: options(),
                })
                .is_ok()
            {
                crate::store::navigate(&Route::Agent(Some(format!(
                    "fixes-{}",
                    number.get_value().slug()
                ))));
            }
        };
        Some(view! {
            <div class="ui-vstack" style="--spacing: 10px" role="region" aria-label=t!(ReviewFollowupsRegion)>
                <h3 class="t-headline">{t!(ReviewFollowupsTitle)}</h3>
                {state}
                {finished}
                {agent_branch
                    .then(|| {
                        view! {
                            <p class="t-footnote t-secondary">
                                {t!(ReviewFollowupsIntro)}
                            </p>
                            <div class="ui-hstack" style="--spacing: 8px; flex-wrap: wrap">
                                <Button
                                    variant=ButtonStyle::BorderedProminent
                                    size=ControlSize::Large
                                    disabled=Signal::derive(move || busy || blocked.get())
                                    on_click=threads
                                >
                                    <Icon path=icons::SPARKLES />
                                    {t!(ReviewCommentsTitle)}
                                </Button>
                                {(failed > 0)
                                    .then(|| {
                                        view! {
                                            <Button
                                                size=ControlSize::Large
                                                disabled=Signal::derive(move || busy || blocked.get())
                                                on_click=fixes
                                            >
                                                <Icon path=icons::SPARKLES />
                                                {t!(ReviewFollowupsFix, n = failed)}
                                            </Button>
                                        }
                                    })}
                            </div>
                        }
                    })}
            </div>
        })
    }
}
