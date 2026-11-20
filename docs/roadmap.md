# Roadmap

Features that are planned but not built yet. Nothing here is available in the app.

## GitHub Projects as ticket source (instead of Jira)

Work without Jira: Cockpit reads tickets from a GitHub Project (Projects v2) of the repo or organization.

- **What is read:** items (issues and draft issues), the Status field as board columns, assignees, labels, and custom
  fields such as Team or Priority.
- **Keys like `#123` / `acme/app#123`**, so that auto-linking of pull requests and the agent work as for Jira tickets.
- **Read-only first.** Writes such as status or assignee follow the same approval rules as for Jira.
- **Create a project.** If the selected repo has no linked GitHub Project, the setup offers "Create project". Only on
  an explicit click, never automatically, Cockpit creates a Projects v2 project for the repo owner (GraphQL
  `createProjectV2`), links it to the repo (`linkProjectV2ToRepository`) and sets up a Status field with the default
  columns Todo, In Progress, In Review and Done.

Technical notes:

- The Projects v2 API is GraphQL only.
- The token needs the `read:project` scope (classic). A fine-grained token needs the organization permission
  "Projects: Read". A GitHub App needs the "Projects" permission.
- Creating a project needs write rights: the `project` scope (classic token), the organization or user permission
  "Projects: Read and write" (fine-grained token) or "Projects: write" (GitHub App). Without them, Cockpit explains
  which right is missing.
- The sync stores the items like Jira tickets, with a new `Source` value.

Open questions: one project or several, which field counts as status, and whether a created project belongs to the
organization or the user.
