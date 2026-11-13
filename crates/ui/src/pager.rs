use leptos::prelude::*;

use crate::{Button, ButtonShape, ButtonStyle, ControlSize, Icon, icons};

/// Carousel (`TabView` with `.page`): glass arrows above the content,
/// page dots in a glass capsule below.
///
/// Every page is a view of its own.
#[component]
pub fn Pager(
    pages: Vec<AnyView>,
    /// Label for screen readers.
    #[prop(into)]
    label: String,
) -> impl IntoView {
    let count = pages.len();
    let index = RwSignal::new(0usize);
    let go = move |i: usize| index.set(i.min(count.saturating_sub(1)));
    let offset = move || {
        let i = index.get();
        format!(
            "transform: translateX(calc({}% - {}px))",
            -(i as i32) * 100,
            i * 16
        )
    };

    view! {
        <div class="ui-pager" role="region" aria-roledescription="Karussell" aria-label=label>
            <div class="ui-zstack">
                <div class="ui-pager__viewport">
                    <div class="ui-pager__track" style=offset>
                        {pages
                            .into_iter()
                            .enumerate()
                            .map(|(i, page)| {
                                view! {
                                    <div
                                        class="ui-pager__page"
                                        aria-label=format!("Seite {} von {count}", i + 1)
                                        aria-hidden=move || (index.get() != i).then_some("true")
                                    >
                                        {page}
                                    </div>
                                }
                            })
                            .collect_view()}
                    </div>
                </div>
                <div style="align-self: center; justify-self: start; margin-left: 16px">
                    <Button
                        variant=ButtonStyle::Glass
                        shape=ButtonShape::Circle
                        size=ControlSize::Large
                        aria_label="Vorherige Seite"
                        disabled=Signal::derive(move || index.get() == 0)
                        on_click=move |_| go(index.get().saturating_sub(1))
                    >
                        <Icon path=icons::CHEVRON_LEFT size=20 stroke=2.4 />
                    </Button>
                </div>
                <div style="align-self: center; justify-self: end; margin-right: 16px">
                    <Button
                        variant=ButtonStyle::Glass
                        shape=ButtonShape::Circle
                        size=ControlSize::Large
                        aria_label="Nächste Seite"
                        disabled=Signal::derive(move || index.get() + 1 >= count)
                        on_click=move |_| go(index.get() + 1)
                    >
                        <Icon path=icons::CHEVRON_RIGHT size=20 stroke=2.4 />
                    </Button>
                </div>
            </div>
            <div class="ui-pager__bar">
                <div class="ui-pager__dots">
                    {(0..count)
                        .map(|i| {
                            view! {
                                <button
                                    class="ui-pager__dot"
                                    type="button"
                                    aria-label=format!("Seite {} von {count}", i + 1)
                                    aria-current=move || (index.get() == i).to_string()
                                    on:click=move |_| go(i)
                                ></button>
                            }
                        })
                        .collect_view()}
                </div>
            </div>
        </div>
    }
}
