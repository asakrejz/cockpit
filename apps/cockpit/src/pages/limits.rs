//! Settings for the agents: prices and limits (budget, time limit, parallel runs),
//! plus the rules per repo. Saved in the workspace through actions (`SetAgentLimits`,
//! `SetRepoRules`); what applies and how it is checked is in `cockpit_core::usage` and `::rules`.

use cockpit_core::{
    Action, AgentLimits, RepoRules,
    usage::{format_euro, parse_euro},
};
use feature_ui::*;
use leptos::prelude::*;

use crate::{sidebar::AppSidebar, store::use_store};

/// Text of an amount of money for an input field: `5,00`, empty = no limit.
fn money_text(cents: Option<u64>) -> String {
    cents
        .map(|c| format_euro(c).replace(" €", ""))
        .unwrap_or_default()
}

/// Empty = no limit; otherwise an amount in euro. `None` for nonsense.
fn parse_optional_euro(text: &str) -> Option<Option<u64>> {
    if text.trim().is_empty() {
        Some(None)
    } else {
        parse_euro(text).map(Some)
    }
}

fn parse_optional_minutes(text: &str) -> Option<Option<u32>> {
    let text = text.trim();
    if text.is_empty() {
        Some(None)
    } else {
        text.parse::<u32>().ok().map(Some)
    }
}

fn bad_euro(t: &str) -> bool {
    parse_optional_euro(t).is_none() || t.trim() == "0"
}

fn bad_price(t: &str) -> bool {
    parse_euro(t).is_none()
}

fn bad_minutes(t: &str) -> bool {
    parse_optional_minutes(t).is_none() || t.trim() == "0"
}

/// One row per entry.
fn lines(text: &str) -> Vec<String> {
    text.lines().map(str::to_string).collect()
}

/// Usage today and this month, with the limits next to it.
#[component]
fn Spend() -> impl IntoView {
    let store = use_store();
    let today = Signal::derive(move || store.ws.with(|w| w.spent_today(store.now.get())));
    let month = Signal::derive(move || store.ws.with(|w| w.spent_month(store.now.get())));
    let budget = Signal::derive(move || store.ws.with(|w| w.limits.month_budget_cents));
    view! {
        <List>
            <ListSection
                header="Verbrauch"
                footer="Tag und Monat zählen nach UTC. Abgebrochene Läufe zählen mit."
            >
                <ListRow title="Heute" wrap=true>
                    <span class="t-footnote t-secondary">{move || today.get().line()}</span>
                </ListRow>
                <ListRow title="Diesen Monat" wrap=true>
                    <span class="t-footnote t-secondary">
                        {move || {
                            let spent = month.get();
                            match budget.get() {
                                Some(b) => format!("{} · Budget {}", spent.line(), format_euro(b)),
                                None => spent.line(),
                            }
                        }}
                    </span>
                </ListRow>
            </ListSection>
        </List>
    }
}

