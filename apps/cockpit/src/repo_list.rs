//! The searchable list of repos with checkboxes, shared by the setup (agent repos, watched
//! repos) and the start of an agent run. The search, the limit of what is drawn and the
//! "narrow the list down" note are here; what a click does and what a row says is the page's.

use cockpit_core::api::GitHubRepoInfo;
use cockpit_core::setup::{REPOS_SHOWN, RepoPage, repo_page};
use feature_ui::*;
use leptos::prelude::*;

/// One line of a repo list: the `owner/name` and a short note after it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct RepoEntry {
    pub repo: String,
    pub detail: String,
}

impl RepoEntry {
    /// A repo from GitHub's list, with what GitHub says about access to it. `read_only` also
    /// names a repo the credential cannot push to (it matters when the agent works in it).
    pub fn from_github(info: &GitHubRepoInfo, read_only: bool) -> Self {
        let mut detail = if info.private {
            t!(SetupReposPrivate)
        } else {
            t!(SetupReposPublic)
        };
        if info.archived {
            detail.push_str(&format!(" · {}", t!(SetupReposArchived)));
        }
        if read_only && info.can_push == Some(false) {
            detail.push_str(&format!(" · {}", t!(SetupReposReadOnly)));
        }
        Self {
            repo: info.repo.clone(),
            detail,
        }
    }
}

/// The search over a list of entries: what was typed and what matches. The page creates it to
/// read the matches (for a summary or an empty note) and hands it to [`RepoList`].
#[derive(Clone, Copy)]
pub struct RepoSearch {
    query: RwSignal<String>,
    page: Memo<RepoPage<RepoEntry>>,
}

impl RepoSearch {
    /// `entries` is the whole list in the order it is shown; the search only narrows it down.
    pub fn new(entries: impl Into<Signal<Vec<RepoEntry>>>) -> Self {
        let entries: Signal<Vec<RepoEntry>> = entries.into();
        let query = RwSignal::new(String::new());
        let page = Memo::new(move |_| {
            let q = query.get();
            entries.with(|e| repo_page(e, |r| r.repo.as_str(), &q))
        });
        Self { query, page }
    }

    /// What was typed in the search field.
    pub fn query(&self) -> String {
        self.query.get()
    }

    /// How many entries match the search, drawn or not.
    pub fn matching(&self) -> usize {
        self.page.with(|p| p.matching)
    }
}

/// The search field, what the page puts between field and list (`children`: a button, notes),
/// the list of entries and the note when more entries match than are drawn.
#[component]
pub fn RepoList(
    search: RepoSearch,
    /// What screen readers call the list.
    #[prop(into)]
    label: Signal<String>,
    /// The chosen repos (`owner/name`); their boxes are checked.
    #[prop(into)]
    selected: Signal<Vec<String>>,
    /// A box was clicked: the repo is to be chosen or dropped.
    #[prop(into)]
    on_toggle: Callback<String>,
    children: Children,
) -> impl IntoView {
    let RepoSearch { query, page } = search;
    view! {
        <SearchField value=query placeholder=Signal::derive(move || t!(SetupReposSearch)) />
        {children()}
        <ul class="ck-repo-list" aria-label=move || label.get()>
            <For each=move || page.with(|p| p.shown.clone()) key=|entry| entry.clone() let:entry>
                {
                    let name = entry.repo.clone();
                    let checked = {
                        let name = name.clone();
                        move || selected.with(|s| s.contains(&name))
                    };
                    let toggle = {
                        let name = name.clone();
                        move |_| on_toggle.run(name.clone())
                    };
                    view! {
                        <li>
                            <label class="ck-repo-item">
                                <input type="checkbox" prop:checked=checked on:change=toggle />
                                <span class="ck-repo-name">{name}</span>
                                <span class="t-footnote t-secondary">{entry.detail}</span>
                            </label>
                        </li>
                    }
                }
            </For>
        </ul>
        <Show when=move || page.with(RepoPage::too_many)>
            <p class="t-footnote t-secondary">{move || t!(SetupReposTooMany, max = REPOS_SHOWN)}</p>
        </Show>
    }
}
