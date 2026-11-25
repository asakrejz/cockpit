//! Tab „Dateien“: file tree with counts, one collapsible diff per file, „Angesehen“.

use cockpit_core::{TreeRow, file_tree};
use web_sys::{ScrollIntoViewOptions, ScrollLogicalPosition};

use super::*;

fn anchor(index: usize) -> String {
    format!("ck-file-{index}")
}

/// Scrolls to the diff of a file.
fn show_file(index: usize) {
    if let Some(el) = document().get_element_by_id(&anchor(index)) {
        let options = ScrollIntoViewOptions::new();
        options.set_block(ScrollLogicalPosition::Start);
        el.scroll_into_view_with_scroll_into_view_options(&options);
    }
}

fn stat_view(additions: usize, deletions: usize) -> impl IntoView {
    view! {
        <span class="t-footnote t-mono ck-stat">
            <span class="ck-stat__add">{format!("+{additions}")}</span>
            " "
            <span class="ck-stat__del">{format!("−{deletions}")}</span>
        </span>
    }
}

pub(in super::super) fn files_tab(store: Store, r: Review, lines: LineUi, ui: PanelUi) -> AnyView {
    if r.files.is_empty() {
        return view! {
            <p class="t-subheadline t-secondary">
                {t!(ReviewPanelFilesEmpty)}
            </p>
        }
        .into_any();
    }
    let key = StoredValue::new(r.key());
    let files = StoredValue::new(r.files.clone());
    let total = r.files.len();
    let (adds, dels) = r.files.iter().fold((0, 0), |(a, d), f| {
        let (fa, fd) = f.counts();
        (a + fa, d + fd)
    });
    let progress = move || {
        let n = ui.viewed.with(|v| files.with_value(|f| v.count(f)));
        t!(ReviewPanelFilesProgress, n = n, total = total)
    };

    let rows = file_tree(&r.files)
        .into_iter()
        .map(|row| tree_row(row, files, ui))
        .collect_view();

    let sent = r.sent;
    let sections = (0..r.files.len())
        .map(|index| file_section(store, r.clone(), index, sent, lines, ui, key))
        .collect_view();

    view! {
        <div class="ui-vstack ui-gap-12">
            <div class="ui-hstack ui-justify-between">
                <span class="t-subheadline t-semibold" aria-live="polite">
                    {progress}
                </span>
                {stat_view(adds, dels)}
            </div>
            <ul class="ck-tree" aria-label=t!(ReviewPanelFilesTree)>
                {rows}
            </ul>
            {sections}
        </div>
    }
    .into_any()
}

fn tree_row(row: TreeRow, files: StoredValue<Vec<FileDiff>>, ui: PanelUi) -> impl IntoView {
    let indent = format!("padding-inline-start: {}px", 8 + row.depth * 16);
    let stat = stat_view(row.additions, row.deletions);
    match row.file {
        None => view! {
            <li class="ck-tree__row ck-tree__row--dir" style=indent>
                <span class="ck-ellipsis t-mono">{format!("{}/", row.name)}</span>
                {stat}
            </li>
        }
        .into_any(),
        Some(index) => {
            let mark = move || {
                ui.viewed
                    .with(|v| files.with_value(|f| v.is_viewed(&f[index])))
            };
            view! {
                <li class="ck-tree__row" style=indent>
                    <button
                        type="button"
                        class="ck-tree__file"
                        on:click=move |_| {
                            let name = files.with_value(|f| f[index].name.clone());
                            set_open(ui, &name, true);
                            show_file(index);
                        }
                    >
                        <span class="ck-ellipsis t-mono">{row.name}</span>
                    </button>
                    {move || {
                        mark()
                            .then(|| {
                                view! {
                                    <span class="ck-tree__done" role="img" aria-label=t!(ReviewPanelViewedAria)>
                                        <Icon path=icons::CHECK size=13 stroke=2.6 />
                                    </span>
                                }
                            })
                    }}
                    {stat}
                </li>
            }
            .into_any()
        }
    }
}

