//! The entries that are stuck: the empty state and one row per entry.

use cockpit_core::{Stuck, StuckKind};
use feature_ui::*;
use leptos::prelude::*;

use super::labels::{kind_icon, kind_label, kind_tone};
use super::person_avatar;
use crate::{route::Route, store::Store, view};

/// No entry is stuck: that is good news and should look like it.
pub(super) fn empty_state() -> impl IntoView {
    view! {
        <EmptyState
            title=t!(OverviewNoneStuckTitle)
            text=t!(OverviewNoneStuckText)
            icon=icons::CHECK
        />
    }
}

/// An entry that is stuck: reason, who is on it, and the way there.
pub(super) fn stuck_row(store: Store, s: Stuck) -> impl IntoView {
    let id = s.item_id.clone();
    let key = s.key.clone();
    let tone = view::tone(kind_tone(s.kind)).id();
    view! {
        <li class="ck-stuck">
            <span
                class="ui-list__icon ck-reason"
                style=format!("background: var(--sys-color-{tone})")
                aria-hidden="true"
            >
                <Icon path=kind_icon(s.kind) size=13 stroke=2.4 />
            </span>
            <div class="ui-vstack ui-gap-6">
                <div class="ui-hstack ui-gap-6">
                    <Badge variant=BadgeVariant::Neutral>{kind_label(s.kind)}</Badge>
                    <span class="t-footnote t-mono t-secondary">{s.key}</span>
                </div>
                <p class="t-headline">{s.title}</p>
                <p class="t-subheadline t-secondary">{move || s.reason.render(lang())}</p>
                <div class="ui-hstack ui-gap-8">
                    <Button size=ControlSize::Small on_click=move |_| store.open_item(&id)>
                        {t!(OverviewToEntry)}
                    </Button>
                    {s
                        .plan
                        .clone()
                        .map(|key| {
                            view! {
                                <Button size=ControlSize::Small href=Route::Agent(Some(key)).hash()>
                                    {t!(OverviewViewPlan)}
                                </Button>
                            }
                        })}
                    {s
                        .question
                        .clone()
                        .map(|key| {
                            view! {
                                <Button size=ControlSize::Small href=Route::Agent(Some(key)).hash()>
                                    {t!(OverviewAnswerQuestion)}
                                </Button>
                            }
                        })}
                    {(s.kind == StuckKind::AgentStopped)
                        .then(|| {
                            view! {
                                <Button size=ControlSize::Small href=Route::Agent(Some(key.clone())).hash()>
                                    {t!(OverviewRaiseLimit)}
                                </Button>
                            }
                        })}
                    {s
                        .review
                        .map(|n| {
                            view! {
                                <Button size=ControlSize::Small href=Route::Review(Some(n)).hash()>
                                    {t!(OverviewOpenReview)}
                                </Button>
                            }
                        })}
                    <span class="ui-spacer"></span>
                    <span class="ui-hstack t-footnote t-secondary ui-gap-6">
                        {t!(OverviewWaitingOn)} {person_avatar(&s.who)}
                        <span>{s.who.name_in(lang())}</span>
                    </span>
                </div>
            </div>
        </li>
    }
}
