//! The repos the agent works in, and which Jira project goes to which repo.

use cockpit_core::api::{ConfigChange, RepoCheck, RepoState};
use cockpit_core::setup::agent_repos_read;
use feature_ui::*;
use leptos::prelude::*;
use leptos::task::spawn_local;

use super::fields::{Context, Feedback, Message, Section, config_save, error_text};
use crate::server;

/// One row of the list. The signals live as long as the page.
#[derive(Clone, Copy)]
struct Row {
    id: usize,
    repo: RwSignal<String>,
    base: RwSignal<String>,
    projects: RwSignal<String>,
}

#[component]
pub fn AgentReposSection(ctx: Context) -> impl IntoView {
    let next_id = StoredValue::new(0usize);
    let new_row = move |repo: &str, base: &str, projects: &str| {
        let id = next_id.get_value();
        next_id.set_value(id + 1);
        Row {
            id,
            repo: RwSignal::new(repo.into()),
            base: RwSignal::new(base.into()),
            projects: RwSignal::new(projects.into()),
        }
    };
    let rows: RwSignal<Vec<Row>> = RwSignal::new(Vec::new());
    let default = RwSignal::new(String::new());
    let loaded = RwSignal::new(false);
    // Fill from the stored values once (they arrive after the page opens).
    Effect::new(move |_| {
        let stored = ctx.setup.with(|s| s.agent_repos.clone());
        if loaded.get_untracked() || (stored.repos.is_empty() && stored.default.is_none()) {
            return;
        }
        loaded.set(true);
        rows.set(
            stored
                .repos
                .iter()
                .map(|r| new_row(&r.repo, &r.base_branch, &r.projects.join(", ")))
                .collect(),
        );
        default.set(stored.default.unwrap_or_default());
    });

    let feedback: Feedback = RwSignal::new(None);
    let checks: RwSignal<Vec<RepoCheck>> = RwSignal::new(Vec::new());
    let checking = RwSignal::new(false);

    let read = move || {
        let list: Vec<(String, String, String)> = rows
            .get_untracked()
            .iter()
            .map(|r| {
                (
                    r.repo.get_untracked(),
                    r.base.get_untracked(),
                    r.projects.get_untracked(),
                )
            })
            .collect();
        agent_repos_read(&list, &default.get_untracked())
    };
    let save = move |_| match read() {
        Ok(input) => config_save(
            ctx,
            ConfigChange {
                agent_repos: Some(input),
                ..Default::default()
            },
            feedback,
            "Repos für den Agenten gespeichert.",
            || {},
        ),
        Err(m) => feedback.set(Some((false, m))),
    };
    let check = move |_| match read() {
        Ok(input) if input.repos.is_empty() => {
            feedback.set(Some((
                false,
                "Trage zuerst mindestens ein Repo ein.".into(),
            )));
        }
        Ok(input) => {
            feedback.set(None);
            checking.set(true);
            let names: Vec<String> = input.repos.iter().map(|r| r.repo.clone()).collect();
            spawn_local(async move {
                match server::post_repos_check(names).await {
                    Ok(answer) => checks.set(answer.repos),
                    Err(e) => feedback.set(Some((false, error_text(&e)))),
                }
                checking.set(false);
            });
        }
        Err(m) => feedback.set(Some((false, m))),
    };
    let add = move |_| {
        let r = new_row("", "main", "");
        rows.update(|l| l.push(r));
    };

    view! {
        <Section title="6. In welchen Repos arbeitet der Agent?">
            <p class="t-subheadline t-secondary">
                "Der Agent klont für jeden Lauf genau ein Repo und legt dort seinen Branch „claude/…“ an. Ordne Jira-Projekten ein Repo zu: Tickets eines Projekts landen dann dort. Alle anderen Tickets gehen ins Standard-Repo. Vor dem Start kannst du das Repo in der Agent-Ansicht noch ändern."
            </p>
            <VStack spacing="12px">
                <For each=move || rows.get() key=|r| r.id let:row>
                    <div class="ck-agent-repo">
                        <TextField label="Repo" value=row.repo placeholder="firma/web" autocomplete="off" />
                        <TextField label="Basis-Branch" value=row.base placeholder="main" autocomplete="off" />
                        <TextField
                            label="Jira-Projekte"
                            value=row.projects
                            placeholder="WEB, SHOP"
                            hint="Leer lassen, wenn dieses Repo nur zur Auswahl stehen soll."
                            autocomplete="off"
                        />
                        <HStack spacing="12px">
                            <label style="display: inline-flex; gap: 6px; align-items: center; cursor: pointer">
                                <input
                                    type="radio"
                                    name="agent-default-repo"
                                    prop:checked=move || {
                                        let r = row.repo.get();
                                        !r.trim().is_empty() && default.get() == r.trim()
                                    }
                                    on:change=move |_| default.set(row.repo.get_untracked().trim().to_string())
                                />
                                "Standard-Repo"
                            </label>
                            <Button
                                variant=ButtonStyle::Borderless
                                on_click=move |_| rows.update(|l| l.retain(|r| r.id != row.id))
                            >
                                "Entfernen"
                            </Button>
                        </HStack>
                    </div>
                </For>
                <HStack spacing="12px">
                    <Button on_click=add>"Repo hinzufügen"</Button>
                    <Button
                        on_click=check
                        disabled=Signal::derive(move || checking.get())
                    >
                        {move || if checking.get() { "Prüfe …" } else { "Repos prüfen" }}
                    </Button>
                    <Button variant=ButtonStyle::GlassProminent on_click=save>
                        "Speichern"
                    </Button>
                </HStack>
                <Message feedback=feedback />
                <ul class="ui-checklist">
                    {move || {
                        checks
                            .get()
                            .into_iter()
                            .map(|c| {
                                let mark = match c.state {
                                    RepoState::Ok => "✓",
                                    RepoState::ReadOnly => "!",
                                    _ => "✗",
                                };
                                view! { <li>{format!("{mark} {} – {}", c.repo, c.message)}</li> }
                            })
                            .collect_view()
                    }}
                </ul>
                <p class="t-footnote t-secondary">
                    "„Repos prüfen“ schaut nur nach (lesend): Gibt es das Repo, sieht dein GitHub-Zugang es, und darf er dort schreiben?"
                </p>
            </VStack>
        </Section>
    }
}
