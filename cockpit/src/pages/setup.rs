//! Einrichten: Jira, GitHub und den Agenten verbinden.
//!
//! Das Format der Eingaben wird wirklich geprüft. Die Verbindung selbst ist in dieser
//! Demo simuliert. Schlüssel leben nur in den Feldern dieser Seite: Sie werden nie
//! gespeichert und verschwinden, sobald die Seite verlassen wird. In der Übersicht lassen
//! sich außerdem die Demo-Daten zurücksetzen.

use std::time::Duration;

use cockpit_core::{
    Action, Connections,
    connections::{check_agent, check_github, check_jira},
};
use feature_ui::*;
use leptos::{html, prelude::*};
use web_sys::wasm_bindgen::JsCast;

use crate::mode_switch::ModeSwitch;
use crate::store::{Mode, use_store};
use crate::sync_status::SyncStatus;

const STEPS: &[&str] = &["Jira", "GitHub", "Agent"];
/// Nach dem letzten Schritt folgt die Übersicht.
const OVERVIEW: usize = 3;
/// So lange „prüft“ die simulierte Verbindung.
const CHECK_TIME: Duration = Duration::from_millis(700);

/// Stand der Prüfung einer Verbindung.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Check {
    Untested,
    Testing,
    Failed(String),
    Connected,
}

/// Eine Verbindung im Formular: ihr Stand und wie sie sich in [`Connections`] einträgt.
#[derive(Clone, Copy)]
struct Link {
    check: RwSignal<Check>,
    set: fn(&mut Connections, bool),
    /// Text für „verbunden“: „Verbunden“ oder „Bereit“.
    ready: &'static str,
}

impl Link {
    fn new(connected: bool, set: fn(&mut Connections, bool), ready: &'static str) -> Self {
        let check = if connected {
            Check::Connected
        } else {
            Check::Untested
        };
        Self {
            check: RwSignal::new(check),
            set,
            ready,
        }
    }

    fn connected(&self) -> Signal<bool> {
        let check = self.check;
        Signal::derive(move || check.get() == Check::Connected)
    }

    /// Prüft die Eingaben. Stimmen sie, steht die Verbindung nach kurzer Wartezeit.
    fn test(&self, input: Result<(), String>) {
        let store = use_store();
        let (check, set) = (self.check, self.set);
        match input {
            Err(message) => check.set(Check::Failed(message)),
            Ok(()) => {
                check.set(Check::Testing);
                set_timeout(
                    move || {
                        // Nur wenn die Prüfung noch läuft: Die Seite kann inzwischen verlassen
                        // oder die Eingabe geändert worden sein.
                        if check.try_get_untracked() == Some(Check::Testing) {
                            check.set(Check::Connected);
                            store.conn.update(|c| set(c, true));
                        }
                    },
                    CHECK_TIME,
                );
            }
        }
    }

    /// Geänderte Eingaben machen eine Prüfung ungültig.
    fn invalidate(&self) {
        if self.check.get_untracked() != Check::Untested {
            self.check.set(Check::Untested);
            let set = self.set;
            use_store().conn.update(|c| set(c, false));
        }
    }

    fn status(&self) -> impl IntoView + use<> {
        let (check, ready) = (self.check, self.ready);
        let tone = Signal::derive(move || match check.get() {
            Check::Untested => StatusTone::Gray,
            Check::Testing => StatusTone::Blue,
            Check::Failed(_) => StatusTone::Red,
            Check::Connected => StatusTone::Green,
        });
        view! {
            <Status tone=tone>
                {move || match check.get() {
                    Check::Untested => "Noch nicht geprüft".to_string(),
                    Check::Testing => "Prüfe …".to_string(),
                    Check::Failed(message) => message,
                    Check::Connected => ready.to_string(),
                }}
            </Status>
        }
    }
}

/// Ruft `on_change` auf, sobald sich eines der Felder ändert – nicht beim ersten Lauf.
fn watch(fields: Vec<RwSignal<String>>, on_change: impl Fn() + 'static) {
    Effect::new(move |first_run: Option<()>| {
        for field in &fields {
            field.track();
        }
        if first_run.is_some() {
            on_change();
        }
    });
}