/// Prices and limits. Saved with the button; invalid input is marked red.
#[component]
fn Limits() -> impl IntoView {
    let store = use_store();
    let l = store.ws.with_untracked(|w| w.limits.clone());
    let run_budget = RwSignal::new(money_text(l.run_budget_cents));
    let month_budget = RwSignal::new(money_text(l.month_budget_cents));
    let minutes = RwSignal::new(l.run_minutes.map(|m| m.to_string()).unwrap_or_default());
    let parallel = RwSignal::new(l.max_parallel.min(99) as i32);
    let price_in = RwSignal::new(money_text(Some(l.pricing.input_cents_per_mtok)));
    let price_out = RwSignal::new(money_text(Some(l.pricing.output_cents_per_mtok)));
    let message = RwSignal::new(None::<(String, bool)>);

    let draft = move || {
        let mut limits = store.ws.with_untracked(|w| w.limits.clone());
        limits.run_budget_cents = parse_optional_euro(&run_budget.get())?;
        limits.month_budget_cents = parse_optional_euro(&month_budget.get())?;
        limits.run_minutes = parse_optional_minutes(&minutes.get())?;
        limits.max_parallel = parallel.get().max(1) as u32;
        limits.pricing.input_cents_per_mtok = parse_euro(&price_in.get())?;
        limits.pricing.output_cents_per_mtok = parse_euro(&price_out.get())?;
        Some(limits).filter(AgentLimits::is_valid)
    };
    let invalid = |f: fn(&str) -> bool, s: RwSignal<String>| Signal::derive(move || f(&s.get()));
    let can_save = Signal::derive(move || draft().is_some());

    let save = move |_| {
        let Some(limits) = draft() else { return };
        message.set(Some(
            match store.dispatch(Action::SetAgentLimits { limits }) {
                Ok(_) => ("Gespeichert.".to_string(), false),
                Err(e) => (e.message(), true),
            },
        ));
    };

    view! {
        <Spend />
        <List>
            <ListSection
                header="Grenzen"
                footer="Erreicht ein Lauf eine Grenze, hält er an. Du erhöhst sie dann und setzt ihn fort – oder brichst ab. Empty lassen heißt: keine Grenze. Das Zeitlimit zählt die Arbeitszeit des Agenten, nicht die Wartezeit auf dich."
            >
                <ListRow title="Läufe gleichzeitig" wrap=true subtitle="Ein weiterer Lauf startet nicht, solange so viele arbeiten (auch wartend auf eine Antwort).">
                    <Stepper value=parallel min=1 max=99 />
                </ListRow>
            </ListSection>
        </List>
        <div class="ck-quiet">
            <TextField
                label="Budget pro Lauf (€)"
                value=run_budget
                placeholder="unbegrenzt"
                error=invalid(bad_euro, run_budget)
                hint="z. B. 5,00"
            />
            <TextField
                label="Budget pro Monat (€)"
                value=month_budget
                placeholder="unbegrenzt"
                error=invalid(bad_euro, month_budget)
                hint="über alle Läufe"
            />
            <TextField
                label="Zeitlimit pro Lauf (Minuten)"
                value=minutes
                placeholder="unbegrenzt"
                error=invalid(bad_minutes, minutes)
            />
        </div>
        <p class="t-subheadline t-bold" style="padding-inline: 4px">"Preise pro Million Token"</p>
        <div class="ck-quiet">
            <TextField
                label="Eingabe (€)"
                value=price_in
                error=invalid(bad_price, price_in)
            />
            <TextField
                label="Ausgabe (€)"
                value=price_out
                error=invalid(bad_price, price_out)
                hint="Kosten werden auf ganze Cent aufgerundet."
            />
        </div>
        <HStack>
            <Button variant=ButtonStyle::GlassProminent disabled=Signal::derive(move || !can_save.get()) on_click=save>
                "Grenzen speichern"
            </Button>
            {move || {
                message
                    .get()
                    .map(|(text, error)| {
                        view! {
                            <span class="t-footnote" class:t-secondary=!error role="status">
                                {text}
                            </span>
                        }
                    })
            }}
        </HStack>
    }
}

