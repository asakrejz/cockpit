//! Overview: on the left who works on what and how full they are; on the right what is stuck, and
//! the entries of the chosen person. The rules for it are in `cockpit_core::team`.

use cockpit_core::{
    Action, Item, Kind, LoadLevel, Member, PeopleSource, Person, Stuck, StuckKind, Tone,
    people_search,
    team::{self, Group, Side},
};
use feature_ui::*;
use leptos::prelude::*;

use crate::{
    pages::{inbox::labels, team_picker::TeamPicker},
    route::Route,
    store::{Store, navigate, use_store},
    view,
};

/// Line "1 open · 2 in progress" – only what exists.
fn counts(m: &Member) -> String {
    let parts: Vec<String> = [
        (m.open > 0).then(|| t!(OverviewCountOpen, n = m.open)),
        (m.in_progress > 0).then(|| t!(OverviewCountProgress, n = m.in_progress)),
        (m.review > 0).then(|| t!(OverviewCountReview, n = m.review)),
        (m.pull_requests > 0).then(|| t!(OverviewCountPrs, n = m.pull_requests)),
    ]
    .into_iter()
    .flatten()
    .collect();
    if parts.is_empty() {
        t!(OverviewNothingOpen)
    } else {
        parts.join(" · ")
    }
}

fn kind_label(kind: StuckKind) -> String {
    match kind {
        StuckKind::ChecksFailed => t!(OverviewKindChecks),
        StuckKind::AgentStopped => t!(OverviewKindStopped),
        StuckKind::QuestionWaiting => t!(OverviewKindQuestion),
        StuckKind::PlanWaiting => t!(OverviewKindPlan),
        StuckKind::AgentWaiting => t!(OverviewKindAgent),
        StuckKind::ReviewWaiting => t!(OverviewKindReview),
        StuckKind::NoBranch => t!(OverviewKindNoBranch),
    }
}

fn load_label(level: LoadLevel) -> String {
    match level {
        LoadLevel::Free => t!(OverviewLoadFree),
        LoadLevel::Busy => t!(OverviewLoadBusy),
        LoadLevel::Full => t!(OverviewLoadFull),
    }
}

/// What the entry is: the status of a ticket, or the state of a pull request.
fn state_text(item: &Item) -> String {
    match (item.kind, &item.pr) {
        (Kind::PullRequest, Some(pr)) => labels::pr_status(pr),
        (Kind::PullRequest, None) => t!(OverviewPullRequest),
        (Kind::Ticket, _) => labels::ticket_status(item.status),
    }
}

fn kind_icon(kind: StuckKind) -> &'static str {
    match kind {
        StuckKind::ChecksFailed => icons::ALERT,
        StuckKind::AgentStopped => icons::ALERT,
        StuckKind::QuestionWaiting => icons::SPARKLES,
        StuckKind::PlanWaiting => icons::TASKS,
        StuckKind::AgentWaiting => icons::SPARKLE,
        StuckKind::ReviewWaiting => icons::EYE,
        StuckKind::NoBranch => icons::GIT_BRANCH,
    }
}

fn kind_tone(kind: StuckKind) -> Tone {
    match kind {
        StuckKind::ChecksFailed => Tone::Red,
        StuckKind::AgentStopped => Tone::Red,
        StuckKind::QuestionWaiting => Tone::Orange,
        StuckKind::PlanWaiting => Tone::Orange,
        StuckKind::AgentWaiting => Tone::Green,
        StuckKind::ReviewWaiting => Tone::Orange,
        StuckKind::NoBranch => Tone::Indigo,
    }
}

fn group_label(group: Group) -> String {
    match group {
        Group::Both => t!(OverviewGroupBoth),
        Group::Jira => t!(OverviewGroupJira),
        Group::GitHub => t!(OverviewGroupGithub),
    }
}

