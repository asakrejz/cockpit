//! After the connection test: the Jira project to work in.

use cockpit_core::Action;
use cockpit_core::api::JiraProject;
use feature_ui::*;
use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::pages::setup::fields::{Context, Feedback, Message, Section};
use crate::server::{self, CallError};

/// After the test: the Jira projects to choose from ("All projects" is possible). Chosen as in
/// the inbox with `Action::SelectJiraProject`; the list is only read.
#[component]
pub(super) fn JiraProjectChoice(ctx: Context) -> impl IntoView {
    let projects = RwSignal::new(Vec::<JiraProject>::new());
    let back: Feedback = RwSignal::new(None);
    let loaded = RwSignal::new(false);
    spawn_local(async move {
        match server::fetch_jira_projects().await {
            Ok(r) => projects.set(r.projects),
            Err(CallError::Unreachable) => {
                back.set(Some((false, CallError::Unreachable.message())))
            }
            Err(e) => back.set(Some((
                false,
                t!(SetupJiraProjectsFailed, error = e.message()),
            ))),
        }
        loaded.set(true);
    });
    let options = Signal::derive(move || {
        let mut list: Vec<(String, String)> = projects.with(|p| {
            p.iter()
                .map(|p| {
                    let label = if p.name.is_empty() {
                        p.key.clone()
                    } else {
                        format!("{} · {}", p.key, p.name)
                    };
                    (p.key.clone(), label)
                })
                .collect()
        });
        list.sort_by_key(|a| a.1.to_lowercase());
        std::iter::once((String::new(), t!(SetupJiraAllProjects)))
            .chain(list)
            .collect()
    });
    let chosen = Signal::derive(move || {
        ctx.store
            .ws
            .with(|w| w.jira_project.clone().unwrap_or_default())
    });
    let switch = move |key: String| {
        let project = (!key.is_empty()).then_some(key);
        if let Err(e) = ctx.store.dispatch(Action::SelectJiraProject { project }) {
            back.set(Some((false, e.msg().render(feature_ui::lang_untracked()))));
        } else {
            back.set(Some((true, t!(SetupJiraChoiceSaved))));
        }
    };
    view! {
        <Section title=t!(SetupJiraChoiceTitle)>
            <p class="t-subheadline t-secondary">
                {t!(SetupJiraChoiceText)}
            </p>
            {move || {
                (!loaded.get())
                    .then(|| view! { <p class="t-footnote t-secondary">{t!(SetupJiraLoading)}</p> })
            }}
            {move || {
                (loaded.get() && projects.with(|p| p.is_empty()) && back.with(|r| r.is_none()))
                    .then(|| {
                        view! {
                            <p class="ui-hint">
                                {t!(SetupJiraNoProjects)}
                            </p>
                        }
                    })
            }}
            <Select
                options=options
                value=chosen
                on_change=switch
                aria_label=t!(SetupJiraProjectAria)
            />
            <Message feedback=back />
        </Section>
    }
}