fn file_section(
    store: Store,
    r: Review,
    index: usize,
    sent: bool,
    lines: LineUi,
    ui: PanelUi,
    key: StoredValue<PrKey>,
) -> impl IntoView {
    let file = r.files[index].clone();
    let name = StoredValue::new(file.name.clone());
    let (additions, deletions) = file.counts();
    let diff = StoredValue::new(file);
    let r = StoredValue::new(r);
    let is_viewed = move || ui.viewed.with(|v| diff.with_value(|f| v.is_viewed(f)));
    let open = move || is_open(ui, &name.get_value(), is_viewed());

    let on_check = move |ev: leptos::ev::Event| {
        let checked = event_target_checked(&ev);
        ui.viewed.update(|v| diff.with_value(|f| v.set(f, checked)));
        // Looked at → closes, like on GitHub; unchecked → open again.
        set_open(ui, &name.get_value(), !checked);
        ui.viewed
            .with_untracked(|v| key.with_value(|k| storage::save(k, v)));
    };

    let body = move || {
        if !open() {
            return ().into_any();
        }
        let name = name.get_value();
        let f = diff.get_value();
        if f.lines.is_empty() {
            return view! {
                <p class="t-footnote t-secondary">
                    {t!(ReviewPanelNoTextGithub)}
                </p>
            }
            .into_any();
        }
        let all = ui.more.with(|m| m.contains(&name));
        let total = f.lines.len();
        let limit = if all { total } else { total.min(SHOWN_LINES) };
        let mut rows = view::diff_rows(&f);
        rows.truncate(limit);
        let selected = Signal::derive({
            let name = name.clone();
            move || {
                lines
                    .open
                    .with(|o| o.as_ref().filter(|(f, _)| *f == name).map(|(_, i)| *i))
            }
        });
        // After sending there is nothing left to comment on; the comments stay visible.
        let on_select = (!sent).then(|| {
            let name = name.clone();
            Callback::new(move |index: usize| {
                lines.editing.set(None);
                lines.suggest.set(false);
                let same = lines
                    .open
                    .with_untracked(|o| o.as_ref().is_some_and(|(f, i)| *f == name && *i == index));
                lines.open.set((!same).then(|| (name.clone(), index)));
            })
        });
        let notes = {
            let name = name.clone();
            let r = r.get_value();
            Callback::new(move |index: usize| diff::line_notes(store, &r, &name, index, lines))
        };
        let rest = total - limit;
        view! {
            <Diff
                rows=rows
                label=name.clone()
                on_select=on_select
                selected=selected
                below=Some(notes)
            />
            {(rest > 0)
                .then(|| {
                    let name = name.clone();
                    view! {
                        <Button
                            size=ControlSize::Small
                            on_click=move |_| ui.more.update(|m| m.push(name.clone()))
                        >
                            {t!(ReviewPanelShowMore, rest = rest)}
                        </Button>
                    }
                })}
        }
        .into_any()
    };

    view! {
        <section class="ck-file" id=anchor(index)>
            <div class="ck-file__head">
                <button
                    type="button"
                    class="ck-file__toggle"
                    aria-expanded=move || open().to_string()
                    on:click=move |_| set_open(ui, &name.get_value(), !open())
                >
                    <span class="ck-file__chevron" data-open=move || open().to_string()>
                        <Icon path=icons::CHEVRON_ROW size=14 stroke=2.4 />
                    </span>
                    <span class="ck-ellipsis t-subheadline t-semibold t-mono">{name.get_value()}</span>
                </button>
                {stat_view(additions, deletions)}
                <label class="ck-file__viewed t-footnote">
                    <input
                        type="checkbox"
                        prop:checked=is_viewed
                        on:change=on_check
                    />
                    {t!(ReviewPanelViewed)}
                </label>
            </div>
            {body}
        </section>
    }
}