/// The segmented control „Alle / Jira / GitHub“. It is `Action::SetPeopleSource`, kept in the
/// workspace like the other filters.
fn source_picker(store: Store) -> impl IntoView {
    let current = move || store.ws.with(|w| w.people_source);
    let label = |source: PeopleSource| match source {
        PeopleSource::All => t!(OverviewSourceAll),
        PeopleSource::Jira => t!(OverviewSourceJira),
        PeopleSource::GitHub => t!(OverviewSourceGithub),
    };
    view! {
        <div class="ui-segmented" role="group" aria-label=t!(OverviewSourceLabel) style="align-self: flex-start">
            {PeopleSource::ALL
                .into_iter()
                .map(|source| {
                    view! {
                        <button
                            type="button"
                            aria-pressed=move || (current() == source).to_string()
                            on:click=move |_| {
                                if let Err(e) = store.dispatch(Action::SetPeopleSource { source })
                                    && e != cockpit_core::ActionError::Unchanged
                                {
                                    store.notify(e.msg().render(feature_ui::lang_untracked()));
                                }
                            }
                        >
                            {label(source)}
                        </button>
                    }
                })
                .collect_view()}
        </div>
    }
}

fn person_avatar(person: &Person) -> impl IntoView + use<> {
    view! {
        <Avatar
            initials=person.initials_in(lang())
            agent=person.is_agent()
            title=person.name_in(lang())
        />
    }
}

#[component]
pub fn OverviewPage() -> impl IntoView {
    let store = use_store();
    let members = Memo::new(move |_| store.ws.with(team::members));
    let stuck = Memo::new(move |_| store.ws.with(team::stuck));

    // The address chooses the person (`#/overview/Ada`); without one, or with „Alle“, everybody
    // is shown. On a narrow screen a choice also opens the detail level.
    let level = Signal::derive(move || store.route.with(Route::level));
    let chosen = Memo::new(move |_| match store.route.get() {
        Route::OverviewMember(key) => Some(key),
        _ => None,
    });
    // `None`: everybody (also if the chosen person has nothing open any more).
    let current = Memo::new(move |_| {
        let name = chosen.get();
        members.with(|list| team::chosen(list, name.as_deref()).cloned())
    });
    let stuck_shown = Memo::new(move |_| team::stuck_of(stuck.get(), current.get().as_ref()));

    view! {
        <SplitView level=level back_label=t!(OverviewTitle) back_href=Route::Overview.hash()>

            <Pane label=t!(OverviewTeam)>
                <PaneHeader>
                    <h1 class="t-large-title t-bold">{t!(OverviewTitle)}</h1>
                    <p class="t-subheadline t-secondary">
                        {t!(OverviewSubtitle)}
                    </p>
                    <TeamPicker />
                    {source_picker(store)}
                </PaneHeader>
                <ul class="ck-inbox">
                    {move || {
                        let active = current.get().map(|m| m.key);
                        let all = store.ws.with(|w| members.with(|l| team::entry_ids(w, l, None))).len();
                        let list = members.get();
                        let sections = store.ws.with(|w| {
                            team::sections(w, &list)
                                .into_iter()
                                .map(|(group, people)| {
                                    (group, people.into_iter().cloned().collect::<Vec<_>>())
                                })
                                .collect::<Vec<_>>()
                        });
                        let rows = sections
                            .into_iter()
                            .map(|(group, people)| {
                                let heading = group.map(group_label);
                                let rows = people
                                    .into_iter()
                                    .map(|m| {
                                        let selected = active.as_deref() == Some(m.key.as_str());
                                        member_row(m, selected)
                                    })
                                    .collect_view();
                                view! {
                                    {heading
                                        .map(|h| {
                                            view! { <li class="ui-list__header t-footnote t-secondary">{h}</li> }
                                        })}
                                    {rows}
                                }
                            })
                            .collect_view();
                        view! { {everyone_row(all, active.is_none())} {rows} }
                    }}
                </ul>
            </Pane>

            <Pane label=t!(OverviewDetails)>
                <PaneHeader>
                    <div class="ui-hstack" style="justify-content: space-between; flex-wrap: nowrap">
                        <h2 class="t-title1 t-bold">{t!(OverviewStuck)}</h2>
                        <Badge variant=BadgeVariant::Count>{move || stuck_shown.get().len().to_string()}</Badge>
                    </div>
                </PaneHeader>
                <PaneBody>
                    {move || {
                        let list = stuck_shown.get();
                        if list.is_empty() {
                            empty_state().into_any()
                        } else {
                            view! {
                                <ul class="ck-stuck-list">
                                    {list.into_iter().map(|s| stuck_row(store, s)).collect_view()}
                                </ul>
                            }
                                .into_any()
                        }
                    }}
                    {move || person_items(store, current.get(), members.get())}
                </PaneBody>
            </Pane>
        </SplitView>
    }
}

