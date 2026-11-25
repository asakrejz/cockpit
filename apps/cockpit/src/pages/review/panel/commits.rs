//! Tab „Commits“: the commits of the pull request; a click shows only the diff of that commit.
//! Read only – comments belong to the diff of the whole pull request.

use cockpit_core::api::{PanelCommit, PanelFile};
use cockpit_core::{DiffLine, commit_date, short_sha};
use leptos::task::spawn_local;

use super::*;

fn diff_rows(lines: &[DiffLine]) -> Vec<DiffRow> {
    view::diff_rows(&FileDiff {
        name: String::new(),
        lines: lines.to_vec(),
    })
}

/// Loads the commit list once.
fn load_commits(repo: String, number: u32, ui: PanelUi) {
    ui.commits.set(Load::Loading);
    spawn_local(async move {
        ui.commits
            .set(match server::fetch_panel_commits(&repo, number).await {
                Ok(c) => Load::Done(c),
                Err(e) => Load::Failed(e.message()),
            });
    });
}

fn load_commit(repo: String, number: u32, sha: String, ui: PanelUi) {
    ui.chosen.set(Some(sha.clone()));
    ui.commit_diff.set(Load::Loading);
    ui.full.set(Vec::new());
    spawn_local(async move {
        let answer = server::fetch_panel_commit(&repo, number, &sha, None).await;
        // The person may have picked another commit meanwhile.
        if ui.chosen.get_untracked().as_deref() != Some(sha.as_str()) {
            return;
        }
        ui.commit_diff.set(match answer {
            Ok(d) => Load::Done(d),
            Err(e) => Load::Failed(e.message()),
        });
    });
}

/// Fetches the rest of one file of the chosen commit.
fn load_rest(repo: String, number: u32, sha: String, path: String, ui: PanelUi) {
    spawn_local(async move {
        if let Ok(mut d) = server::fetch_panel_commit(&repo, number, &sha, Some(&path)).await
            && ui.chosen.get_untracked().as_deref() == Some(sha.as_str())
            && let Some(file) = d.files.pop()
        {
            ui.full.update(|f| f.push(file));
        }
    });
}

pub(in super::super) fn commits_tab(r: Review, ui: PanelUi) -> AnyView {
    if r.repo.is_empty() {
        return view! {
            <p class="t-subheadline t-secondary">
                {t!(ReviewPanelCommitsUnknownRepo)}
            </p>
        }
        .into_any();
    }
    let (repo, number) = (r.repo.clone(), r.number);
    if matches!(ui.commits.get_untracked(), Load::Idle) {
        load_commits(repo.clone(), number, ui);
    }
    let repo = StoredValue::new(repo);
    view! {
        {move || match ui.chosen.get() {
            Some(sha) => commit_view(repo.get_value(), number, sha, ui),
            None => list_view(repo.get_value(), number, ui),
        }}
    }
    .into_any()
}

fn list_view(repo: String, number: u32, ui: PanelUi) -> AnyView {
    match ui.commits.get() {
        Load::Idle | Load::Loading => {
            view! { <p class="t-subheadline t-secondary">{t!(ReviewPanelCommitsLoading)}</p> }
                .into_any()
        }
        Load::Failed(m) => failure(m, move || load_commits(repo.clone(), number, ui)),
        Load::Done(list) if list.commits.is_empty() => {
            view! { <p class="t-subheadline t-secondary">{t!(ReviewPanelCommitsEmpty)}</p> }
                .into_any()
        }
        Load::Done(list) => {
            let note = list.truncated.then(|| {
                view! {
                    <p class="t-footnote t-secondary">
                        {t!(ReviewPanelCommitsMore)}
                    </p>
                }
            });
            let items = list
                .commits
                .into_iter()
                .map(|c| {
                    let (repo, sha) = (repo.clone(), c.sha.clone());
                    view! {
                        <li>
                            <button
                                type="button"
                                class="ck-commit"
                                on:click=move |_| load_commit(repo.clone(), number, sha.clone(), ui)
                            >
                                <span class="t-subheadline t-semibold ck-commit__title">{c.title.clone()}</span>
                                <span class="t-footnote t-secondary">
                                    {format!("{} · {}", c.author, commit_date(&c.date))}
                                </span>
                                <span class="t-footnote t-mono t-secondary">{short_sha(&c.sha).to_string()}</span>
                            </button>
                        </li>
                    }
                })
                .collect_view();
            view! {
                {note}
                <ul class="ck-commits" aria-label=t!(ReviewPanelTabCommits)>
                    {items}
                </ul>
            }
            .into_any()
        }
    }
}