/// Rules of a repo: paths, required tests, instructions, plan approval.
#[component]
fn Rules(repo: String) -> impl IntoView {
    let store = use_store();
    let repo = StoredValue::new(repo);
    let current = store.ws.with_untracked(|w| w.rules_for(&repo.get_value()));
    let allowed = RwSignal::new(current.allowed_paths.join("\n"));
    let forbidden = RwSignal::new(current.forbidden_paths.join("\n"));
    let tests = RwSignal::new(current.required_tests.join("\n"));
    let instructions = RwSignal::new(current.instructions.clone());
    let plan_required = RwSignal::new(current.plan_required);
    let message = RwSignal::new(None::<(String, bool)>);

    let save = move |_| {
        let rules = RepoRules {
            allowed_paths: lines(&allowed.get_untracked()),
            forbidden_paths: lines(&forbidden.get_untracked()),
            required_tests: lines(&tests.get_untracked()),
            instructions: instructions.get_untracked(),
            plan_required: plan_required.get_untracked(),
        };
        let result = store.dispatch(Action::SetRepoRules {
            repo: repo.get_value(),
            rules,
        });
        message.set(Some(match result {
            Ok(_) => (
                "Gespeichert. Gilt für neue Läufe und wartende Pläne.".to_string(),
                false,
            ),
            Err(e) => (e.message(), true),
        }));
    };

    view! {
        <List>
            <ListSection
                header=format!("Regeln für {}", repo.get_value())
                footer="Ein Muster pro Zeile. * steht für beliebige Zeichen in einem Ordnernamen, ** für beliebig viele Ordner; ohne / gilt ein Muster in jedem Ordner (*.lock). Verboten schlägt Erlaubt."
            >
                <ListRow title="Plan-Freigabe ist Pflicht" toggle_row=true>
                    <Toggle checked=plan_required />
                </ListRow>
            </ListSection>
        </List>
        <div class="ck-rules">
            <div class="ui-vstack" style="--spacing: 6px">
                <p class="t-subheadline t-bold">"Erlaubte Pfade"</p>
                <Textarea
                    value=allowed
                    label="Erlaubte Pfade"
                    placeholder="leer = überall, z. B. src/**"
                    min_height="88px"
                />
            </div>
            <div class="ui-vstack" style="--spacing: 6px">
                <p class="t-subheadline t-bold">"Verbotene Pfade"</p>
                <Textarea
                    value=forbidden
                    label="Verbotene Pfade"
                    placeholder="z. B. .github/workflows/**"
                    min_height="88px"
                />
            </div>
            <div class="ui-vstack" style="--spacing: 6px">
                <p class="t-subheadline t-bold">"Pflicht-Tests"</p>
                <Textarea
                    value=tests
                    label="Pflicht-Tests, ein Befehl pro Zeile"
                    placeholder="Befehle vor dem Pull Request, z. B. cargo test"
                    min_height="88px"
                />
            </div>
            <div class="ui-vstack" style="--spacing: 6px">
                <p class="t-subheadline t-bold">"Anweisungen an den Agenten"</p>
                <Textarea
                    value=instructions
                    label="Zusätzliche Anweisungen"
                    placeholder="Freitext, z. B. Kleine Commits, deutsche Kommentare."
                    min_height="88px"
                />
            </div>
        </div>
        <HStack>
            <Button variant=ButtonStyle::GlassProminent on_click=save>
                "Regeln speichern"
            </Button>
            {move || {
                message
                    .get()
                    .map(|(text, error)| {
                        view! {
                            <span class="t-footnote" class:t-secondary=!error role="status">
                                {text}
                            </span>
                        }
                    })
            }}
        </HStack>
    }
}

#[component]
pub fn LimitsPage() -> impl IntoView {
    let store = use_store();
    // Repos from the data plus the ones the agent is configured for (rules can be set for both).
    let mut repos = store.ws.with_untracked(|w| w.repos());
    store.health.with_untracked(|h| {
        for r in h.iter().flat_map(|h| h.agent.repos.repos.iter()) {
            if !repos.contains(&r.repo) {
                repos.push(r.repo.clone());
            }
        }
    });

    view! {
        <SplitView>
            <AppSidebar>{""}</AppSidebar>

            <Pane label="Grenzen und Regeln">
                <PaneHeader>
                    <h1 class="t-large-title t-bold">"Grenzen & Regeln"</h1>
                    <p class="t-subheadline t-secondary">
                        "Was der Agent kosten darf, wie lange er arbeitet und woran er sich pro Repo halten muss."
                    </p>
                </PaneHeader>
                <PaneBody>
                    <div class="ck-settings">
                        <Limits />
                        {repos
                            .into_iter()
                            .map(|repo| view! { <Rules repo=repo /> })
                            .collect_view()}
                    </div>
                </PaneBody>
            </Pane>
        </SplitView>
    }
}
