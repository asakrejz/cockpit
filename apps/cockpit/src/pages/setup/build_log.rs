//! Output of a long task (the image build): collapsed to the last lines, an arrow at the top right
//! opens the complete output. Which lines are shown is decided by `cockpit_core::setup::log_*`.

use cockpit_core::setup::{log_hidden, log_visible};
use feature_ui::*;
use leptos::html::Pre;
use leptos::prelude::*;

/// The output with its toggle. `expanded` belongs to the caller so that it survives while the
/// lines change (and is reset for each new build).
#[component]
pub fn CollapsibleLog(
    #[prop(into)] lines: Signal<Vec<String>>,
    expanded: RwSignal<bool>,
) -> impl IntoView {
    let pre = NodeRef::<Pre>::new();
    let text = Signal::derive(move || lines.with(|l| log_visible(l, expanded.get())));
    let label = Signal::derive(move || {
        lines.with(|l| match (expanded.get(), log_hidden(l)) {
            (true, _) => t!(SetupBuildCollapse),
            (false, 0) => t!(SetupBuildExpand),
            (false, n) => t!(SetupBuildExpandHidden, n = n),
        })
    });

    // Follow the newest line, as long as the reader is at the bottom (or the box is collapsed).
    Effect::new(move |_| {
        text.track();
        if let Some(el) = pre.get_untracked() {
            let at_end = el.scroll_height() - el.scroll_top() - el.client_height() < 40;
            if at_end || !expanded.get_untracked() {
                el.set_scroll_top(el.scroll_height());
            }
        }
    });

    view! {
        <div class="ck-log" data-expanded=move || expanded.get().then_some("")>
            <pre class="ui-log ck-log__text" node_ref=pre aria-live="polite" tabindex="0">
                {move || text.get()}
            </pre>
            <button
                type="button"
                class="ck-log__toggle"
                aria-expanded=move || expanded.get().to_string()
                aria-label=move || label.get()
                title=move || label.get()
                on:click=move |_| expanded.update(|e| *e = !*e)
            >
                <Icon path=icons::CHEVRON_ROW size=16 />
            </button>
        </div>
    }
}