fn commit_head(sha: &str, ui: PanelUi) -> AnyView {
    let info: Option<PanelCommit> = ui.commits.with_untracked(|c| match c {
        Load::Done(l) => l.commits.iter().find(|c| c.sha == sha).cloned(),
        _ => None,
    });
    let short = short_sha(sha).to_string();
    view! {
        <div class="ui-vstack ui-gap-8 ui-items-start">
            <Button
                size=ControlSize::Small
                on_click=move |_| {
                    ui.chosen.set(None);
                    ui.commit_diff.set(Load::Idle);
                }
            >
                {t!(ReviewPanelCommitsBack)}
            </Button>
            {info
                .map(|c| {
                    view! {
                        <h2 class="t-headline">{c.title}</h2>
                        {(!c.body.is_empty())
                            .then(|| view! { <p class="t-subheadline t-secondary ck-commit__body">{c.body}</p> })}
                        <span class="t-footnote t-secondary">
                            {format!("{} · {} · {}", c.author, commit_date(&c.date), short)}
                        </span>
                    }
                })}
        </div>
    }
    .into_any()
}

fn commit_view(repo: String, number: u32, sha: String, ui: PanelUi) -> AnyView {
    let head = commit_head(&sha, ui);
    let body = match ui.commit_diff.get() {
        Load::Idle | Load::Loading => {
            view! { <p class="t-subheadline t-secondary">{t!(ReviewPanelCommitLoading)}</p> }
                .into_any()
        }
        Load::Failed(m) => {
            let (repo, sha) = (repo.clone(), sha.clone());
            failure(m, move || {
                load_commit(repo.clone(), number, sha.clone(), ui)
            })
        }
        Load::Done(d) => {
            let note = d.truncated_files.then(|| {
                view! { <p class="t-footnote t-secondary">{t!(ReviewPanelCommitTruncated)}</p> }
            });
            let files = d
                .files
                .into_iter()
                .map(|f| commit_file(repo.clone(), number, sha.clone(), f, ui))
                .collect_view();
            view! {
                {note}
                {files}
            }
            .into_any()
        }
    };
    view! {
        <div class="ui-vstack ui-gap-12">
            {head}
            {body}
        </div>
    }
    .into_any()
}

fn commit_file(
    repo: String,
    number: u32,
    sha: String,
    file: PanelFile,
    ui: PanelUi,
) -> impl IntoView {
    let head_name = file.name.clone();
    let name = StoredValue::new(file.name.clone());
    let file = StoredValue::new(file);
    // The first part of the file, or all of it once it was fetched.
    let shown = move || {
        ui.full
            .with(|f| f.iter().find(|x| x.name == name.get_value()).cloned())
            .unwrap_or_else(|| file.get_value())
    };
    view! {
        <section class="ck-file">
            <div class="ck-file__head">
                <span class="ck-ellipsis t-subheadline t-semibold t-mono">{head_name}</span>
                <span class="t-footnote t-mono ck-stat">
                    {move || {
                        let f = shown();
                        view! {
                            <span class="ck-stat__add">{format!("+{}", f.additions)}</span>
                            " "
                            <span class="ck-stat__del">{format!("−{}", f.deletions)}</span>
                        }
                    }}
                </span>
            </div>
            {move || {
                let f = shown();
                if f.no_patch {
                    return view! {
                        <p class="t-footnote t-secondary">
                            {t!(ReviewPanelNoText)}
                        </p>
                    }
                    .into_any();
                }
                let rest = f.total_lines.saturating_sub(f.lines.len());
                let (repo, sha, name) = (repo.clone(), sha.clone(), f.name.clone());
                view! {
                    <Diff rows=diff_rows(&f.lines) label=f.name.clone() />
                    {(f.truncated && rest > 0)
                        .then(|| {
                            view! {
                                <Button
                                    size=ControlSize::Small
                                    on_click=move |_| {
                                        load_rest(repo.clone(), number, sha.clone(), name.clone(), ui)
                                    }
                                >
                                    {t!(ReviewPanelLoadRest, rest = rest)}
                                </Button>
                            }
                        })}
                }
                .into_any()
            }}
        </section>
    }
}
