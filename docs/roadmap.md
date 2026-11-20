# Roadmap

Features that are planned but not built yet. Nothing here is available in the app.

## GitHub Projects as ticket source (instead of Jira)

Work without Jira: Cockpit reads tickets from a GitHub Project (Projects v2) of the repo or organization.

- **What is read:** items (issues and draft issues), the Status field as board columns, assignees, labels, and custom
  fields such as Team or Priority.
- **Keys like `#123` / `acme/app#123`**, so that auto-linking of pull requests and the agent work as for Jira tickets.
- **Read-only first.** Writes such as status or assignee follow the same approval rules as for Jira.

Technical notes:

- The Projects v2 API is GraphQL only.
- The token needs the `read:project` scope (classic). A fine-grained token needs the organization permission
  "Projects: Read". A GitHub App needs the "Projects" permission.
- The sync stores the items like Jira tickets, with a new `Source` value.

Open questions: one project or several, and which field counts as status.
