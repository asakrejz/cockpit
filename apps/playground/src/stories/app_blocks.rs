use feature_ui::*;
use leptos::prelude::*;

use super::{Story, common::Section};

pub const STORY: Story = Story {
    id: "app-blocks",
    title: "App-Bausteine",
    group: "Beispiele",
    description: "Bausteine für Arbeits-Apps wie das Cockpit: Status, Avatar, Inbox, Kommentare, Diff, Vorschläge und Fortschritt.",
    api: ".ui-status · .ui-avatar · .ui-chip · .ui-inbox-row · .ui-comment · .ui-diff · .ui-suggestion · .ui-timeline · .ui-steps",
    render,
};

const STEPS: &[&str] = &["Jira", "GitHub", "Agent"];
const VERDICTS: &[(&str, &str)] = &[
    ("comment", "Kommentieren"),
    ("approve", "Genehmigen"),
    ("changes", "Änderungen"),
];

fn render() -> AnyView {
    let selected = RwSignal::new(0usize);
    let step = RwSignal::new(1usize);
    let progress = RwSignal::new(2usize);
    let state = RwSignal::new(SuggestionState::Pending);
    let verdict = RwSignal::new("comment");
    let note = RwSignal::new(String::new());
    // Diff with selectable lines: position of the chosen line.
    let line = RwSignal::new(None::<usize>);

    let timeline_state = move |i: usize| {
        Signal::derive(move || match progress.get() {
            p if i < p => StepState::Done,
            p if i == p => StepState::Active,
            _ => StepState::Pending,
        })
    };

    view! {
        <Section title="Status · Avatar · Chip" api="<Status tone> · <Avatar initials agent large> · <Chip>">
            <Card>
                <CardBody css="gap: 16px">
                    <HStack spacing="16px">
                        {StatusTone::ALL
                            .map(|tone| view! { <Status tone=tone>{tone.id()}</Status> })
                            .into_iter()
                            .collect_view()}
                    </HStack>
                    <HStack spacing="12px">
                        <Avatar initials="DU" />
                        <Avatar initials="LK" large=true />
                        <Avatar agent=true />
                        <Avatar agent=true large=true />
                        <Chip>
                            <Icon path=icons::GIT_BRANCH size=13 stroke=2.2 />
                            "feature/BAU-131-slider-thumb"
                        </Chip>
                        <Chip>"main"</Chip>
                    </HStack>
                </CardBody>
            </Card>
        </Section>

        <Section title="Inbox & Kommentare" api="<InboxRow selected on_select> · <UnreadDot> · <CommentBubble own> · <Textarea>">
            <Grid cols=2 spacing="20px">
                <Pane label="Inbox">
                    <ul style="list-style: none; margin: 0; padding: 8px 0">
                        {["Toggle: Fokus-Ring fehlt", "Slider: Thumb als Kapsel", "Sheet: Detents"]
                            .into_iter()
                            .enumerate()
                            .map(|(i, title)| {
                                view! {
                                    <InboxRow
                                        selected=Signal::derive(move || selected.get() == i)
                                        on_select=move |_| selected.set(i)
                                    >
                                        <UnreadDot read=Signal::derive(move || selected.get() == i) />
                                        <span class="ui-vstack" style="--spacing: 4px; flex: 1 1 auto; min-width: 0">
                                            <span class="ui-hstack" style="--spacing: 6px">
                                                <Badge variant=BadgeVariant::Neutral>"Jira"</Badge>
                                                <span class="t-footnote t-mono t-secondary">{format!("BAU-14{i}")}</span>
                                            </span>
                                            <span class="t-headline">{title}</span>
                                        </span>
                                    </InboxRow>
                                }
                            })
                            .collect_view()}
                    </ul>
                </Pane>
                <VStack spacing="12px">
                    <CommentBubble who="Lena" initials="LK" time="vor 12 Min." text="Betrifft auch den Slider-Thumb." />
                    <CommentBubble who="Du" initials="DU" time="gerade eben" text="Schaue ich mir an." own=true />
                    <Textarea value=note label="Antwort" placeholder="Antworten …" min_height="72px" />
                </VStack>
            </Grid>
        </Section>

        <Section title="Diff mit auswählbaren Zeilen" api="<Diff on_select selected below>">
            <Diff
                label="toggle.css"
                rows=vec![
                    DiffRow { no: 20, kind: DiffKind::Context, text: ".ui-toggle input {".into() },
                    DiffRow { no: 21, kind: DiffKind::Del, text: "  outline: 1px solid red;".into() },
                    DiffRow { no: 21, kind: DiffKind::Add, text: "  outline: none;".into() },
                    DiffRow { no: 22, kind: DiffKind::Context, text: "}".into() },
                ]
                on_select=Some(
                    Callback::new(move |i: usize| line.update(|l| *l = if *l == Some(i) { None } else { Some(i) })),
                )
                selected=line
                below=Some(
                    Callback::new(move |i: usize| {
                        view! {
                            {move || {
                                (line.get() == Some(i))
                                    .then(|| {
                                        view! {
                                            <div class="ui-diff__note">
                                                <span class="t-footnote t-secondary">"Inhalt unter Zeile "{i + 1}</span>
                                                <p class="t-subheadline">
                                                    "Der Slot nimmt beliebige Bausteine auf, hier ein Hinweis."
                                                </p>
                                            </div>
                                        }
                                    })
                            }}
                        }
                            .into_any()
                    }),
                )
            />
        </Section>

        <Section title="Diff & Vorschlag" api="<Diff rows> · <SuggestionCard state> · <SegmentedPicker stretch>">
            <Grid cols=2 spacing="20px">
                <Diff
                    label="components.css"
                    rows=vec![
                        DiffRow { no: 412, kind: DiffKind::Context, text: ".ui-slider::-webkit-slider-thumb {".into() },
                        DiffRow { no: 413, kind: DiffKind::Del, text: "  width: 28px;".into() },
                        DiffRow { no: 413, kind: DiffKind::Add, text: "  width: var(--comp-slider-thumb-w);".into() },
                        DiffRow { no: 414, kind: DiffKind::Add, text: "  background: #ffffff;".into() },
                        DiffRow { no: 415, kind: DiffKind::Context, text: "}".into() },
                    ]
                />
                <VStack spacing="12px">
                    <SuggestionCard state=state>
                        <div class="ui-hstack" style="--spacing: 8px">
                            <span class="t-footnote t-mono t-secondary">"components.css · Zeile 414"</span>
                            <span class="ui-spacer"></span>
                            <Badge variant=BadgeVariant::Neutral>"Hinweis"</Badge>
                        </div>
                        <p class="t-subheadline">"Hier steht ein fester Farbwert. Komponenten sollen nur Tokens lesen."</p>
                        <HStack spacing="8px">
                            <Button
                                variant=ButtonStyle::BorderedProminent
                                size=ControlSize::Small
                                on_click=move |_| state.set(SuggestionState::Approved)
                            >
                                "Freigeben"
                            </Button>
                            <Button
                                size=ControlSize::Small
                                destructive=true
                                on_click=move |_| state.set(SuggestionState::Declined)
                            >
                                "Ablehnen"
                            </Button>
                            <Button
                                variant=ButtonStyle::Borderless
                                size=ControlSize::Small
                                on_click=move |_| state.set(SuggestionState::Pending)
                            >
                                "Zurücksetzen"
                            </Button>
                        </HStack>
                    </SuggestionCard>
                    <SegmentedPicker
                        options=VERDICTS
                        value=verdict
                        on_change=move |id| verdict.set(id)
                        aria_label="Ergebnis"
                        stretch=true
                    />
                </VStack>
            </Grid>
        </Section>

        <Section title="Fortschritt" api="<Steps labels current> · <Progress value max> · <Timeline> · <Log>">
            <Grid cols=2 spacing="20px">
                <VStack spacing="16px">
                    <Steps labels=STEPS current=step />
                    <Progress value=progress max=3 label="Fortschritt" />
                    <HStack>
                        <Button on_click=move |_| progress.update(|p| *p = (*p + 1) % 4)>"Timeline weiter"</Button>
                        <Button on_click=move |_| step.update(|s| *s = (*s + 1) % 4)>"Schritte weiter"</Button>
                    </HStack>
                    <Log text=Signal::derive(move || {
                        format!("› Schritt {} von 3\n› Onboarding bei {}", progress.get().min(3), step.get())
                    }) />
                </VStack>
                <Timeline>
                    <TimelineStep number=1 state=timeline_state(0) title="Ticket gelesen" text="Beschreibung ausgewertet" />
                    <TimelineStep number=2 state=timeline_state(1) title="Branch erstellt" text="agent/BAU-142 von main" />
                    <TimelineStep number=3 state=timeline_state(2) title="Pull Request erstellt" text="wartet auf dein Review" />
                </Timeline>
            </Grid>
        </Section>
    }
    .into_any()
}