/// A person in the left list: numbers by status and the workload.
fn member_row(m: Member, selected: bool) -> impl IntoView {
    let key = m.key.clone();
    let total = m.total();
    let level = m.load_level();
    let summary = t!(
        OverviewCapacity,
        name = m.person.name_in(lang()),
        used = total.min(team::CAPACITY),
        total = team::CAPACITY
    );
    view! {
        <InboxRow selected=selected on_select=move |_| {
            navigate(&Route::OverviewMember(key.clone()));
        }>
            {person_avatar(&m.person)}
            <span class="ui-vstack" style="--spacing: 6px; flex: 1 1 auto; min-width: 0">
                <span class="ui-hstack" style="--spacing: 8px; flex-wrap: nowrap">
                    <span class="t-headline ck-ellipsis" style="flex: 1 1 auto">{m.person.name_in(lang())}</span>
                    <Badge variant=BadgeVariant::Neutral>{total.to_string()}</Badge>
                </span>
                {m
                    .github
                    .clone()
                    .map(|name| {
                        view! { <span class="t-footnote t-secondary ck-ellipsis">{t!(OverviewGithubName, name = name)}</span> }
                    })}
                <span class="t-subheadline t-secondary">{counts(&m)}</span>
                <Progress value=total.min(team::CAPACITY) max=team::CAPACITY label=summary />
                <span class="t-footnote t-secondary">{load_label(level)}</span>
            </span>
        </InboxRow>
    }
}

/// The first row: everybody. Chosen when no person is.
fn everyone_row(total: usize, selected: bool) -> impl IntoView {
    view! {
        <InboxRow selected=selected on_select=move |_| navigate(&Route::OverviewAll)>
            <span class="ui-vstack" style="--spacing: 6px; flex: 1 1 auto; min-width: 0">
                <span class="ui-hstack" style="--spacing: 8px; flex-wrap: nowrap">
                    <span class="t-headline ck-ellipsis" style="flex: 1 1 auto">{t!(OverviewEveryone)}</span>
                    <Badge variant=BadgeVariant::Neutral>{total.to_string()}</Badge>
                </span>
                <span class="t-subheadline t-secondary">{t!(OverviewEveryoneSummary)}</span>
            </span>
        </InboxRow>
    }
}

/// No entry is stuck: that is good news and should look like it.
fn empty_state() -> impl IntoView {
    view! {
        <EmptyState
            title=t!(OverviewNoneStuckTitle)
            text=t!(OverviewNoneStuckText)
            icon=icons::CHECK
        />
    }
}

/// An entry that is stuck: reason, who is on it, and the way there.
fn stuck_row(store: Store, s: Stuck) -> impl IntoView {
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
            <div class="ui-vstack" style="--spacing: 6px; min-width: 0">
                <div class="ui-hstack" style="--spacing: 6px">
                    <Badge variant=BadgeVariant::Neutral>{kind_label(s.kind)}</Badge>
                    <span class="t-footnote t-mono t-secondary">{s.key}</span>
                </div>
                <p class="t-headline">{s.title}</p>
                <p class="t-subheadline t-secondary">{move || s.reason.render(lang())}</p>
                <div class="ui-hstack" style="--spacing: 8px">
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
                    <span class="ui-hstack t-footnote t-secondary" style="--spacing: 6px">
                        {t!(OverviewWaitingOn)} {person_avatar(&s.who)}
                        <span>{s.who.name_in(lang())}</span>
                    </span>
                </div>
            </div>
        </li>
    }
}

