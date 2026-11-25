//! Step „Tickets“: where the tickets come from. Jira or GitHub Projects, and „both“ as a quiet
//! third option. The assistant then shows only the steps of that choice. Saved at once
//! (`[github_projects] tickets_from`); nothing is written to Jira or GitHub.

use cockpit_core::TicketsFrom;
use cockpit_core::api::{ConfigChange, GitHubProjectsInput};
use feature_ui::*;
use leptos::prelude::*;

use super::fields::{Context, Feedback, Intro, Message, Section, config_save};

#[component]
pub fn SourceStep(ctx: Context) -> impl IntoView {
    let feedback: Feedback = RwSignal::new(None);
    let current = Signal::derive(move || ctx.setup.with(|s| s.github_projects.tickets_from));
    let choose = move |from: TicketsFrom| {
        if current.get_untracked() == from {
            return;
        }
        config_save(
            ctx,
            ConfigChange {
                github_projects: Some(GitHubProjectsInput {
                    tickets_from: Some(from),
                    ..Default::default()
                }),
                ..Default::default()
            },
            feedback,
            t!(SetupProjectsFromSaved),
            || {},
        );
    };
    let hint = move || match current.get() {
        TicketsFrom::Jira => t!(SetupProjectsSourceJiraHint),
        TicketsFrom::GitHubProjects => t!(SetupProjectsSourceProjectsHint),
        TicketsFrom::Both => t!(SetupProjectsSourceBothHint),
    };
    let main = [
        (TicketsFrom::Jira, t!(SetupProjectsFromJira)),
        (TicketsFrom::GitHubProjects, t!(SetupProjectsFromProjects)),
    ];
    let main_buttons = main
        .into_iter()
        .map(|(from, label)| {
            view! {
                <button
                    type="button"
                    aria-pressed=move || (current.get() == from).to_string()
                    on:click=move |_| choose(from)
                >
                    {label}
                </button>
            }
        })
        .collect_view();
    view! {
        <VStack class="ui-gap-20">
            <Intro title=t!(SetupProjectsSourceTitle)>
                <p>{t!(SetupProjectsSourceIntro)}</p>
            </Intro>
            <Section title=t!(SetupProjectsFromLabel)>
                <div
                    class="ui-segmented ui-segmented--fill"
                    role="group"
                    aria-label=t!(SetupProjectsFromLabel)
                >
                    {main_buttons}
                </div>
                <p class="t-footnote t-secondary">{hint}</p>
                <Button
                    variant=ButtonStyle::Borderless
                    size=ControlSize::Small
                    on_click=move |_| choose(TicketsFrom::Both)
                    disabled=Signal::derive(move || current.get() == TicketsFrom::Both)
                >
                    {t!(SetupProjectsSourceBoth)}
                </Button>
                <Message feedback=feedback />
            </Section>
        </VStack>
    }
}
