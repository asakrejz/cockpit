# Using the Cockpit app

What the pages of the app do and how to work with them. Setting up the services is in the [setup guide](setup-guide.md).

**Navigation:** a floating glass pill at the bottom edge on every screen size: Aufgaben, Reviews, Agenten, Übersicht and
**Mehr**. "Aufgaben" is the whole inbox; its filters (Alle, Ich, Team, Zurückgestellt) are switched on the page. It opens
with the filter chosen last (saved per browser; "Ich" at first). "Mehr" opens a menu with the other pages (Protokoll, Grenzen & Regeln, Benachrichtigungen, Einrichten), the connection states of Jira, GitHub and the agent, and on the inbox the rules
(which events count as attention). The lists of pull requests and agent runs are the first column of their pages.
Every column scrolls on its own. Details: [Layout and navigation](architecture.md#layout-and-navigation).

**Language:** the app speaks German and English. It starts in the language of the browser (German for `de*`, otherwise
English). You change it in the "More" menu ("Language") or in the first step of the setup assistant; the change applies at
once and is saved in the workspace. The agents write their plans, summaries and review comments in the same language. The language of pull request titles and texts is a separate setting per repo (setup assistant, default English). Texts from Jira and GitHub stay as they are. The page names below are the German ones;
in English they read Tasks, Pull requests, Agents, Log, Limits & rules, Notifications, Set up.

## Pages

| Page | Address | What it does |
|------|---------|--------------|
| Einrichten | `#/setup`, `#/setup/<step>` | state and next step per service: sign in to / disconnect Jira and GitHub, check the agent endpoint, detect the container runtime |
| Aufgaben | `#/tasks` (the filter chosen last), `#/`, `#/mine`, `#/team`, `#/snoozed` (the older addresses keep their filter) | inbox of tickets and pull requests, filtered by switchable rules (mention, review request, blocked PR, overdue ticket, agent done, assigned); snooze; change status, create a branch, reply, assign the agent; edit a ticket; pull requests link to their ticket automatically (the ticket key in the branch name or title; there is no manual linking), the section "Entwicklung" lists every linked PR with branch, state and checks; search; the button "Filter" (with the number of active filters) opens the filter dialog with all filters, chips under it remove single filters; keys `↑↓`/`j k`, `e`, `r`, `s`, `u`, `/` |
| Review | `#/review/acme/app/87` | the description and all comments are shown as Markdown (headings, lists, code, links; no raw HTML); tabs „Dateien“ (file tree with +/− counts, one collapsible diff per file, „Angesehen“ per file, long diffs cut at 300 lines with „Mehr anzeigen“) and „Commits“ (list; a click shows only that commit's diff, read only); select lines and write your own comments; approve, reject or edit comments of the agent; send the review; checks with log; merge (only you) |
| Übersicht | `#/overview` | open entries and load per person; "Hängt" shows failed checks, waiting reviews, tickets without a branch and agents waiting for you |
| Benachrichtigungen | `#/notifications` | per occasion choose browser, email and Slack; quiet hours; permission request by button; test notification |
| Agent | `#/agent/BAU-142` | assignment with options; plan for approval; five-step flow; answer questions, send hints, pause; history as a conversation; for a finished real run "Pull Request anlegen" (preview, editable); finished runs whose pull request is merged or closed (see below); "Aufräumen" |
| Grenzen & Regeln | `#/limits` | prices per million tokens, budget per run and month, time limit, parallel runs; per repo allowed/forbidden paths, required tests, instructions and plan requirement; usage today and this month |

## Inbox filters

The button "Filter" at the top of the inbox shows the number of active filters as a badge and opens the
**filter dialog** (a centered card on wide screens, a sheet from the bottom on narrow ones). The dialog holds the Jira
project, every filter below, and the attention rules ("Erwähnung", "Review-Anfrage", … and "Status folgt dem Pull
Request"; these switches act at once). The filters are defined once as data (`cockpit_core::FILTERS`: ID, label,
choices, default) and applied by `cockpit_core::InboxFilter` (`crates/cockpit-core/src/search.rs`); the dialog renders
that list. Filters combine with AND. "Anwenden" applies the edits together (action `SetInboxFilter`), "Zurücksetzen"
puts the edits back to the start state. The applied filters are part of the workspace (stored on the server like the
rules and the Jira project, so they survive a reload). Every active filter is also a chip under the button; its "✕" removes
only that filter. The left column shows entries only, no switches.

## Mein Team

On the Übersicht page the button "Mein Team" (under the title) opens a dialog with the Jira teams seen in the
synced tickets (the "Team" field, see [configuration](configuration.md)). Choose one or more teams and "Übernehmen" (action
`SetTeamChoice`, stored in the workspace like the filters); "Ohne Team einbeziehen" (off by default) also shows tickets
without a team. With no team chosen everything shows, and a hint says you can choose your team. With a choice, the
Übersicht (people, load, "Hängt", lists) and the inbox filters "Alle" and "Team" show only tickets of those teams; people
appear only if they have such entries, "Du" and "Agent" always stay. What is yours (filter "Ich", entries assigned to you)
is never hidden. A pull request is not filtered unless it is linked to a ticket (ticket key in branch or title) and none of its
tickets is shown. The rules live in `cockpit_core::team_filter`. Display only: nothing is written to Jira.

"Zugewiesen an" (`person`) is a drop-down with a search field: "Alle", "Mir", "Nicht zugewiesen" and the people of the
tickets present. It is a display filter only; nothing is written to Jira.

**Assignee of a ticket.** In the ticket detail the assignee is a search field with suggestions: people already seen in
the workspace come first, then Jira's own search for people the ticket can be assigned to (`GET /api/jira/assignable`,
read only). Choosing one sets the assignee in Cockpit; it is not sent to Jira yet.

| ID | Label | Choices (default first) |
|----|-------|-------------------------|
| `source` | Quelle | Alle · Jira · GitHub |
| `ownership` | Besitz | Meine (author, requested reviewer or assignee; the attention list) · Alle (also colleagues' pull requests and tickets, without attention) |
| `sprint` | Sprint (tickets only) | Aktueller Sprint (`sprint.state == active`) · Alle offenen · Zuletzt aktualisiert (14 days) · Backlog (no sprint) |
| `project` | Ticket-Projekt | Alle · each Jira project (a pull request counts to the project of the ticket key in its branch name or title) · Ohne Ticket (pull requests without a key) |
| `repo`, `status`, `label` | Repo, Status, Label | Alle … |
| `person` | Zugewiesen an | Alle · Mir · Nicht zugewiesen · each person of the tickets (a Jira ticket without assignee has no owner: "Nicht zugewiesen") |
| `done` | Erledigte (tickets only) | ausblenden · zeigen |

"Aktueller Sprint" behaves like "Alle offenen" as long as no ticket carries sprint data (a project without sprints; the
source state then says so). Pull requests of colleagues never raise attention or notifications: only pull requests
with a relation to you (author, requested reviewer, assignee) can. They appear in the inbox only with "Alle", and on the
Pull Requests page (`#/review`) the "Besitz" selection (default "Meine") switches between your pull requests and all
open pull requests of the watched repos (`[github] repos`; the others show the ones you are involved in).

## Notifications

Notifications are derived purely from comparing the workspace before and after an action
(`crates/cockpit-core/src/notify.rs`). In the **browser** Cockpit never asks for permission by itself, only via the
button. Email and Slack are only prepared: the choice is stored, sending will go through the backend later.

## Jira project and sprint view

If the Jira site has several projects, pick one in the filter dialog ("Jira-Projekt", the first field). The list comes
from Jira (`GET /api/jira/projects`, behind it `GET /rest/api/3/project/search`) plus the projects of the tickets already
present. With a project selected the inbox shows only its tickets (pull requests stay visible), and the choice takes
precedence over `projects` and `jql` from `config.toml`. "Alle Projekte" restores those.

What the sync fetches follows the choice (read queries only) and always covers all tickets of other people too, not only
yours (what is neither assigned to nor watched by you appears under "Team" with "Im Projekt"):

| Project | Sprint in the dialog | Query |
|---------|----------------------|-------|
| none | Aktueller Sprint (default) | `sprint in openSprints()` (restricted to `projects` from `config.toml`), or your own `jql` |
| `BAU` | Aktueller Sprint | `project = BAU AND sprint in openSprints()` |
| `BAU` or none | Alle offenen | `project in (BAU) AND statusCategory != Done` |
| `BAU` or none | Backlog | … `AND sprint is EMPTY` |
| `BAU` or none | Zuletzt aktualisiert | … `AND updated >= -14d` |

Without a chosen project a sprint view other than "Aktueller Sprint" asks for the projects from `config.toml` (all
projects if there are none); an own `jql` is only replaced when a project is chosen. The sprint view is saved with the
other filters (action `SetInboxFilter`); after a change the server syncs immediately.

## What a run leaves behind

Containers are started with `--rm`, and the clone under `<data>/agent/runs/<run>` is deleted when a run ends (it only
stays while a run is halted by a limit or waits for you). What can remain is the branch on GitHub, the run in the list
and, after a pause or a cancel, a clone.

- **„Erledigt“.** The sync (every interval, and after each pass) asks GitHub about the pull request of every quiet run
  (finished, ready, paused or halted – never one that works). If it is merged the run gets the state **„Erledigt
  (gemergt)“**, if it is closed without merging **„Erledigt (geschlossen)“**. A paused or halted run is ended first,
  and its clone is deleted. The core does this as the action `CloseAgentRun` (system only, never the interface); old
  data without the field load unchanged. Starting the ticket again clears the state.
- **Hidden by default.** The list of runs hides done runs. The switch **„Erledigte anzeigen (n)“** above the list
  (only shown when there are any) brings them back with their number; the run you have open stays visible.
- **„Branch löschen“.** On a done run that the Cockpit implemented, the pane shows the branch and the button. Pressing
  it opens a question that names the branch; only on confirmation the backend deletes exactly that branch on GitHub.
  The button is off, with the reason next to it, unless **all** hold: the pull request is merged or closed, the branch
  fits the branch pattern of the repo, it is not the base branch, the default branch of the repo or a protected
  branch, it is the head of that pull request and the branch the run pushed. The backend reads this from GitHub again
  at the click. Nothing deletes a branch by itself.
- **„Aufräumen“.** The button under the list of runs first shows a preview: containers with the label `cockpit.run`
  that belong to no working run, and folders under `<data>/agent/runs` that no active run uses. Only after „Jetzt
  entfernen“ they are removed (containers with `docker`/`podman rm -f`, never via the socket). The image and containers
  without the label are never touched. If the container program is not reachable, only folders are listed. The
  removal re-checks every item and skips whatever started working in the meantime.

## Choosing the repo at the start

The start dialog of a ticket has a searchable list of every repo your GitHub access can see. The suggestions come first,
each with a short reason: used before for tickets of the same Jira project, has a pull request or branch for this
ticket, named in the ticket, or set in the settings. Type to search; a valid `owner/name` that is not listed can be
used as typed. The first time the agent works in a repo, Cockpit reads its default branch (and a branch pattern proposal
from `CONTRIBUTING.md`) from GitHub and remembers it in `[[agent.repos]]`.

### Working in several repos at once

You can tick **more than one repo**. The first one is the main repo. The agent works in all of them at the same time
and changes only those the ticket needs. Each repo gets its own branch (named by the rules of that repo) and its own
pull request.

On the run page the section "Repos in diesem Lauf" shows every repo with its branch and its state:
*Nichts geändert* (nothing was pushed), *Gepusht*, *Pull Request erstellt* or *Push abgelehnt* (with the reason). If one
repo is refused – for example because of a forbidden path – the other repos are still pushed. You create the pull
request per repo with its own button; the text of each lists the other pull requests of the same run. Deleting the branch
after the merge also works per repo. The pull requests show up in the ticket under "Entwicklung" as usual, because they
all carry the ticket key.

More about agent behaviour (questions, steering, follow-up jobs, costs, limits, rules per repo) is in
[architecture.md](architecture.md#agent-behaviour).
