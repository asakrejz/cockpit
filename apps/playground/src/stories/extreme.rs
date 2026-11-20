use feature_ui::*;
use leptos::prelude::*;

use super::{Story, common::Section};

pub const STORY: Story = Story {
    id: "extreme",
    title: "Extreme Inhalte",
    group: "Inhalt",
    description: "Sehr lange Wörter und URLs, einzelne Wörter und leere Texte: nichts darf ein Pane verbreitern, Karten-Fußzeilen sitzen unten rechts.",
    api: ".ui-card-grid · .ui-card__footer · .ui-ellipsis · .ui-list__row",
    render,
};

const LONG_WORD: &str =
    "Donaudampfschifffahrtsgesellschaftskapitänsmützenabzeichenhalterungsschraube";
const URL: &str = "https://example.invalid/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb?q=1";
const BRANCH: &str = "feature/PROJ-12345-ein-sehr-langer-branch-name-ohne-ein-einziges-leerzeichen-der-nicht-aufhoert";
const SHORT_BODY: &str = "Kurz";

fn card(title: &'static str, body: &'static str, who: &'static str) -> impl IntoView {
    view! {
        <Card>
            <CardBody>
                <p class="t-headline">{title}</p>
                <p class="t-subheadline t-secondary">{body}</p>
            </CardBody>
            <CardFooter>
                <span class="t-footnote t-secondary">{who}</span>
            </CardFooter>
        </Card>
    }
}

fn render() -> AnyView {
    view! {
        <div data-extreme="" style="min-width: 0">
            <Section title="Karten · gleiche Höhe, Fußzeile unten rechts" api="<CardGrid> · <Card> · <CardBody> · <CardFooter>">
                <div class="ui-pane" data-extreme-pane="" style="padding: 16px">
                    <CardGrid>
                        {card("Ein Wort", SHORT_BODY, "Anna")}
                        {card(LONG_WORD, URL, "Bartholomäus-Maximilian von und zu Sehrlangername")}
                        {card("", "", "")}
                        {card("Mittel", "Ein normaler Satz mit etwas mehr Text als bei der kurzen Karte.", "Chris")}
                    </CardGrid>
                </div>
            </Section>
            <Section title="Listenzeilen" api="<ListRow> · .ui-ellipsis">
                <div class="ui-pane" data-extreme-pane="" style="padding: 16px">
                    <List>
                        <ListSection header=LONG_WORD footer=URL>
                            <ListRow title=LONG_WORD subtitle=URL detail=BRANCH chevron=true href="#extreme" />
                            <ListRow title="Kurz" detail="x" />
                            <ListRow title="" />
                            <ListRow title="Repo" wrap=true>
                                <span class="ui-ellipsis" title=BRANCH>{BRANCH}</span>
                            </ListRow>
                        </ListSection>
                    </List>
                    <pre class="doc-code">{format!("let x = \"{URL}\"; // {LONG_WORD}")}</pre>
                </div>
            </Section>
        </div>
    }
    .into_any()
}
