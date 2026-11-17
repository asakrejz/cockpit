# Using the Cockpit app

Pages of the app (hash routes). Back to the [README](../README.md).

Pages (hash routes):

| Page | Address | What it does |
|------|---------|--------------|
| Einrichten | `#/setup` | state and next step per service: sign in to / disconnect Jira and GitHub, check the agent endpoint, detect the container runtime |
| Aufmerksamkeit | `#/`, `#/mine`, `#/team`, `#/snoozed` | inbox of tickets and pull requests, filtered by switchable rules (mention, review request, blocked PR, overdue ticket, agent done, assigned); snooze; change status, create a branch, reply, assign the agent; edit a ticket; link ticket and PR; search, source filter and project/repo/person/status/label filters; keys `↑↓`/`j k`, `e`, `r`, `s`, `u`, `/` |
| Review | `#/review/87` | read the diff; select lines and write your own comments; approve, reject or edit comments of the agent; send the review; checks with log; merge (only you) |
| Übersicht | `#/overview` | open entries and load per person; "Hängt" shows failed checks, waiting reviews, tickets without a branch and agents waiting for you |
| Benachrichtigungen | `#/notifications` | per occasion choose browser, email and Slack; quiet hours; permission request by button; test notification |
| Agent | `#/agent/BAU-142` | assignment with options; plan for approval; five-step flow; answer questions, send hints, pause; history as a conversation; for a finished real run "Pull Request anlegen" (preview, editable) |
| Grenzen & Regeln | `#/limits` | prices per million tokens, budget per run and month, time limit, parallel runs; per repo allowed/forbidden paths, required tests, instructions and plan requirement; usage today and this month |

Inbox filters: besides the Jira project the header filters by **repo**, **person**, **status** and **label**; each
selection defaults to "Alle", filters combine with AND, "Filter zurücksetzen" resets them. The choice belongs to the
page only and is not persisted. The logic is `cockpit_core::InboxFilter` (`crates/cockpit-core/src/search.rs`).

Notifications are derived purely from comparing the workspace before and after an action
(`crates/cockpit-core/src/notify.rs`). In the **browser** Cockpit never asks for permission by itself, only via the
button. Email and Slack are only prepared: the choice is stored, sending will go through the backend later.

More about agent behaviour (questions, steering, follow-up jobs, costs, limits, rules per repo) is in
[docs/architecture.md](architecture.md#agent-behaviour).
