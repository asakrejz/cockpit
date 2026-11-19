//! „Mein Team“: the button with the chosen teams, a hint while none is chosen, and the dialog
//! to choose them. The choice is `Action::SetTeamChoice` (display only – nothing is written to
//! Jira); the rules that apply it are in `cockpit_core::team_filter`.

use cockpit_core::{Action, TeamChoice, TeamRef};
use feature_ui::*;
use leptos::prelude::*;

use crate::store::{Store, use_store};

/// The line under the title of the overview: the button, and the hint if no team is chosen.
#[component]
pub fn TeamPicker() -> impl IntoView {
    let store = use_store();
    let open = RwSignal::new(false);
    let has_teams = move || {
        store
            .ws
            .with(|w| w.has_team_data() || w.team_choice.is_active())
    };
    let label = Signal::derive(move || {
        store.ws.with(|w| {
            if w.team_choice.is_active() {
                let names: Vec<&str> = w
                    .team_choice
                    .teams
                    .iter()
                    .map(|t| t.name.as_str())
                    .collect();
                t!(OverviewTeamChosen, names = names.join(", "))
            } else {
                t!(OverviewTeamButton)
            }
        })
    });
    let hint = move || !store.ws.with(|w| w.team_choice.is_active());
    view! {
        <Show when=has_teams>
            <div class="ui-vstack" style="--spacing: 6px">
                <div>
                    <Button on_click=move |_| open.set(true) aria_label=t!(OverviewTeamButtonOpen)>
                        {move || label.get()}
                    </Button>
                </div>
                <Show when=hint>
                    <p class="t-footnote t-secondary">{t!(OverviewTeamHint)}</p>
                </Show>
            </div>
            {team_dialog(store, open)}
        </Show>
    }
}

/// One switch per team the tickets carry, and „Ohne Team einbeziehen“. Edits go to a draft;
/// „Übernehmen“ applies it.
fn team_dialog(store: Store, open: RwSignal<bool>) -> impl IntoView {
    let draft = RwSignal::new(TeamChoice::default());
    Effect::new(move |_| {
        if open.get() {
            draft.set(store.ws.with_untracked(|w| w.team_choice.clone()));
        }
    });
    let apply = move |_| {
        let choice = draft.get_untracked();
        if let Err(e) = store.dispatch(Action::SetTeamChoice { choice })
            && e != cockpit_core::ActionError::Unchanged
        {
            store.notify(e.msg().render(feature_ui::lang_untracked()));
        }
        open.set(false);
    };
    let reset = move |_| draft.set(TeamChoice::default());
    let body = move || {
        let teams = store.ws.with_untracked(|w| w.teams());
        let none_on = RwSignal::new(draft.with_untracked(|d| d.include_no_team));
        Effect::new(move |previous: Option<()>| {
            let on = none_on.get();
            if previous.is_some() {
                draft.update(|d| d.include_no_team = on);
            }
        });
        let rows = if teams.is_empty() {
            view! { <p class="t-subheadline t-secondary">{t!(OverviewTeamNoneFound)}</p> }
                .into_any()
        } else {
            view! {
                <ul class="ui-sidebar__nav" aria-label=t!(OverviewTeamList)>
                    {teams.into_iter().map(|team| team_switch(draft, team)).collect_view()}
                </ul>
            }
            .into_any()
        };
        view! {
            <div class="ck-dialog">
                <p class="t-footnote t-secondary">{t!(OverviewTeamDialogIntro)}</p>
                {rows}
                <ul class="ui-sidebar__nav">
                    <li class="ck-rule" title=t!(OverviewTeamIncludeNoneHint)>
                        <label class="t-subheadline">
                            <span>{t!(OverviewTeamIncludeNone)}</span>
                            <Toggle checked=none_on aria_label=t!(OverviewTeamIncludeNone) />
                        </label>
                    </li>
                </ul>
                <p class="t-footnote t-secondary">{t!(OverviewTeamIncludeNoneHint)}</p>
            </div>
        }
    };
    let footer = move || {
        view! {
            <Button on_click=reset>{t!(OverviewTeamReset)}</Button>
            <Button variant=ButtonStyle::GlassProminent on_click=apply>
                {t!(OverviewTeamApply)}
            </Button>
        }
    };
    view! {
        <Dialog open=open title=t!(OverviewTeamDialogTitle) footer=footer>
            {body}
        </Dialog>
    }
}

/// The switch of one team; it writes into the draft.
fn team_switch(draft: RwSignal<TeamChoice>, team: TeamRef) -> impl IntoView {
    let id = team.id.clone();
    let on = RwSignal::new(draft.with_untracked(|d| d.contains(&id)));
    let shown = team.clone();
    Effect::new(move |previous: Option<()>| {
        let checked = on.get();
        if previous.is_none() {
            return;
        }
        draft.update(|d| {
            d.teams.retain(|t| t.id != shown.id);
            if checked {
                d.teams.push(shown.clone());
            }
        });
    });
    view! {
        <li class="ck-rule">
            <label class="t-subheadline">
                <span>{team.name.clone()}</span>
                <Toggle checked=on aria_label=team.name />
            </label>
        </li>
    }
}