#[component]
pub fn SetupPage() -> impl IntoView {
    let store = use_store();
    let conn = store.conn.get_untracked();
    let step = RwSignal::new(if conn.all_connected() { OVERVIEW } else { 0 });

    let jira = Link::new(conn.jira, |c, on| c.jira = on, "Verbunden");
    let github = Link::new(conn.github, |c, on| c.github = on, "Verbunden");
    let agent = Link::new(conn.agent, |c, on| c.agent = on, "Bereit");

    // Eingaben. Sie gehören dieser Seite und werden nirgends abgelegt.
    let jira_url = RwSignal::new(String::new());
    let jira_mail = RwSignal::new(String::new());
    let jira_token = RwSignal::new(String::new());
    let github_token = RwSignal::new(String::new());
    let github_repos = RwSignal::new(String::new());
    let agent_key = RwSignal::new(String::new());
    watch(vec![jira_url, jira_mail, jira_token], move || {
        jira.invalidate()
    });
    watch(vec![github_token, github_repos], move || {
        github.invalidate()
    });
    watch(vec![agent_key], move || agent.invalidate());

    // Was der Agent darf, gilt sofort.
    let may_branch = RwSignal::new(conn.may_branch);
    let may_open_pr = RwSignal::new(conn.may_open_pr);
    let may_run_tests = RwSignal::new(conn.may_run_tests);
    Effect::new(move |_| {
        let (branch, pr, tests) = (may_branch.get(), may_open_pr.get(), may_run_tests.get());
        store.conn.update(|c| {
            c.may_branch = branch;
            c.may_open_pr = pr;
            c.may_run_tests = tests;
        });
    });

    let next = move |_| step.update(|s| *s += 1);
    let back = move |_| step.update(|s| *s = s.saturating_sub(1));
    let disconnect = move |_| {
        for link in [jira, github, agent] {
            link.check.set(Check::Untested);
        }
        store.conn.update(|c| {
            c.jira = false;
            c.github = false;
            c.agent = false;
        });
        step.set(0);
    };

    // Fußzeile eines Schritts: links prüfen, rechts weiter.
    let actions = move |link: Link, test: Callback<()>, forward: &'static str, first: bool| {
        view! {
            <HStack css="justify-content: space-between">
                <HStack spacing="12px">
                    <Button on_click=move |_| test.run(())>"Verbindung testen"</Button>
                    {link.status()}
                </HStack>
                <HStack>
                    {(!first)
                        .then(|| {
                            view! {
                                <Button variant=ButtonStyle::Borderless size=ControlSize::Large on_click=back>
                                    "Zurück"
                                </Button>
                            }
                        })}
                    <Button
                        variant=ButtonStyle::GlassProminent
                        size=ControlSize::Large
                        disabled=Signal::derive(move || !link.connected().get())
                        on_click=next
                    >
                        {forward}
                    </Button>
                </HStack>
            </HStack>
        }
    };

    let jira_step = move || {
        let test = Callback::new(move |_| {
            jira.test(check_jira(
                &jira_url.get_untracked(),
                &jira_mail.get_untracked(),
                &jira_token.get_untracked(),
            ));
        });
        view! {
            <VStack spacing="20px">
                <Intro
                    title="Jira verbinden"
                    text="Damit Cockpit deine Tickets und die deines Teams lesen, kommentieren und den Status ändern kann."
                />
                <Card>
                    <CardBody css="gap: 16px">
                        <TextField
                            label="Jira-Adresse"
                            value=jira_url
                            input_type="url"
                            placeholder="https://[FIRMA].atlassian.net"
                        />
                        <TextField
                            label="E-Mail deines Atlassian-Kontos"
                            value=jira_mail
                            input_type="email"
                            placeholder="name@beispiel.de"
                        />
                        <TextField
                            label="API-Token"
                            value=jira_token
                            input_type="password"
                            placeholder="••••••••••••••••••••"
                            hint="Erstellen unter: Atlassian-Konto › Sicherheit › API-Token."
                        />
                    </CardBody>
                </Card>
                {actions(jira, test, "Weiter", true)}
            </VStack>
        }
    };

    let github_step = move || {
        let test = Callback::new(move |_| {
            let input = check_github(&github_token.get_untracked(), &github_repos.get_untracked());
            github.test(input.map(|_| ()));
        });
        view! {
            <VStack spacing="20px">
                <Intro
                    title="GitHub verbinden"
                    text="Für Pull Requests, Reviews und Branches. Am besten ein fein abgestuftes Token nur für die Repositories, die du hier brauchst."
                />
                <Card>
                    <CardBody css="gap: 16px">
                        <TextField
                            label="Personal Access Token"
                            value=github_token
                            input_type="password"
                            placeholder="github_pat_••••••••••••"
                            hint="Erstellen unter: Settings › Developer settings › Fine-grained tokens."
                        />
                        <TextField
                            label="Repositories"
                            value=github_repos
                            placeholder="[ORGANISATION]/[REPOSITORY]"
                            hint="Mehrere mit Komma trennen."
                        />
                    </CardBody>
                </Card>
                <List>
                    <ListSection header="Benötigte Berechtigungen">
                        <ListRow title="Pull requests" subtitle="Reviews lesen und senden" detail="Lesen & Schreiben" />
                        <ListRow title="Contents" subtitle="Branches erstellen, Code lesen" detail="Lesen & Schreiben" />
                        <ListRow title="Issues" subtitle="Kommentare und Verknüpfungen" detail="Lesen & Schreiben" />
                        <ListRow title="Metadata" subtitle="Pflicht für jedes Token" detail="Lesen" />
                    </ListSection>
                </List>
                {actions(github, test, "Weiter", false)}
            </VStack>
        }
    };

    let agent_step = move || {
        let test = Callback::new(move |_| agent.test(check_agent(&agent_key.get_untracked())));
        view! {
            <VStack spacing="20px">
                <Intro
                    title="Agent verbinden"
                    text="Der Agent setzt Tickets um und bereitet Reviews vor. Du legst fest, was er selbstständig darf."
                />
                <Card>
                    <CardBody css="gap: 16px">
                        <TextField
                            label="API-Key des Modell-Anbieters"
                            value=agent_key
                            input_type="password"
                            placeholder="sk-••••••••••••••••"
                        />
                    </CardBody>
                </Card>
                <List>
                    <ListSection
                        header="Was der Agent darf"
                        footer="Der Agent schreibt nie direkt in den Hauptbranch und führt keine Merges aus."
                    >
                        <ListRow title="Branches erstellen" toggle_row=true>
                            <Toggle checked=may_branch />
                        </ListRow>
                        <ListRow title="Pull Requests erstellen" subtitle="Für dein Review – der Agent merged nie" toggle_row=true>
                            <Toggle checked=may_open_pr />
                        </ListRow>
                        <ListRow title="Tests vor dem Pull Request ausführen" toggle_row=true>
                            <Toggle checked=may_run_tests />
                        </ListRow>
                        <ListRow title="Kommentare und Reviews senden" subtitle="Nur nach deiner Freigabe – nicht abschaltbar">
                            <Status tone=StatusTone::Green>"Freigabe nötig"</Status>
                        </ListRow>
                    </ListSection>
                </List>
                {actions(agent, test, "Fertigstellen", false)}
            </VStack>
        }
    };

    let overview = move || {
        let row = |title: &'static str, link: Link| {
            view! { <ListRow title=title>{link.status()}</ListRow> }
        };
        view! {
            <VStack spacing="20px">
                <List>
                    <ListSection
                        header="Alles verbunden"
                        prominent=true
                        footer="Mit „Zurück“ änderst du eine Verbindung oder was der Agent darf."
                    >
                        {row("Jira", jira)}
                        {row("GitHub", github)}
                        {row("Agent", agent)}
                    </ListSection>
                </List>
                <ModeSwitch />
                <SyncStatus />
                <ResetDemo />
                <HStack css="justify-content: space-between">
                    <HStack>
                        <Button variant=ButtonStyle::Borderless size=ControlSize::Large on_click=back>
                            "Zurück"
                        </Button>
                        <Button
                            variant=ButtonStyle::Borderless
                            size=ControlSize::Large
                            destructive=true
                            on_click=disconnect
                        >
                            "Alle trennen"
                        </Button>
                    </HStack>
                    <Button variant=ButtonStyle::GlassProminent size=ControlSize::Large href="#/">
                        "Zum Dashboard"
                    </Button>
                </HStack>
            </VStack>
        }
    };

    view! {
        <div class="ck-setup">
            <Container readable=true>
                <VStack spacing="28px">
                    <VStack spacing="10px">
                        <HStack spacing="10px">
                            <span style="display: inline-flex; color: var(--sys-color-tint)">
                                <Icon path=icons::BRAND size=26 />
                            </span>
                            <span class="t-title3 t-bold">"Cockpit"</span>
                            <Badge variant=BadgeVariant::Neutral>
                                {move || if use_store().mode.get() == Mode::Server { "Server" } else { "Demo" }}
                            </Badge>
                        </HStack>
                        <h1 class="t-display">"Einrichten."</h1>
                        <p class="t-title3 t-secondary">
                            "Drei Verbindungen, dann siehst du Tickets, Pull Requests und Agenten an einem Ort."
                        </p>
                    </VStack>

                    <Steps labels=STEPS current=step />

                    {move || match step.get() {
                        0 => jira_step().into_any(),
                        1 => github_step().into_any(),
                        2 => agent_step().into_any(),
                        _ => overview().into_any(),
                    }}

                    <p class="t-footnote t-secondary" style="padding-inline: 4px">
                        "Wo deine Schlüssel gespeichert werden: nirgends. Sie stehen nur in den Feldern dieser Seite und verschwinden, wenn du sie verlässt. Im Browser bleiben nur die Haken „verbunden“, die Berechtigungen des Agenten und der Stand der Beispieldaten (gelesen, Status, Kommentare, Reviews, Agenten-Läufe). Dies ist eine Demo – Cockpit prüft das Format deiner Eingaben, verbindet sich aber noch nicht mit Jira, GitHub oder einem Modell-Anbieter."
                    </p>
                </VStack>
            </Container>
        </div>
    }
}

