use leptos::{ev, prelude::*};

use crate::{Button, ButtonShape, ButtonStyle, ControlSize, Icon, icons};

/// Dialog over the page: a centered card on wide screens, a sheet from the bottom on narrow
/// ones (the CSS decides, the markup is the same). Closes with the "✕", the Escape key or a
/// click on the dimmed area. `children` is the scrolling content, `footer` the buttons.
#[component]
pub fn Dialog(
    open: RwSignal<bool>,
    #[prop(into)] title: String,
    children: ChildrenFn,
    #[prop(into)] footer: ViewFn,
) -> impl IntoView {
    let title = StoredValue::new(title);
    let listener = window_event_listener(ev::keydown, move |event| {
        if open.get_untracked() && event.key() == "Escape" {
            open.set(false);
        }
    });
    on_cleanup(move || listener.remove());
    view! {
        <Show when=move || open.get()>
            <div
                class="ui-dialog-overlay"
                on:click=move |event| {
                    // Only a click on the dimmed area itself, not one inside the dialog.
                    if event.target() == event.current_target() {
                        open.set(false);
                    }
                }
            >
                <div class="ui-dialog" role="dialog" aria-modal="true" aria-label=title.get_value()>
                    <div class="ui-dialog__grabber"></div>
                    <div class="ui-dialog__head">
                        <p class="t-headline">{title.get_value()}</p>
                        <Button
                            variant=ButtonStyle::Glass
                            shape=ButtonShape::Circle
                            size=ControlSize::Small
                            aria_label=crate::t!(UiClose)
                            on_click=move |_| open.set(false)
                        >
                            <Icon path=icons::CLOSE size=14 stroke=2.6 />
                        </Button>
                    </div>
                    <div class="ui-dialog__body">{children()}</div>
                    <div class="ui-dialog__foot">{footer.run()}</div>
                </div>
            </div>
        </Show>
    }
}
