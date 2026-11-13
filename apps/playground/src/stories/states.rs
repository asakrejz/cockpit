use feature_ui::*;
use leptos::prelude::*;

use super::{Story, common::Section};

pub const STORY: Story = Story {
    id: "states",
    title: "Zustände",
    group: "Beispiele",
    description: "Leer, lädt, Fehler und Hinweisleiste: jede Fläche sagt, was los ist und was der nächste Schritt ist.",
    api: "EmptyState · ErrorState · StatusBanner · Skeleton",
    render,
};

fn render() -> AnyView {
    view! {
        <Section title="Leerzustand" api="<EmptyState title text icon>{Aktion}</EmptyState>">
            <EmptyState
                title="Noch keine Tickets"
                text="Verbinde Jira und GitHub unter „Einrichten“, dann erscheinen sie hier."
                icon=icons::TASKS
            >
                <Button size=ControlSize::Small variant=ButtonStyle::GlassProminent href="#states">
                    "Einrichten"
                </Button>
            </EmptyState>
        </Section>
        <Section title="Filter ohne Treffer" api="<EmptyState> mit „Filter zurücksetzen“">
            <EmptyState title="Keine Treffer" text="Passe Suche, Quelle oder Filter an." icon=icons::SEARCH>
                <Button size=ControlSize::Small href="#states">"Filter zurücksetzen"</Button>
            </EmptyState>
        </Section>
        <Section title="Lädt" api="<Skeleton rows label>">
            <Skeleton rows=4 />
        </Section>
        <Section title="Fehler" api="<ErrorState title text>{Aktion}</ErrorState> (role=alert)">
            <ErrorState
                title="Server nicht erreichbar"
                text="Cockpit wartet auf den lokalen Server und versucht es automatisch erneut."
            >
                <Button size=ControlSize::Small variant=ButtonStyle::GlassProminent href="#states">
                    "Jetzt erneut versuchen"
                </Button>
            </ErrorState>
        </Section>
        <Section title="Hinweisleiste" api="<StatusBanner tone title text>{Aktionen}</StatusBanner>">
            <div class="ui-vstack">
                <StatusBanner
                    tone=BannerTone::Info
                    title="GitHub: Abgleich pausiert"
                    text="Das Kontingent ist aufgebraucht. Cockpit fragt in 2 Min. wieder an. Letzter Stand von vor 10 Min."
                />
                <StatusBanner
                    tone=BannerTone::Warning
                    title="Jira nicht erreichbar"
                    text="Letzter Stand von vor 12 Min. Cockpit versucht es weiter."
                >
                    <Button size=ControlSize::Small href="#states">"Jetzt abgleichen"</Button>
                </StatusBanner>
                <StatusBanner
                    tone=BannerTone::Error
                    title="GitHub: Token abgelehnt"
                    text="Der Token ist abgelaufen oder widerrufen. Erneuere ihn unter „Einrichten“."
                >
                    <Button size=ControlSize::Small variant=ButtonStyle::GlassProminent href="#states">
                        "Einrichten"
                    </Button>
                </StatusBanner>
            </div>
        </Section>
    }
    .into_any()
}