/// Überschrift und Erklärung eines Schritts.
#[component]
fn Intro(title: &'static str, text: &'static str) -> impl IntoView {
    view! {
        <div class="ui-list__section">
            <p class="ui-list__header" data-prominent="">{title}</p>
            <p class="t-subheadline t-secondary" style="padding-inline: 4px">{text}</p>
        </div>
    }
}

/// „Demo-Daten zurücksetzen“ mit Rückfrage. Die Rückfrage ist ein eigener Block mit
/// `role="alertdialog"`: Der Fokus springt auf „Abbrechen“ (die sichere Wahl),
/// Esc bricht ab, beide Antworten sind echte Buttons.
#[component]
fn ResetDemo() -> impl IntoView {
    let store = use_store();
    let asking = RwSignal::new(false);
    let cancel_box = NodeRef::<html::Div>::new();
    let ask_box = NodeRef::<html::Div>::new();

    // Fokus: beim Öffnen auf „Abbrechen“, beim Schließen zurück auf „Zurücksetzen“.
    let focus_in = move |node: NodeRef<html::Div>| {
        if let Some(button) = node
            .get_untracked()
            .and_then(|b| b.query_selector("button").ok().flatten())
            .and_then(|b| b.dyn_into::<web_sys::HtmlElement>().ok())
        {
            let _ = button.focus();
        }
    };
    Effect::new(move |previous: Option<bool>| {
        let open = asking.get();
        if previous == Some(open) {
            return open;
        }
        // Erst nach dem Zeichnen, sonst gibt es den Button noch nicht.
        let target = if open { cancel_box } else { ask_box };
        if previous.is_some() {
            set_timeout(move || focus_in(target), Duration::ZERO);
        }
        open
    });

    let confirm = move |_| {
        let _ = store.dispatch(Action::ResetDemo);
        asking.set(false);
    };

    view! {
        <List>
            <ListSection
                header="Demo-Daten"
                footer="Gelesen, Status, Branches, Kommentare, Reviews und Agenten-Läufe bleiben beim Neuladen erhalten. Deine Verbindungen bleiben auch nach dem Zurücksetzen."
            >
                <ListRow
                    title="Demo-Daten zurücksetzen"
                    subtitle="Tickets, Pull Requests, Reviews und Agenten beginnen wieder von vorn"
                    wrap=true
                >
                    <div node_ref=ask_box>
                        <Button
                            destructive=true
                            disabled=Signal::derive(move || asking.get())
                            on_click=move |_| asking.set(true)
                        >
                            "Zurücksetzen …"
                        </Button>
                    </div>
                </ListRow>
            </ListSection>
        </List>
        <Show when=move || asking.get()>
            <div
                role="alertdialog"
                aria-labelledby="ck-reset-title"
                aria-describedby="ck-reset-text"
                on:keydown=move |ev| {
                    if ev.key() == "Escape" {
                        asking.set(false);
                    }
                }
            >
                <Card>
                    <CardBody css="gap: 12px">
                        <p id="ck-reset-title" class="t-headline">"Demo-Daten wirklich zurücksetzen?"</p>
                        <p id="ck-reset-text" class="t-subheadline t-secondary">
                            "Alle Änderungen an Tickets, Reviews und Agenten-Läufen gehen verloren und die Beispieldaten beginnen von vorn. Deine Verbindungen bleiben."
                        </p>
                        <HStack>
                            <div node_ref=cancel_box>
                                <Button size=ControlSize::Large on_click=move |_| asking.set(false)>
                                    "Abbrechen"
                                </Button>
                            </div>
                            <Button
                                variant=ButtonStyle::GlassProminent
                                size=ControlSize::Large
                                destructive=true
                                on_click=confirm
                            >
                                "Ja, zurücksetzen"
                            </Button>
                        </HStack>
                    </CardBody>
                </Card>
            </div>
        </Show>
    }
}
