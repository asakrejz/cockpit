//! Small helpers that several stories need.

use feature_ui::*;
use leptos::prelude::*;

/// Section of a story with a heading and a short reference.
#[component]
pub fn Section(
    #[prop(into)] title: String,
    #[prop(optional, into)] api: Option<String>,
    children: Children,
) -> impl IntoView {
    view! {
        <section class="pg-section">
            <VStack spacing="4px">
                <Text variant=TextStyle::Title1 weight=TextWeight::Bold tag=TextTag::H2>
                    {title}
                </Text>
                {api
                    .map(|a| {
                        view! {
                            <Text variant=TextStyle::Subheadline color=TextColor::Secondary mono=true>
                                {a}
                            </Text>
                        }
                    })}
            </VStack>
            {children()}
        </section>
    }
}

/// Finds a value by its ID (for segmented pickers that store IDs).
pub fn find_by_id<T: Copy>(all: &[T], id: &str, id_of: fn(T) -> &'static str) -> Option<T> {
    all.iter().copied().find(|&item| id_of(item) == id)
}

/// Small label in monospace, e.g. a token or attribute name.
#[component]
pub fn Token(children: Children) -> impl IntoView {
    view! { <span class="doc-token">{children()}</span> }
}
