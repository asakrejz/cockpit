//! The job form: hint, repo and switches.

use feature_ui::*;
use leptos::prelude::*;

use super::context::{AgentCtx, OptionSignals};
use super::repo_choice::repo_choice;

/// The "Job" section: hint, repo and the switches before the start.
pub(super) fn job(ctx: AgentCtx) -> impl IntoView {
    let AgentCtx {
        base_branch,
        followup,
        tests_forced,
        plan_forced,
        locked,
        ..
    } = ctx;
    let OptionSignals {
        run_tests,
        draft_pr,
        hint,
        plan_first,
        repo,
        tests_allowed,
        ..
    } = ctx.form;
    view! {
        <Show when=move || !followup.get()>
        <div class="ui-list__section">
            <p class="ui-list__header">{t!(AgentJobJob)}</p>
            <div class="ui-vstack ui-gap-10">
                <Textarea
                    value=hint
                    label=t!(AgentJobHintLabel)
                    placeholder=t!(AgentJobHintPlaceholder)
                    min_height="72px"
                    disabled=locked
                />
                <List>
                    <ListSection nested=true>
                        {repo_choice(ctx)}
                        <ListRow title=t!(AgentJobBaseBranch)>
                            <Chip>{move || base_branch.get()}</Chip>
                        </ListRow>
                        <Show when=move || !repo.get().is_empty()>
                            <p class="t-footnote t-secondary" role="status">
                                {move || {
                                    t!(
                                        AgentJobWorksIn,
                                        repo = repo.get(),
                                        base = base_branch.get(),
                                    )
                                }}
                            </p>
                        </Show>
                        <ListRow title=t!(AgentJobRunTests) toggle_row=true>
                            <Show when=move || !tests_allowed.get()>
                                <span class="t-footnote t-secondary">{t!(AgentJobOffInSettings)}</span>
                            </Show>
                            <Show when=move || tests_forced.get()>
                                <span class="t-footnote t-secondary">{t!(AgentJobRequired)}</span>
                            </Show>
                            <Toggle
                                checked=run_tests
                                disabled=Signal::derive(move || {
                                    locked.get() || !tests_allowed.get() || tests_forced.get()
                                })
                            />
                        </ListRow>
                        <ListRow title=t!(AgentJobDraftPr) toggle_row=true>
                            <Toggle checked=draft_pr disabled=locked />
                        </ListRow>
                        <ListRow title=t!(AgentJobPlanFirst) toggle_row=true>
                            <Show when=move || plan_forced.get()>
                                <span class="t-footnote t-secondary">{t!(AgentJobRequired)}</span>
                            </Show>
                            <Toggle
                                checked=plan_first
                                disabled=Signal::derive(move || locked.get() || plan_forced.get())
                            />
                        </ListRow>
                    </ListSection>
                </List>
            </div>
        </div>
        </Show>
    }
}
