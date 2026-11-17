//! The job form: hint, repo and switches.

use feature_ui::*;
use leptos::prelude::*;

use super::context::{AgentCtx, OptionSignals};

/// The „Auftrag“ section: hint, repo and the switches before the start.
pub(super) fn job(ctx: AgentCtx) -> impl IntoView {
    let AgentCtx {
        repo_touched,
        project,
        routed,
        repo_options,
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
        agent_repos,
        repo,
        tests_allowed,
        ..
    } = ctx.form;
    view! {
        <Show when=move || !followup.get()>
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
                        <Show when=move || !agent_repos.get().is_empty()>
                            <ListRow title="Repo">
                                <Select
                                    options=repo_options
                                    value=Signal::derive(move || repo.get())
                                    on_change=move |v: String| {
                                        repo_touched.set(true);
                                        repo.set(v);
                                    }
                                    aria_label="Repo für den Agenten"
                                    disabled=locked
                                />
                            </ListRow>
                            <Show when=move || repo.get().is_empty() && !locked.get()>
                                <p class="t-footnote t-secondary" role="status">
                                    {move || match project.get() {
                                        Some(p) => format!("Für das Jira-Projekt {p} ist kein Repo zugeordnet und es gibt kein Standard-Repo. Wähle ein Repo aus."),
                                        None => "Es gibt kein Standard-Repo. Wähle ein Repo aus.".to_string(),
                                    }}
                                </p>
                            </Show>
                            <Show when=move || {
                                !locked.get() && !repo.get().is_empty()
                                    && matches!(routed.get(), cockpit_core::RepoChoice::Default(_))
                                    && project.get().is_some()
                            }>
                                <p class="t-footnote t-secondary">
                                    "Keine Zuordnungsregel für dieses Jira-Projekt – das Standard-Repo ist vorgewählt."
                                </p>
                            </Show>
                        </Show>
                        <ListRow title="Basis-Branch">
                            <Chip>{move || agent_repos.with(|a| a.base_branch(&repo.get()))}</Chip>
                        </ListRow>
                        <ListRow title="Tests vor dem Pull Request ausführen" toggle_row=true>
                            <Show when=move || !tests_allowed.get()>
                                <span class="t-footnote t-secondary">"In den Einstellungen aus"</span>
                            </Show>
                            <Show when=move || tests_forced.get()>
                                <span class="t-footnote t-secondary">"Pflicht laut Regeln"</span>
                            </Show>
                            <Toggle
                                checked=run_tests
                                disabled=Signal::derive(move || {
                                    locked.get() || !tests_allowed.get() || tests_forced.get()
                                })
                            />
                        </ListRow>
                        <ListRow title="Pull Request als Entwurf anlegen" toggle_row=true>
                            <Toggle checked=draft_pr disabled=locked />
                        </ListRow>
                        <ListRow title="Plan vor dem Start zeigen" toggle_row=true>
                            <Show when=move || plan_forced.get()>
                                <span class="t-footnote t-secondary">"Pflicht laut Regeln"</span>
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
