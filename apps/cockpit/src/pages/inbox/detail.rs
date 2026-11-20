use super::assignee::AssigneeField;
use super::*;

/// The detail of the chosen entry.
pub(super) fn detail(
    store: Store,
    item: Item,
    items: Memo<Vec<Item>>,
    draft: RwSignal<String>,
    edit: Edit,
) -> impl IntoView {
    let id = StoredValue::new(item.id.clone());
    let is_ticket = item.is_ticket();
    // Tickets from GitHub Projects carry Markdown (body and comments), not Jira formatting.
    let is_project = item.source == cockpit_core::Source::GitHubProject;
    let source = item.source.label();
    let review_href = item
        .pr
        .as_ref()
        .map(|pr| Route::Review(Some(pr.key())).hash());

    // Tickets show their status for switching, pull requests the state of the checks.
    let state = if is_ticket {
        view! {
            <Picker
                options=labels::options(TicketStatus::OPTIONS, TicketStatus::from_id, labels::ticket_status)
                value=item.status.id()
                on_change=move |status: &'static str| {
                    if let Some(status) = TicketStatus::from_id(status) {
                        let _ = store.dispatch(Action::SetStatus {
                            item: id.get_value(),
                            status,
                        });
                    }
                }
                aria_label=t!(InboxDetailStatus)
            />
        }
        .into_any()
    } else {
        item.pr
            .as_ref()
            .map(|pr| {
                let (_, tone) = pr.status();
                let text = labels::pr_status(pr);
                view! { <Status tone=view::tone(tone)>{text}</Status> }
            })
            .into_any()
    };

    let branch = match item.branch.clone() {
        Some(name) => view! { <Chip>{name}</Chip> }.into_any(),
        None => view! {
            <Button
                size=ControlSize::Small
                on_click=move |_| {
                    let _ = store.dispatch(Action::CreateBranch { item: id.get_value() });
                }
            >
                {t!(InboxDetailCreateBranch)}
            </Button>
        }
        .into_any(),
    };

    // The most important action: commission the agent – or, if there is already a pull request, review.
    let primary = match (&review_href, is_ticket) {
        (Some(href), _) => view! {
            <Button variant=ButtonStyle::GlassProminent size=ControlSize::Large href=href.clone()>
                {if is_ticket { t!(InboxDetailCheckPr) } else { t!(InboxDetailOpenReview) }}
            </Button>
        }
        .into_any(),
        (None, true) => view! {
            <Button
                variant=ButtonStyle::GlassProminent
                size=ControlSize::Large
                href=Route::Agent(Some(item.key.clone())).hash()
            >
                <Icon path=icons::SPARKLES />
                {t!(InboxDetailAssignAgent)}
            </Button>
        }
        .into_any(),
        (None, false) => ().into_any(),
    };

    // Edit ticket: title and description, assignee, priority.
    let initial = StoredValue::new((item.title.clone(), item.description.clone()));
    let start = move |_| {
        let (title, description) = initial.get_value();
        edit.title.set(title);
        edit.description.set(description);
        edit.on.set(true);
    };
    let save = move |_| {
        let saved = store.dispatch(Action::EditTicket {
            item: id.get_value(),
            title: edit.title.get_untracked(),
            description: edit.description.get_untracked(),
        });
        // "Nothing changes" is no reason to stay in edit mode.
        if saved.is_ok() || matches!(saved, Err(cockpit_core::ActionError::Unchanged)) {
            edit.on.set(false);
        }
    };
    let cancel = move |_| edit.on.set(false);
    let edit_button = is_ticket.then(|| {
        view! {
            <Button size=ControlSize::Small on_click=start>
                {t!(InboxDetailEdit)}
            </Button>
        }
    });
    let edit_form = is_ticket.then(|| {
        view! {
            <Show when=move || edit.on.get()>
                <div class="ck-edit ui-vstack" style="--spacing: 12px">
                    <TextField
                        label=t!(InboxDetailTitle)
                        value=edit.title
                        hint=t!(InboxDetailTitleHint)
                        error=Signal::derive(move || edit.title.get().trim().is_empty())
                    />
                    <Textarea value=edit.description label=t!(InboxDetailDescription) />
                    <div class="ui-hstack" style="--spacing: 8px; justify-content: flex-end">
                        <Button on_click=cancel>{t!(InboxDetailCancel)}</Button>
                        <Button
                            variant=ButtonStyle::BorderedProminent
                            disabled=Signal::derive(move || edit.title.get().trim().is_empty())
                            on_click=save
                        >
                            {t!(InboxDetailSave)}
                        </Button>
                    </div>
                </div>
            </Show>
        }
    });

    let ticket_key = item.key.clone();
    let ticket_props = is_ticket.then(|| {
        view! {
            <ListSection header=t!(InboxDetailTicket) nested=true>
                <li>
                    <div class="ui-list__row" style="flex-wrap: wrap">
                        <span class="ui-list__title">{t!(InboxDetailPriority)}</span>
                        <Picker
                            options=labels::options(Priority::OPTIONS, Priority::from_id, labels::priority)
                            value=item.priority.id()
                            on_change=move |p: &'static str| {
                                if let Some(priority) = Priority::from_id(p) {
                                    let _ = store.dispatch(Action::SetPriority {
                                        item: id.get_value(),
                                        priority,
                                    });
                                }
                            }
                            aria_label=t!(InboxDetailPriority)
                        />
                    </div>
                </li>
                <li>
                    <div class="ui-list__row" style="flex-wrap: wrap">
                        <span class="ui-list__title">{t!(InboxDetailAssignedTo)}</span>
                        <AssigneeField store=store item=id.get_value() key=ticket_key />
                    </div>
                </li>
            </ListSection>
        }
    });

    let linked_ticket = (!is_ticket)
        .then(|| {
            let number = item.pr.as_ref()?.number;
            store.ws.with_untracked(|w| {
                w.ticket_of_pr(number)
                    .map(|t| (t.id.clone(), t.key.clone(), t.title.clone()))
            })
        })
        .flatten()
        .map(|(ticket_id, key, title)| {
            view! {
                <li>
                    <div class="ui-list__row" style="flex-wrap: wrap">
                        {row_icon(icons::GIT_PR)} <span class="ui-list__title">{t!(InboxDetailTicket)}</span>
                        <span class="ui-list__detail ck-ellipsis">{t!(InboxDetailKeyTitle, key = key, title = title)}</span>
                        <Button
                            size=ControlSize::Small
                            on_click=move |_| store.open_item(&ticket_id)
                        >
                            {t!(InboxDetailOpenTicket, key = key)}
                        </Button>
                    </div>
                </li>
            }
        });

    // The pull request's number links to GitHub – to the address the sync read from the pull
    // request itself (it also fits other GitHub hosts), otherwise to github.com.
    let pr_link = move |pr: &cockpit_core::PrRef| -> AnyView {
        let label = labels::pr_label(pr);
        let url = store.ws.with_untracked(|w| {
            w.items
                .iter()
                .find(|i| {
                    i.kind == cockpit_core::Kind::PullRequest
                        && i.pr.as_ref().is_some_and(|p| p.key().same(&pr.key()))
                })
                .and_then(Item::web_url)
        });
        match url.or_else(|| pr.github_url()) {
            Some(url) => super::details::external_link(
                url,
                t!(
                    InboxDetailOpenExternal,
                    key = pr.key().to_string(),
                    source = "GitHub"
                ),
                "ck-key-link",
                label.into_any(),
            ),
            None => label.into_any(),
        }
    };
    let pr_text = match item.pr.as_ref() {
        None => t!(InboxDetailPrNone).into_any(),
        Some(pr) => pr_link(pr),
    };

    // Tickets list every automatically linked pull request, each with its branch and checks.
    let dev_prs = if is_ticket {
        store.ws.with_untracked(|w| w.linked_prs(&item.id))
    } else {
        Vec::new()
    };
    let has_dev = !dev_prs.is_empty();
    let dev_rows = dev_prs
        .into_iter()
        .map(|linked| {
            let (_, tone) = linked.pr.status();
            let status = labels::pr_status(&linked.pr);
            let link = pr_link(&linked.pr);
            let key = linked.pr.key().to_string();
            let branch = (!linked.branch.is_empty()).then(|| view! { <Chip>{linked.branch.clone()}</Chip> });
            view! {
                <li>
                    <div class="ui-list__row" style="flex-wrap: wrap" data-linked-pr=key>
                        {row_icon(icons::GIT_PR)} <span class="ui-list__title">{t!(InboxDetailPullRequest)}</span>
                        <span class="ui-list__detail">{link}</span>
                        {branch}
                        <Status tone=view::tone(tone)>{status}</Status>
                    </div>
                </li>
            }
        })
        .collect_view();
    let comments = item
        .comments
        .iter()
        .map(|c| {
            view! {
                <CommentBubble
                    who=c.author.name.clone()
                    initials=c.author.initials.clone()
                    time=c.time.clone()
                    text=c.text_msg.as_ref().map_or_else(|| c.text.clone(), |m| m.render(lang()))
                    rich=c.rich.clone().unwrap_or_default()
                    own=c.own
                    markdown=is_project
                />
            }
        })
        .collect_view();

    // Snooze: four options; if the entry is already snoozed, it shows for how long.
    let snoozed = store.ws.with_untracked(|w| w.snooze_of(&item.id).cloned());
    let snooze_row = match snoozed {
        Some(snooze) => view! {
            <div class="ck-snooze ui-hstack" style="--spacing: 10px; justify-content: space-between">
                <span class="t-subheadline t-secondary">{snooze_text(snooze.until_ms)}</span>
                <Button
                    size=ControlSize::Small
                    on_click=move |_| {
                        let _ = store.dispatch(Action::Unsnooze { item: id.get_value() });
                    }
                >
                    {t!(InboxDetailUnsnooze)}
                </Button>
            </div>
        }
        .into_any(),
        None => view! {
            <div class="ck-snooze ui-hstack" style="--spacing: 8px" role="group" aria-label=t!(InboxDetailSnoozeGroup)>
                <span class="t-subheadline t-secondary">{t!(InboxDetailSnoozeLabel)}</span>
                {SnoozeChoice::ALL
                    .into_iter()
                    .map(|choice| {
                        let label = labels::snooze_choice(choice);
                        view! {
                            <Button
                                size=ControlSize::Small
                                on_click=move |_| snooze(store, items, &id.get_value(), choice)
                            >
                                {label}
                            </Button>
                        }
                    })
                    .collect_view()}
            </div>
        }
        .into_any(),
    };

    let send = move |_| {
        let text = draft.get_untracked();
        let sent = store.dispatch(Action::Reply {
            item: id.get_value(),
            text,
        });
        if sent.is_ok() {
            draft.set(String::new());
        }
    };

    let project_part = project::project_section(store, &item);
    let key_link = super::details::key_link(&item);
    let detail_sections = super::details::details_sections(item.details.as_ref());
    view! {
        <PaneHeader>
            <div class="ui-hstack" style="--spacing: 10px">
                <Badge variant=BadgeVariant::Neutral>{source}</Badge>
                {key_link}
                <span class="ui-spacer"></span>
                <span class="ui-hstack" style="--spacing: 8px; flex-wrap: nowrap">
                    <Avatar initials=item.owner.initials.clone() />
                    <span class="t-subheadline">{item.owner.name.clone()}</span>
                </span>
            </div>
            <div class="ui-hstack" style="justify-content: space-between; align-items: flex-start">
                <h2 class="t-title1 t-bold">{item.title.clone()}</h2>
                {edit_button}
            </div>
            {state}
        </PaneHeader>

        <PaneBody>
            {edit_form}
            <div hidden=move || edit.on.get() && is_ticket>
                {match item.description_rich.clone() {
                    Some(doc) => view! { <RichText doc=doc /> }.into_any(),
                    None if is_project => {
                        view! { <Markdown text=item.description.clone() /> }.into_any()
                    }
                    None => view! { <p class="t-body">{item.description.clone()}</p> }.into_any(),
                }}
            </div>

            {detail_sections}

            <List>{ticket_props}</List>

            {project_part}

            <List>
                <ListSection header=t!(InboxDetailDevelopment) nested=true>
                    <li>
                        <div class="ui-list__row" style="flex-wrap: wrap">
                            {row_icon(icons::GIT_BRANCH)} <span class="ui-list__title">{t!(InboxDetailBranch)}</span> {branch}
                        </div>
                    </li>
                    {dev_rows}
                    {(!has_dev)
                        .then(|| {
                            view! {
                                <li>
                                    <div class="ui-list__row" style="flex-wrap: wrap">
                                        {row_icon(icons::GIT_PR)} <span class="ui-list__title">{t!(InboxDetailPullRequest)}</span>
                                        <span class="ui-list__detail">{pr_text}</span>
                                        {(!is_ticket)
                                            .then(|| review_href.clone())
                                            .flatten()
                                            .map(|href| {
                                                view! { <Button size=ControlSize::Small href=href>{t!(InboxDetailOpenReview)}</Button> }
                                            })}
                                    </div>
                                </li>
                            }
                        })}
                    {linked_ticket}
                    {item
                        .agent_note_msg
                        .as_ref()
                        .map(|m| m.render(lang()))
                        .or_else(|| item.agent_note.clone())
                        .map(|note| {
                            view! {
                                <li>
                                    <div class="ui-list__row">
                                        <Avatar agent=true />
                                        <span class="ui-list__title">{note}</span>
                                    </div>
                                </li>
                            }
                        })}
                </ListSection>
            </List>

            <div class="ui-hstack" style="--spacing: 10px">
                {primary}
                <Button
                    size=ControlSize::Large
                    disabled=item.read
                    on_click=move |_| {
                        let _ = store.dispatch(Action::MarkDone { item: id.get_value() });
                    }
                >
                    {if item.read { t!(InboxDetailDone) } else { t!(InboxDetailMarkDone) }}
                </Button>
            </div>

            {snooze_row}

            <div class="ui-list__section">
                <p class="ui-list__header">{t!(InboxDetailComments)}</p>
                <div class="ui-vstack" style="--spacing: 12px">
                    {comments}
                    <div class="ui-vstack" style="--spacing: 8px">
                        <div class="ck-reply">
                            <Textarea
                                value=draft
                                label=t!(InboxDetailReplyLabel)
                                placeholder=if is_ticket { t!(InboxDetailReplyTicket) } else { t!(InboxDetailReplyPr) }
                            />
                        </div>
                        <div class="ui-hstack" style="justify-content: space-between">
                            <span class="t-footnote t-secondary">
                                {t!(InboxDetailPostedIn, source = source)}
                            </span>
                            <Button
                                variant=ButtonStyle::BorderedProminent
                                disabled=Signal::derive(move || draft.get().trim().is_empty())
                                on_click=send
                            >
                                {t!(InboxDetailReply)}
                            </Button>
                        </div>
                    </div>
                </div>
            </div>
        </PaneBody>
    }
}
