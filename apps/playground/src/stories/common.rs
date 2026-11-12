//! Kleine Helfer, die mehrere Stories brauchen.

use feature_ui::*;
use leptos::prelude::*;

/// Abschnitt einer Story mit Überschrift und Kurzreferenz.
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

/// Sucht einen Wert anhand seiner ID (für Segmented-Picker, die IDs speichern).
pub fn find_by_id<T: Copy>(all: &[T], id: &str, id_of: fn(T) -> &'static str) -> Option<T> {
    all.iter().copied().find(|&item| id_of(item) == id)
}

/// Kleine Beschriftung in Monospace, z. B. ein Token- oder Attributname.
#[component]
pub fn Token(children: Children) -> impl IntoView {
    view! { <span class="doc-token">{children()}</span> }
}
