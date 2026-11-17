# Using the Cockpit app

Pages of the app (hash routes). Back to the [README](../README.md).

Pages (hash routes):

| Page | Address | What it does |
|------|---------|--------------|
| Einrichten | `#/setup`, `#/setup/<step>` | state and next step per service: sign in to / disconnect Jira and GitHub, check the agent endpoint, detect the container runtime |
| Aufmerksamkeit | `#/`, `#/mine`, `#/team`, `#/snoozed` | inbox of tickets and pull requests, filtered by switchable rules (mention, review request, blocked PR, overdue ticket, agent done, assigned); snooze; change status, create a branch, reply, assign the agent; edit a ticket; link ticket and PR; search, source filter and project/repo/person/status/label filters; keys `↑↓`/`j k`, `e`, `r`, `s`, `u`, `/` |
| Review | `#/review/87` | read the diff; select lines and write your own comments; approve, reject or edit comments of the agent; send the review; checks with log; merge (only you) |
| Übersicht | `#/overview` | open entries and load per person; "Hängt" shows failed checks, waiting reviews, tickets without a branch and agents waiting for you |
| Benachrichtigungen | `#/notifications` | per occasion choose browser, email and Slack; quiet hours; permission request by button; test notification |
| Agent | `#/agent/BAU-142` | assignment with options; plan for approval; five-step flow; answer questions, send hints, pause; history as a conversation; for a finished real run "Pull Request anlegen" (preview, editable) |
| Grenzen & Regeln | `#/limits` | prices per million tokens, budget per run and month, time limit, parallel runs; per repo allowed/forbidden paths, required tests, instructions and plan requirement; usage today and this month |

Inbox filters: besides the Jira project the header holds the filters below. They are defined once as data
(`cockpit_core::FILTERS`: ID, label, choices, default) and applied by `cockpit_core::InboxFilter`
(`crates/cockpit-core/src/search.rs`); the bar renders that list, so a filter dialog can render the same one.
Filters combine with AND, "Filter zurücksetzen" returns to the start state. The choice belongs to the page only and is
not persisted.

| ID | Label | Choices (default first) |
|----|-------|-------------------------|
| `ownership` | Besitz | Meine (author, requested reviewer or assignee; the attention list) · Alle (also colleagues' pull requests and tickets, without attention) |
| `sprint` | Sprint (tickets only) | Aktueller Sprint (`sprint.state == active`) · Alle offenen · Zuletzt aktualisiert (14 days) · Backlog (no sprint) |
| `project` | Ticket-Projekt | Alle · each Jira project (a pull request counts to the project of the ticket key in its branch name or title) · Ohne Ticket (pull requests without a key) |
| `repo`, `person`, `status`, `label` | as before | Alle … |
| `done` | Erledigte (tickets only) | ausblenden · zeigen |

"Aktueller Sprint" behaves like "Alle offenen" as long as no ticket carries sprint data (a project without sprints; the
source state then says so). Pull requests of colleagues never raise attention or notifications: only pull requests
with a relation to you (author, requested reviewer, assignee) can. They appear in the inbox only with "Alle", and on the
Pull Requests page (`#/review`) the "Besitz" selection (default "Meine") switches between your pull requests and all
open pull requests of the configured repos.

Notifications are derived purely from comparing the workspace before and after an action
(`crates/cockpit-core/src/notify.rs`). In the **browser** Cockpit never asks for permission by itself, only via the
button. Email and Slack are only prepared: the choice is stored, sending will go through the backend later.

More about agent behaviour (questions, steering, follow-up jobs, costs, limits, rules per repo) is in
[docs/architecture.md](architecture.md#agent-behaviour).
