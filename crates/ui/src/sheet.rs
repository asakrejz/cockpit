use leptos::prelude::*;

use crate::{Button, ButtonShape, ButtonStyle, ControlSize, Icon, icons};

/// Floating sheet with dimming. Fills the nearest positioned
/// ancestor (`position: relative/absolute`), e.g. a phone frame.
///
/// The "✕" closes without, the "✓" closes calling `on_confirm`.
#[component]
pub fn Sheet(
    open: RwSignal<bool>,
    #[prop(into)] title: String,
    #[prop(optional, into)] on_confirm: Option<Callback<()>>,
    children: ChildrenFn,
) -> impl IntoView {
    let title = StoredValue::new(title);
    view! {
        <Show when=move || open.get()>
            <div class="ui-sheet-overlay">
                <div class="ui-sheet" data-detent="floating" role="dialog" aria-label=title.get_value()>
                    <div class="ui-sheet__grabber"></div>
                    <div
                        class="ui-hstack"
                        style="justify-content: space-between; flex-wrap: nowrap; margin-bottom: 16px"
                    >
                        <Button
                            variant=ButtonStyle::Glass
                            shape=ButtonShape::Circle
                            size=ControlSize::Large
                            aria_label=crate::t!(UiClose)
                            on_click=move |_| open.set(false)
                        >
                            <Icon path=icons::CLOSE size=16 stroke=2.6 />
                        </Button>
                        <p class="t-headline">{title.get_value()}</p>
                        <Button
                            variant=ButtonStyle::GlassProminent
                            shape=ButtonShape::Circle
                            size=ControlSize::Large
                            aria_label=crate::t!(UiSave)
                            on_click=move |_| {
                                if let Some(cb) = on_confirm {
                                    cb.run(());
                                }
                                open.set(false);
                            }
                        >
                            <Icon path=icons::CHECK size=18 stroke=2.6 />
                        </Button>
                    </div>
                    {children()}
                </div>
            </div>
        </Show>
    }
}
