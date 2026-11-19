//! The assignee of a ticket: a search field with suggestions instead of a list of all names.
//!
//! First the people already known from the synced tickets are searched (local, instant). After
//! a short pause in typing the server is asked who Jira would let you assign tickets of the
//! project to (`GET /api/jira/assignable`, read only); both lists are merged. Choosing someone
//! dispatches `Action::AssignTicket`, exactly as the buttons did before.

use std::time::Duration;

use super::*;
use cockpit_core::{UNASSIGNED_NAME, is_project_key, people_search};
use leptos::task::spawn_local;

/// Pause in typing before Jira is asked.
const DEBOUNCE: Duration = Duration::from_millis(250);
const ME: &str = "me";
const NOBODY: &str = "unassigned";

/// The person picker for one ticket (`item` is the id, `key` e.g. `BAU-142`).
#[component]
pub(super) fn AssigneeField(store: Store, item: String, key: String) -> impl IntoView {
    let project = key
        .split('-')
        .next()
        .map(str::to_string)
        .filter(|p| is_project_key(p));
    let id = StoredValue::new(item);
    let query = RwSignal::new(String::new());
    let remote = RwSignal::new(Vec::<Person>::new());
    let busy = RwSignal::new(false);
    // Which question is the current one; answers to older ones are dropped.
    let generation = StoredValue::new(0u32);

    // After a pause in typing: ask Jira (if there is a project and enough text).
    Effect::new(move |_| {
        let text = query.get().trim().to_string();
        let n = generation.get_value().wrapping_add(1);
        generation.set_value(n);
        remote.set(Vec::new());
        busy.set(false);
        let Some(project) = project.clone() else {
            return;
        };
        if text.chars().count() < people_search::MIN_QUERY_CHARS {
            return;
        }
        set_timeout(
            move || {
                if generation.get_value() != n {
                    return;
                }
                busy.set(true);
                spawn_local(async move {
                    let answer = server::fetch_jira_assignable(&project, &text).await;
                    if generation.get_value() != n {
                        return;
                    }
                    busy.set(false);
                    // Without Jira or on an error the local search is all there is.
                    if let Ok(found) = answer {
                        remote.set(found.people);
                    }
                });
            },
            DEBOUNCE,
        );
    });

    // The current assignee as the text of the field.
    let current = Signal::derive(move || {
        store.ws.with(|w| {
            w.item(&id.get_value())
                .map(|i| match &i.owner {
                    p if *p == Person::me() => t!(InboxDetailAssigneeMe),
                    p if p.name == UNASSIGNED_NAME => t!(InboxDetailAssigneeUnassigned),
                    p => p.name.clone(),
                })
                .unwrap_or_default()
        })
    });

    // Found people by name; the ids of the options map back to a person.
    let found = Memo::new(move |_| {
        let text = query.get();
        let local = store.ws.with(|w| w.people());
        people_search::search(&local, &remote.get(), &text)
    });
    let options = Signal::derive(move || {
        let mut list = vec![
            ComboOption::new(ME, t!(InboxDetailAssigneeMe)).initials(Person::me().initials),
            ComboOption::new(NOBODY, t!(InboxDetailAssigneeUnassigned)).initials("–"),
        ];
        let people = found.get();
        if !people.is_empty()
            && let Some(last) = list.last_mut()
        {
            last.divider = true;
        }
        // You are the first quick choice already.
        list.extend(
            people
                .into_iter()
                .filter(|p| *p != Person::me())
                .map(|p| ComboOption::new(format!("p:{}", p.name), p.name).initials(p.initials)),
        );
        list
    });

    let on_select = move |option: String| {
        let person = match option.as_str() {
            ME => Person::me(),
            NOBODY => Person::new(UNASSIGNED_NAME, "–"),
            other => {
                let name = other.strip_prefix("p:").unwrap_or(other);
                match found.with_untracked(|f| f.iter().find(|p| p.name == name).cloned()) {
                    Some(p) => p,
                    None => return,
                }
            }
        };
        let _ = store.dispatch(Action::AssignTicket {
            item: id.get_value(),
            person,
        });
    };

    view! {
        <div class="ck-assignee">
            <Combobox
                label=t!(InboxDetailAssignedTo)
                label_hidden=true
                value=current
                query=query
                options=options
                on_select=on_select
                placeholder=t!(InboxDetailAssigneePlaceholder)
                empty_text=t!(InboxDetailAssigneeEmpty)
                busy=busy
                busy_text=t!(InboxDetailAssigneeSearching)
            />
        </div>
    }
}
