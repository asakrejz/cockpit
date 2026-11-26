# Using the Cockpit app

A guide to the pages of the app. Setting up the services is in the [setup guide](setup-guide.md); the technical background
is in [architecture.md](architecture.md). The UI follows the browser language (German for `de*`, otherwise English); this
page names the English labels, with the German name in parentheses where it helps to find a button.

**Navigation.** A floating glass pill at the bottom edge on every screen size: Tasks (Aufgaben), Reviews, Agents (Agenten),
Overview (Übersicht) and **More** (Mehr). "Tasks" is the whole inbox; its filters (All, Me, Team, Snoozed) are switched on
the page, and it opens with the filter chosen last (saved per browser; "Me" at first). "More" opens a menu with the other
pages (Log, Limits & rules, Notifications, Setup), the connection states of Jira, GitHub and the agent, and the language
switch. The lists of pull requests and agent runs are the first column of their pages, and every column scrolls on its
own. Details: [Layout and navigation](architecture.md#layout-and-navigation).

**Language.** The app starts in the language of the browser; change it in the "More" menu ("Language") or in the first step
of the setup assistant. The change applies at once and is saved in the workspace. The agent writes its plans and summaries
in the same language. Review comments and the text of pull requests follow the language of the pull request or ticket
instead (see the end of this page); the `pr_language` of a repo is the team default. Texts from Jira and GitHub stay as
they are.

## Pages

| Page | Address | What it does |
|------|---------|--------------|
| Setup | `#/setup`, `#/setup/<step>` | state and next step per service: sign in to / disconnect Jira and GitHub, check the agent endpoint, detect the container runtime |
| Tasks | `#/tasks` (the filter chosen last), `#/`, `#/mine`, `#/team`, `#/snoozed` | inbox of tickets and pull requests, filtered by switchable rules (mention, review request, blocked PR, overdue ticket, agent done, assigned); snooze; change status, create a branch, reply, assign the agent; edit a ticket; pull requests link to their ticket automatically (the ticket key in the branch name, title or description; there is no manual linking), the section "Development" lists every linked pull request with branch, state and checks; search; the "Filter" button with the number of active filters; keys `↑↓`/`j k`, `e`, `r`, `s`, `u`, `/` |
| Pull requests | `#/review/acme/app/87` | the list on the left has a "Filter" button (see [Reviews filter](#reviews-filter)); the description and all comments are shown as Markdown (no raw HTML); tabs "Files" (file tree with +/− counts, one collapsible diff per file, "Viewed" per file, long diffs cut at 300 lines with "Show more") and "Commits" (a click shows only that commit's diff, read only); select lines and write your own comments; approve, reject or edit comments of the agent; send the review; checks with log; merge (only you); before "Review PR" a switch Automatic / German / English picks the language of the agent's comments |
| Overview | `#/overview` | open entries and load per person; "Stuck" shows failed checks, waiting reviews, tickets without a branch and agents waiting for you |
| Notifications | `#/notifications` | per occasion switch the browser notification on or off; quiet hours; permission request by button (Cockpit never asks by itself); test notification |
| Agent | `#/agent/APP-142` | assignment with options; plan for approval; answer questions, send hints, pause; history as a conversation; for a finished run "Create pull request" (preview, editable); finished runs whose pull request is merged or closed (see below); "Clean up" |
| Limits & rules | `#/limits` | prices per million tokens, budget per run and month, time limit, parallel runs; per repo allowed/forbidden paths, required tests, instructions and plan requirement; usage today and this month |
| Log | `#/log` | every change with time and author (you, the agent, System) |

## Inbox filters

The "Filter" button at the top of the inbox shows the number of active filters as a badge and opens the **filter dialog** (a
centered card on wide screens, a sheet from the bottom on narrow ones). The dialog holds the Jira project, every filter
below, and the attention rules ("Mention", "Review request", … and "Status follows the pull request"; these switches act at
once). Filters combine with AND. "Apply" applies the edits together, "Reset" puts the edits back to the start state. The
applied filters are saved with the workspace, so they survive a reload. Every active filter is also a chip under the button;
its "✕" removes only that filter.

| ID | Label | Choices (default first) |
|----|-------|-------------------------|
| `source` | Source | All · Jira · GitHub (covers pull requests and GitHub Projects tickets) |
| `sprint` | Sprint (tickets only) | Current sprint · All open · Recently updated (14 days) · Backlog (no sprint) |
| `project` | Ticket project | all · each Jira project (a pull request counts to the project of the ticket key in its branch name or title) · No ticket (pull requests without a key) |
| `repo` | Repo | all · each repo (pull requests only) |
| `person` | Assignee | Everyone · Me · Unassigned · each person of the tickets (a Jira ticket without assignee has no owner) |
| `reporter`, `watched`, `type`, `priority`, `status`, `label` | Reporter, Watched by me, Type, Priority, Status, Label | all … |
| `done` | Done entries (tickets only) | hide · show |

"Current sprint" behaves like "All open" as long as no ticket carries sprint data (a project without sprints; the source
state then says so). Pull requests of colleagues never raise attention or notifications: only pull requests with a relation
to you (author, requested reviewer, assignee) can. In the inbox they are listed in the tabs "All" and "Team"; the tab "Me"
holds only your own entries. On the Pull requests page (`#/review`) the "Whose" selection (default "All") lists the pull
requests that concern you in every repo plus all open pull requests of the watched repos (`[github] repos`); "Mine" keeps
only those that concern you.

The Jira project and sprint choice also decides what the sync fetches (queries: [configuration.md](configuration.md#jira)).

**Assignee of a ticket.** In the ticket detail the assignee is a search field with suggestions: people already seen in the
workspace come first, then Jira's own search for people the ticket can be assigned to (`GET /api/jira/assignable`, read
only). Choosing one sets the assignee in Cockpit; it is not sent to Jira.

## People per source and "My team"

Jira people and GitHub users are different identities, and the same person often has both a Jira display name and a GitHub
login. The Overview therefore never mixes them by guess:

- **Source filter:** under the title a segmented control "All / Jira / GitHub", saved in the workspace like the other
  filters. People, load and "Stuck" follow it.
- **Sections:** under "All" the list has the sections "Jira and GitHub" (you, the agent and linked people), "Jira" and
  "GitHub". A person appears once per source; the same name in both sources stays two people.
- **Linking (optional):** choose a person and use "Is the same person as …" in the detail: search the name in the other
  source and pick it. The two then show as one person with both sources under "All"; "Remove link" splits them again. Every
  name has at most one link. Nothing is written to Jira or GitHub.
- **You** need no link: your Jira account and your GitHub login are both known, so "You" is one person with both sources.

On the Overview the button "My team" opens a dialog with the Jira teams seen in the synced tickets (the "Team" field, see
[configuration.md](configuration.md#jira)). Choose one or more teams and "Apply"; "Include tickets without a team" (off by
default) also shows tickets without a team. With no team chosen everything shows, and a hint says you can choose your team.
With a choice, the Overview (people, load, "Stuck", lists) and the inbox filters "All" and "Team" show only tickets of those
teams; people appear only if they have such entries, "You" and "Agent" always stay. What is yours (filter "Me", entries
assigned to you) is never hidden. A pull request is not filtered unless it is linked to a ticket (its branch, title or
description names the ticket key, or the ticket carries it) and none of its tickets is shown. Display only: nothing is
written to Jira.

## Tickets from GitHub Projects

With `[github_projects]` set up (see [setup-guide.md](setup-guide.md#github-projects-tickets-without-jira)) the project
items appear as tickets with the badge "GitHub Projects":

- **Key.** `acme/app#123`; a draft issue (it lives only in the project and has no number) is `acme/3#draft-1a2b3c`. In texts
  of that repo the short form `#123` is understood, and pull requests are linked to the ticket by `#123`, `owner/repo#123`
  or `Fixes #123` in branch, title or body, or by GitHub's own connection (the "Development" section lists them with their
  checks).
- **Status.** The columns of the project's status field map onto Open / In progress / Review / Done by their names ("In
  Progress", "In Review", "Done" …; a column Cockpit cannot place takes its position in the row). A closed issue without a
  status column is "Done".
- **Lists and filters.** "All / Me / Team": a ticket is yours if you are one of its assignees. The source filter has an entry
  "GitHub Projects". The sprint view "Current sprint" uses the iteration field; a ticket without an iteration stays visible.
  "My team" uses the project field `Team`. Repo and label filters work on the issue's repo and labels.
- **Detail.** Body and comments are shown as Markdown; the detail lists assignees, milestone, project, issue type, parent,
  sub-issues and every other project field (number, date, text, iteration, single select).
- **Agent.** The repo of the issue is offered first when you start the agent; the pull request it opens carries `Fixes #123`.

**Changing things.** Only on your click, and GitHub is asked first (if it says no, nothing changes in Cockpit and the
message names the missing right): the status buttons move the item to the matching column of the project; "Assigned to"
assigns or unassigns (the search lists the people the repo lets you assign); labels are added and removed in the section of
the project; "Close issue" / "Reopen issue"; "Edit" changes title and text (also of a draft); the comment box posts a
comment; priority and every other project field have their own control (empty clears the field); a draft shows "Convert to
issue" with the repo to create it in; and "New issue" next to the filters creates an issue in a project (optionally in a
column) and adds it to the project. "Status follows the pull request" brings the status of a linked ticket in line on your
screen when the pull request moves; it does not write to GitHub by itself.

## Reviews filter

The Pull requests page lists the open pull requests that concern you (author, review requested, involved) in **every repo
your GitHub access can see**, plus all open pull requests of the repos you watch (see
[configuration.md](configuration.md#several-repos)). Nothing in the setup limits this by Jira project or repo. The "Filter"
button above the list opens a dialog:

- **Whose:** "All" (the default: everything the list loads) or "Mine" (only what concerns you).
- **Repos:** every repo that has pull requests, grouped by owner or organization, each with its number of pull requests;
  tick an owner to choose all its repos. A search field appears when there are more than eight repos. Nothing ticked means
  all repos.
- **GitHub Projects:** shown when GitHub Projects are connected; only pull requests that are linked to an item of the chosen
  project (on the issue's timeline or named in it) stay.

**Sections.** The list is grouped by repo: one section per `owner/name` with a header that shows the number of pull requests
(and, in a badge, how many need your attention: a review requested from you, or your own pull request with requested
changes). Sections with the most pull requests that need you come first, then alphabetically. With only one repo the list has
no header. With many repos (eight or more, from more than one owner) the repos sit under a header per owner or
organization. A click on a header collapses or expands it; the collapsed ones are remembered per browser (`localStorage`).
With the focus in the list, ↑ and ↓ move to the previous or next pull request and skip collapsed sections. A link to a pull
request (`#/review/acme/app/87`, "Go to entry") opens it, expands its section if needed and scrolls the entry into view.

"Apply" stores the choice with the workspace, so it is still there after a restart; "Reset" clears it. The filter only
narrows the list: the sync still loads everything, and the review you have open stays in the list. A line under the button
says how many repos or projects are shown and has "Show all".

## Agent runs

**Choosing the repo at the start.** The start dialog of a ticket has a searchable list of every repo your GitHub access can
see. The suggestions come first, each with a short reason: the repo of the issue, used before for tickets of the same Jira
project, has a pull request or branch for this ticket, named in the ticket, or the default repo. Without a choice the agent
starts in the default repo (`[agent] repo`, or the only configured repo); with several repos and no default the start asks
you to choose. A valid `owner/name` that is not listed can be used as typed. The first time the agent works in a repo,
Cockpit reads its default branch (and a branch pattern proposal from `CONTRIBUTING.md`) from GitHub and remembers it in
`[[agent.repos]]`.

### Working in several repos at once

You can tick **more than one repo**. The first one is the main repo. The agent works in all of them at the same time and
changes only those the ticket needs. Each repo gets its own branch (named by the rules of that repo) and its own pull
request. On the run page the section "Repos of this run" shows every repo with its branch and its state: *Unchanged*
(nothing was pushed), *Pushed*, *Pull request created* or *Push refused* (with the reason). If one repo is refused – for
example because of a forbidden path – the other repos are still pushed. You create the pull request per repo with its own
button; the text of each lists the other pull requests of the same run. Deleting the branch after the merge also works per
repo. The pull requests show up in the ticket under "Development" as usual, because they all carry the ticket key.

More about agent behaviour (questions, steering, follow-up jobs, costs, limits, rules per repo) is in
[architecture.md](architecture.md#agent-behaviour).

### What a run leaves behind

Containers are started with `--rm`, and the clone under `<data>/agent/runs/<run>` is deleted when a run ends (it only stays
while a run is halted by a limit or waits for you). What can remain is the branch on GitHub, the run in the list and, after a
pause or a cancel, a clone.

- **"Done".** The sync asks GitHub about the pull request of every quiet run (finished, ready, paused or halted – never one
  that works). If it is merged the run gets the state **"Done (merged)"**, if it is closed without merging **"Done
  (closed)"**. A paused or halted run is ended first, and its clone is deleted. Starting the ticket again clears the state.
- **Hidden by default.** The list of runs hides done runs. The switch **"Show finished (n)"** above the list (only shown
  when there are any) brings them back; the run you have open stays visible.
- **"Delete branch".** On a done run that Cockpit implemented, the pane shows the branch and the button. Pressing it opens a
  question that names the branch; only on confirmation the backend deletes exactly that branch on GitHub. The button is off,
  with the reason next to it, unless **all** hold: the pull request is merged or closed, the branch fits the branch pattern
  of the repo, it is not the base branch, the default branch of the repo or a protected branch, it is the head of that pull
  request and the branch the run pushed. The backend reads this from GitHub again at the click. Nothing deletes a branch by
  itself.
- **"Clean up".** The button under the list of runs first shows a preview: containers with the label `cockpit.run` that
  belong to no working run, and folders under `<data>/agent/runs` that no active run uses. Only after "Remove now" they are
  removed (containers with `docker`/`podman rm -f`, never via the socket). The image and containers without the label are
  never touched. If the container program is not reachable, only folders are listed. The removal re-checks every item and
  skips whatever started working in the meantime.
- **Removing finished runs: "Remove" and "Remove all finished (n)".** Every run in a final state – done, cancelled or failed,
  halted by a limit, or "done" – has the button **"Remove"** under its buttons, and the list has **"Remove all finished
  (n)"** with the number of runs that go. Both ask first and say that the change is local. Runs that work, wait for approval
  or an answer, or are paused cannot be removed: cancel them first. Only the run disappears from the list of this computer;
  branches and pull requests on GitHub are never touched. Containers with the label `cockpit.run` that carry the name of a
  removed run, and its folder under `<data>/agent/runs`, are removed right after (the same code as "Clean up"). The removal
  is saved with the workspace, so the run does not come back after a sync, a reload or a restart of the server. The
  consumption is a separate ledger: the spend of the month and the day keeps the cost of removed runs, so the budget limits
  stay correct.

### Language of agent reviews and pull request texts

Agent reviews (summary, comments, suggestions) are written in the language of the pull request, German or English. The
language is chosen in this order:

1. the switch in the review panel (Automatic / German / English), if you set it to German or English;
2. the language detected from the title and description of the pull request, plus the description of the linked ticket if
   the pull request description is short. Detection counts typical German and English words and umlauts; it stays silent for
   very short, code-only or mixed texts;
3. the `pr_language` of the repo in `[[agent.repos]]` (the team default);
4. English.

The review panel shows the result, for example "Review language: English – detected from the PR description". The same rule
(without the switch) writes the pull request texts and commit messages of the agent's own runs: detected from the ticket,
else the repo's `pr_language`, else English. Nothing is sent without your approval.
