//! Small notices (toasts) for notifications while the tab is in the foreground.
//! They sit at the bottom right, disappear after a few seconds and lead to the target by a link.

use feature_ui::*;
use leptos::prelude::*;

use crate::{store::use_store, view};

#[component]
pub fn Toasts() -> impl IntoView {
    let store = use_store();
    view! {
        <div class="ck-toasts" role="status" aria-live="polite">
            <For each=move || store.toasts.get() key=|t| t.id let:toast>
                {
                    let id = toast.id;
                    let event = toast.event;
                    let (title, text) = (event.title.clone(), event.text.clone());
                    view! {
                        <div class="ck-toast">
                            <span class="ck-toast__icon" aria-hidden="true">
                                <Icon path=view::notify_icon(event.reason) size=18 />
                            </span>
                            <div class="ui-vstack ui-gap-2 ui-grow">
                                <p class="t-headline">{move || title.render(lang())}</p>
                                <p class="t-subheadline t-secondary">{move || text.render(lang())}</p>
                            </div>
                            <Button size=ControlSize::Small href=event.route on_click=move |_| store.dismiss(id)>
                                {t!(ToastOpen)}
                            </Button>
                            <Button
                                variant=ButtonStyle::Borderless
                                size=ControlSize::Small
                                aria_label=t!(ToastClose)
                                on_click=move |_| store.dismiss(id)
                            >
                                <Icon path=icons::CLOSE size=14 />
                            </Button>
                        </div>
                    }
                }
            </For>
        </div>
    }
}
