//! The Reviews list as sections: one per repo (with the number of pull requests and how many need
//! my attention), under an owner header when there are many repos. Sections collapse; the choice
//! is remembered per browser (`localStorage`, nothing secret; without storage everything is
//! expanded). Up and down move along what is on screen, collapsed sections are skipped.

use std::cell::Cell;

use cockpit_core::review_filter::{
    OWNER_GROUPS_FROM, ReviewGroup, group_sections, is_collapsed, step_key, visible_keys,
};
use cockpit_core::{PrKey, Review};
use feature_ui::*;
use leptos::{ev, prelude::*, wasm_bindgen::JsCast};
use web_sys::{HtmlElement, ScrollIntoViewOptions, ScrollLogicalPosition};

use super::review_label;
use crate::{route::Route, store::Store};

const KEY: &str = "cockpit-review-collapsed";

thread_local! {
    /// The last move came from the keyboard: the page is built anew for the other pull request
    /// and puts the focus back on its entry.
    static KEEP_FOCUS: Cell<bool> = const { Cell::new(false) };
}

/// The collapsed repos (`owner/name`) and owners (`owner`).
pub(super) fn load() -> Vec<String> {
    window()
        .local_storage()
        .ok()
        .flatten()
        .and_then(|s| s.get_item(KEY).ok().flatten())
        .map(|t| {
            t.lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn save(list: &[String]) {
    if let Some(s) = window().local_storage().ok().flatten() {
        let _ = s.set_item(KEY, &list.join("\n"));
    }
}

fn toggle(collapsed: RwSignal<Vec<String>>, key: &str) {
    collapsed.update(
        |c| match c.iter().position(|k| k.eq_ignore_ascii_case(key)) {
            Some(i) => {
                c.remove(i);
            }
            None => c.push(key.to_string()),
        },
    );
    save(&collapsed.get_untracked());
}

/// The groups for the current filter.
fn groups(store: Store, current: Option<&PrKey>) -> Vec<ReviewGroup> {
    store.ws.with(|w| {
        let list = w.reviews_filtered(&w.review_filter, current);
        group_sections(w.review_sections(&list), OWNER_GROUPS_FROM)
    })
}

/// A header you can click to collapse or expand what is under it.
fn header(
    label: String,
    count: usize,
    attention: usize,
    open: bool,
    level: &'static str,
    on_toggle: impl Fn() + 'static,
) -> impl IntoView {
    view! {
        <button
            type="button"
            class="ck-section-head"
            data-level=level
            aria-expanded=open.to_string()
            on:click=move |_| on_toggle()
        >
            <span class="ck-section-head__chevron" data-open=open.then_some("")>
                <Icon path=icons::CHEVRON_RIGHT size=14 stroke=2.4 />
            </span>
            <span class="ck-section-head__name">{label}</span>
            {(attention > 0)
                .then(|| view! { <Badge variant=BadgeVariant::Count>{attention.to_string()}</Badge> })}
            <span class="t-footnote t-secondary">{count.to_string()}</span>
        </button>
    }
}

fn item(r: &Review, is_current: bool, prefix_repo: bool) -> AnyView {
    view! {
        <SidebarItem
            href=Route::Review(Some(r.key())).hash()
            label=review_label(r, prefix_repo)
            current=is_current
            count=if r.sent { t!(ReviewSent) } else { String::new() }
        />
    }
    .into_any()
}

/// The list: sections, headers and entries.
pub(super) fn list(
    store: Store,
    collapsed: RwSignal<Vec<String>>,
    current: Option<PrKey>,
) -> impl IntoView {
    let groups = groups(store, current.as_ref());
    let collapsed_now = collapsed.get();
    let sections_total: usize = groups.iter().map(|g| g.sections.len()).sum();
    // One repo only: no header at all. A filter that leaves one repo of several keeps the repo
    // in the entries' names instead.
    let headers = sections_total > 1;
    let multi_repo = store.ws.with(|w| {
        let mut repos = w.reviews.iter().map(|r| r.repo.as_str());
        repos.next().is_some_and(|first| repos.any(|r| r != first))
    });
    let reviews: Vec<Review> = store.ws.with(|w| w.reviews.clone());
    let find = move |k: &PrKey| reviews.iter().find(|r| r.key().same(k)).cloned();

    groups
        .into_iter()
        .map(|g| {
            let owner = g.owner.clone();
            let owner_open = owner
                .as_deref()
                .is_none_or(|o| !is_collapsed(&collapsed_now, None, o));
            let owner_head = owner.clone().map(|o| {
                let key = o.clone();
                header(
                    o,
                    g.count(),
                    g.attention(),
                    owner_open,
                    "owner",
                    move || toggle(collapsed, &key),
                )
            });
            let body = g
                .sections
                .into_iter()
                .map(|s| {
                    let holds_current = current
                        .as_ref()
                        .is_some_and(|c| s.keys.iter().any(|k| k.same(c)));
                    let open = holds_current
                        || !is_collapsed(&collapsed_now, owner.as_deref(), &s.repo);
                    let head = headers.then(|| {
                        let key = s.repo.clone();
                        let name = if s.repo.is_empty() {
                            t!(ReviewFilterUnknownRepo)
                        } else {
                            s.repo.clone()
                        };
                        header(name, s.keys.len(), s.attention, open, "repo", move || {
                            toggle(collapsed, &key)
                        })
                    });
                    let entries = open.then(|| {
                        let items = s
                            .keys
                            .iter()
                            .filter_map(&find)
                            .map(|r| {
                                let is_current = current.as_ref().is_some_and(|n| n.same(&r.key()));
                                item(&r, is_current, !headers && multi_repo)
                            })
                            .collect_view();
                        view! { <ul class="ui-sidebar__nav">{items}</ul> }
                    });
                    view! { <section class="ck-section" data-repo=s.repo.clone()>{head}{entries}</section> }
                        .into_any()
                })
                .collect_view();
            view! {
                <div class="ck-section-group">
                    {owner_head}
                    {owner_open.then_some(body)}
                </div>
            }
            .into_any()
        })
        .collect_view()
}

/// Opens what hides the open pull request (a deep link or "Go to entry" must show it), and
/// scrolls its entry into view (and focuses it after a move by keyboard).
pub(super) fn reveal(store: Store, collapsed: RwSignal<Vec<String>>, current: Option<PrKey>) {
    let Some(current) = current else { return };
    let repo = store
        .ws
        .with_untracked(|w| w.review(current.clone()).map(|r| r.repo.clone()));
    if let Some(repo) = repo {
        let owner = repo.split('/').next().unwrap_or_default().to_string();
        let hides = |c: &[String]| {
            c.iter()
                .any(|k| k.eq_ignore_ascii_case(&repo) || k.eq_ignore_ascii_case(&owner))
        };
        if hides(&collapsed.get_untracked()) {
            collapsed.update(|c| {
                c.retain(|k| !k.eq_ignore_ascii_case(&repo) && !k.eq_ignore_ascii_case(&owner))
            });
            save(&collapsed.get_untracked());
        }
    }
    request_animation_frame(|| {
        let Some(row) = query_html(".ui-pane[data-kind=\"picker\"] [aria-current=\"page\"]") else {
            return;
        };
        let options = ScrollIntoViewOptions::new();
        options.set_block(ScrollLogicalPosition::Nearest);
        row.scroll_into_view_with_scroll_into_view_options(&options);
        if KEEP_FOCUS.with(|f| f.replace(false)) {
            let _ = row.focus();
        }
    });
}

fn query_html(selector: &str) -> Option<HtmlElement> {
    document()
        .query_selector(selector)
        .ok()
        .flatten()
        .and_then(|e| e.dyn_into::<HtmlElement>().ok())
}

/// Up and down in the list: the previous or next pull request on screen. Only while the focus is
/// in the list, so scrolling the diff stays as it is.
pub(super) fn keys(store: Store, collapsed: RwSignal<Vec<String>>, current: Option<PrKey>) {
    let listener = window_event_listener(ev::keydown, move |event| {
        let forward = match event.key().as_str() {
            "ArrowDown" => true,
            "ArrowUp" => false,
            _ => return,
        };
        if event.alt_key() || event.ctrl_key() || event.meta_key() || event.shift_key() {
            return;
        }
        let in_list = document().active_element().is_some_and(|e| {
            e.closest(".ui-pane[data-kind=\"picker\"]")
                .ok()
                .flatten()
                .is_some()
        });
        if !in_list {
            return;
        }
        event.prevent_default();
        let groups = groups(store, current.as_ref());
        let visible = visible_keys(&groups, &collapsed.get_untracked(), current.as_ref());
        if let Some(next) = step_key(&visible, current.as_ref(), forward) {
            KEEP_FOCUS.with(|f| f.set(true));
            crate::store::navigate(&Route::Review(Some(next)));
        }
    });
    on_cleanup(move || listener.remove());
}
