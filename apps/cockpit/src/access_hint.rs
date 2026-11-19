//! The hint when GitHub does not show the configured repos to the connected access.
//!
//! "No pull requests" is only true if every repo was reachable. Review, inbox and the GitHub
//! step all ask the same question (`GET /api/github/access`) and show the same hint.

use cockpit_core::api::{GitHubAccess, RepoVisibility};
use cockpit_core::setup::Step;
use feature_ui::*;
use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::route::Route;
use crate::server;
use crate::store::use_store;

/// The access report of the server and a way to ask again. It asks once on creation, and only if
/// GitHub is connected; the answer is `None` until it arrives (or if the server cannot say).
pub fn use_github_access() -> (RwSignal<Option<GitHubAccess>>, Callback<()>) {
    let store = use_store();
    let access = RwSignal::new(None::<GitHubAccess>);
    let load = Callback::new(move |()| {
        if !store.conn.with_untracked(|c| c.github) {
            access.set(None);
            return;
        }
        spawn_local(async move {
            access.set(server::fetch_github_access().await.ok());
        });
    });
    // The connection state arrives a moment after the page: ask again when GitHub turns up as
    // connected (or goes away).
    Effect::new(move |_| {
        let _ = store.conn.with(|c| c.github);
        load.run(());
    });
    (access, load)
}

/// The report if some repo is not (surely) visible; otherwise `None`.
pub fn problem_of(access: &RwSignal<Option<GitHubAccess>>) -> Option<GitHubAccess> {
    access.get().filter(GitHubAccess::problem)
}

/// What is wrong, in one title: a missing install first, then a refusal, then "unknown".
fn title(a: &GitHubAccess) -> String {
    if !a.with(RepoVisibility::NotVisible).is_empty() {
        t!(GithubAccessTitleNotVisible)
    } else if !a.with(RepoVisibility::NoPermission).is_empty() {
        t!(GithubAccessTitleNoPermission)
    } else {
        t!(GithubAccessTitleUnreachable)
    }
}

fn why(a: &GitHubAccess) -> String {
    let unknown = a.repos.iter().all(|r| {
        matches!(
            r.visibility,
            RepoVisibility::Visible | RepoVisibility::Unreachable
        )
    });
    if unknown {
        t!(GithubAccessWhyUnreachable)
    } else if a.via_app {
        t!(GithubAccessWhyApp)
    } else {
        t!(GithubAccessWhyToken)
    }
}

/// The hint with the steps. The steps are about the GitHub App, so they only show for it.
#[component]
pub fn AccessHint(access: GitHubAccess, recheck: Callback<()>) -> impl IntoView {
    let broken: Vec<String> = access
        .repos
        .iter()
        .filter(|r| r.visibility != RepoVisibility::Visible)
        .map(|r| r.repo.clone())
        .collect();
    let steps = access.via_app
        && (!access.with(RepoVisibility::NotVisible).is_empty()
            || !access.with(RepoVisibility::NoPermission).is_empty());
    let nowhere = access.installed == Some(false);
    let install_url = access.install_url.clone();
    let again = move |_| recheck.run(());
    view! {
        <div class="ck-access">
            <ErrorState title=title(&access) text=why(&access)>
                <p class="t-footnote t-secondary">
                    {t!(GithubAccessRepos, repos = broken.join(", "))}
                </p>
                {nowhere.then(|| view! { <p class="t-footnote">{t!(GithubAccessNowhere)}</p> })}
                {steps
                    .then(|| {
                        view! {
                            <div class="ck-access__steps">
                                <p class="t-subheadline t-semibold">{t!(GithubAccessSteps)}</p>
                                <ol class="ui-instructions">
                                    <li>{t!(GithubAccessStepInstall)}</li>
                                    <li>{t!(GithubAccessStepAnyAccount)}</li>
                                    <li>{t!(GithubAccessStepOwner)}</li>
                                    <li>{t!(GithubAccessStepRights)}</li>
                                    <li>{t!(GithubAccessStepConfirm)}</li>
                                </ol>
                            </div>
                        }
                    })}
                {steps
                    .then(|| {
                        view! {
                            <p class="t-footnote">{t!(GithubAccessTokenAlternative)}</p>
                        }
                    })}
                {install_url
                    .map(|url| {
                        view! {
                            <a class="ui-link" href=url target="_blank" rel="noopener noreferrer">
                                {t!(GithubAccessInstallLink)}
                            </a>
                        }
                    })}
                <Button size=ControlSize::Small on_click=again>
                    {t!(GithubAccessRecheck)}
                </Button>
            </ErrorState>
        </div>
    }
}

/// A slim bar for a list that has other content: says what is wrong and where to fix it.
#[component]
pub fn AccessBanner(access: GitHubAccess) -> impl IntoView {
    let broken: Vec<String> = access
        .repos
        .iter()
        .filter(|r| r.visibility != RepoVisibility::Visible)
        .map(|r| r.repo.clone())
        .collect();
    view! {
        <StatusBanner
            tone=BannerTone::Warning
            title=title(&access)
            text=t!(GithubAccessRepos, repos = broken.join(", "))
        >
            <Button size=ControlSize::Small href=Route::SetupStep(Step::GitHub).hash()>
                {t!(GithubAccessFix)}
            </Button>
        </StatusBanner>
    }
}
