# Roadmap

Features that are planned but not built yet. Nothing here is available in the app.

## Local tickets and board without Jira

Work without Jira: create tickets in Cockpit when no Jira is connected (optionally alongside Jira).

- **A ticket has** a title, a Markdown description, a type, a priority, an assignee from a local people list, labels, a
  due date, comments and a history.
- **Keys like `COCK-1`**, so that auto-linking of pull requests and the agent work as for Jira tickets.
- **A Kanban page "Board"** for the local tickets.
- **Storage:** its own table `local_tickets` in `config/cockpit.db`, with the source `Source::Local`. The sync never
  touches these tickets.

Planned as three PRs:

1. Core and storage: the `local_tickets` table, `Source::Local`, keys, actions.
2. UI and a setup step: create and edit tickets, the local people list.
3. The board page.

Open questions: fixed or custom columns; one or several local boards.