/// The entries of the chosen person.
fn person_items(store: Store, member: Option<Member>, all: Vec<Member>) -> impl IntoView {
    let items: Vec<Item> = store.ws.with(|w| {
        team::entry_ids(w, &all, member.as_ref())
            .iter()
            .filter_map(|id| w.item(id).cloned())
            .collect()
    });
    let (heading, nothing) = match &member {
        Some(m) => (
            t!(OverviewEntriesOf, name = m.person.name_in(lang())),
            t!(OverviewNothingOpenFor, name = m.person.name_in(lang())),
        ),
        None => (t!(OverviewEntriesAll), t!(OverviewNothingOpen)),
    };
    let link = member.clone().map(|m| link_section(store, m));
    let body = if items.is_empty() {
        view! { <p class="t-subheadline t-secondary">{nothing}</p> }.into_any()
    } else {
        view! {
            <ul class="ck-stuck-list">
                {items
                    .into_iter()
                    .map(|item| {
                        let id = item.id.clone();
                        let tone = team::state_tone(&item);
                        let state = state_text(&item);
                        view! {
                            <li class="ck-stuck">
                                <div class="ui-vstack" style="--spacing: 6px; min-width: 0; flex: 1 1 auto">
                                    <div class="ui-hstack" style="--spacing: 6px">
                                        <Badge variant=BadgeVariant::Neutral>{item.source.label()}</Badge>
                                        <span class="t-footnote t-mono t-secondary">{item.key.clone()}</span>
                                    </div>
                                    <p class="t-headline">{item.title.clone()}</p>
                                    <div class="ui-hstack" style="--spacing: 8px">
                                        <Status tone=view::tone(tone)>{state}</Status>
                                        <span class="ui-spacer"></span>
                                        <Button size=ControlSize::Small on_click=move |_| store.open_item(&id)>
                                            {t!(OverviewToEntry)}
                                        </Button>
                                    </div>
                                </div>
                            </li>
                        }
                    })
                    .collect_view()}
            </ul>
        }
        .into_any()
    };
    view! {
        <section class="ui-vstack" style="--spacing: 12px" aria-label=heading>
            <h3 class="ui-list__header">{heading.clone()}</h3>
            {link}
            {body}
        </section>
    }
}

/// „Ist dieselbe Person wie …“: link a Jira person to a GitHub user (or the other way round), or
/// remove the link. Nothing is guessed; the link is `Action::LinkPeople`.
fn link_section(store: Store, m: Member) -> impl IntoView {
    if m.is_linked() {
        let jira = m.person.name.clone();
        let github = m.github.clone().unwrap_or_default();
        return view! {
            <div class="ui-vstack" style="--spacing: 8px">
                <p class="t-subheadline t-secondary">{t!(OverviewLinkedWith, name = github)}</p>
                <div>
                    <Button
                        size=ControlSize::Small
                        on_click=move |_| {
                            let _ = store.dispatch(Action::UnlinkPeople { jira: jira.clone() });
                            navigate(&Route::OverviewAll);
                        }
                    >
                        {t!(OverviewUnlink)}
                    </Button>
                </div>
            </div>
        }
        .into_any();
    }
    let Some(side) = m.unlinked_side() else {
        return ().into_any();
    };
    let me = m.person.name.clone();
    let query = RwSignal::new(String::new());
    let found = Memo::new(move |_| {
        let local = store.ws.with(|w| w.link_candidates(side));
        people_search::search(&local, &[], &query.get())
    });
    let options = Signal::derive(move || {
        found
            .get()
            .into_iter()
            .map(|p| ComboOption::new(p.name.clone(), p.name).initials(p.initials))
            .collect::<Vec<_>>()
    });
    let on_select = move |other: String| {
        let (jira, github) = match side {
            Side::Jira => (me.clone(), other.clone()),
            Side::GitHub => (other.clone(), me.clone()),
        };
        let jira_name = jira.clone();
        match store.dispatch(Action::LinkPeople { jira, github }) {
            Ok(_) => {
                store.notify(t!(OverviewLinkDone, name = other));
                // The person is now the linked member: stay on them.
                navigate(&Route::OverviewMember(team::linked_key(&jira_name)));
            }
            Err(e) => store.notify(e.msg().render(feature_ui::lang_untracked())),
        }
    };
    let hint = match side {
        Side::Jira => t!(OverviewLinkHintJira),
        Side::GitHub => t!(OverviewLinkHintGithub),
    };
    view! {
        <div class="ui-vstack" style="--spacing: 8px">
            <h4 class="t-headline">{t!(OverviewLinkTitle)}</h4>
            <p class="t-footnote t-secondary">{hint}</p>
            <Combobox
                label=t!(OverviewLinkLabel)
                label_hidden=true
                value=Signal::derive(String::new)
                query=query
                options=options
                on_select=on_select
                placeholder=t!(OverviewLinkPlaceholder)
                empty_text=t!(OverviewLinkEmpty)
            />
        </div>
    }
    .into_any()
}
