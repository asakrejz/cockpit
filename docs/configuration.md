# Configuration reference

This page is the only reference for `config.toml`, the environment variables and `config/.env`. The setup assistant
(page "Setup", German "Einrichten") writes most of it for you; step-by-step instructions are in the
[setup guide](setup-guide.md). [`config/config.example.toml`](../config/config.example.toml) is a commented example with
dummy values to copy from.

Order of precedence: defaults, then the file, then environment variables.

## Where the files live

`config.toml`, the database `cockpit.db` and the key file `.env` live in the folder `config/` of the project. "Project
folder" is the working directory of the server, the folder that holds `config/`; `scripts/start.sh`,
`start-cockpit.command` and the launchd template (`WorkingDirectory`) all start there. The server creates `config/` at
the first start (mode `700`, files `600`) and an empty `config.toml` if there is none. `COCKPIT_CONFIG` stops it from
creating `config.toml`; with `COCKPIT_CONFIG` and `COCKPIT_DB` both set it creates nothing. Git ignores everything in
`config/` except `config/config.example.toml`.

**Safety net for `config/`:** at start and in `GET /api/health` (`config_files`, shown in the last assistant step) the
server warns when Git tracks a file in `config/` other than the example, when one is not ignored, or when permissions are
looser than `700`/`600` (at start they are tightened). CI (`scripts/check-tracked-files.sh`, first step of the `check`
job) fails when anything but `config/config.example.toml` is tracked there. The folder never enters the agent image: the
build context is `apps/agent`, and its `.dockerignore` excludes `config`.

## `config.toml`

Everything is optional.

```toml
port = 8787
# dist = "/path/to/apps/cockpit/dist"   # built web app (default apps/cockpit/dist)
# db = "/path/to/cockpit.db"            # SQLite file (default config/cockpit.db)
seed = "empty"                    # or "demo", see "Seed data"
interval_s = 120                  # sync interval in seconds, default 120, at least 30

[github]
# api_url = "https://ghe.example/api/v3"   # only for GitHub Enterprise
# repos = ["acme/app"]            # optional "watched" repos: all their open pull requests are loaded
# auth = "token"                  # "token" or "oauth": the sign-in the server uses (see below)

[jira]
site = "https://acme.atlassian.net"   # name ("acme") or address
email = "me@acme.example"
# projects = ["APP", "WEB"]       # optional: only these projects
# jql = "project = APP AND ..."   # optional: your own query instead of the projects
# hidden_fields = ["Rank", "customfield_10099"]   # fields (name or ID) the ticket detail leaves out
# field_order = ["Acceptance criteria"]           # fields shown first, in this order
# team_field = "customfield_10400"                # ID of the "Team" field, only if the detection fails
# auth = "token"                  # "token" or "oauth", as in [github]

[github_oauth]                    # sign-in with a GitHub App instead of a token: only the client ID
client_id = "Iv23liDummyClientId"

[atlassian]                       # sign-in with an Atlassian OAuth app: only the client ID
client_id = "dummyAtlassianClientId"
# site = "acme"                   # name, address or id; only needed when several sites are approved

[github_projects]                 # GitHub Projects as a source of tickets, see below
tickets_from = "github_projects"  # "jira", "github_projects" or "both" (default "both" when the section exists)
projects = ["acme/3", "acme/1"]   # owner/number, as in the project's address
status_field = "Status"           # optional: the single-select field that is the ticket status
team_field = "Team"               # optional: the field that holds the team

[agent]
provider = "claude-agent-sdk"     # or "claude-code" or "codex"
container = "podman"              # or "docker"; detected when omitted
repo = "acme/app"                 # default repo of the agent (optional)
# dir = "/path/to/agent"          # folder with Containerfile and runner (default ./apps/agent)

[[agent.repos]]                   # optional settings per repo
repo = "acme/web"
base_branch = "develop"           # optional, default "main"
branch_pattern = "feature/{ticket}-{slug}"   # optional, default "claude/{ticket}-{slug}"
pr_language = "de"                # optional, "en" (default) or "de"
# watch = false                   # true (default): the sync also loads all open pull requests of this repo

[[agent.repos]]
repo = "acme/app"

[agent.endpoint]                  # model access, see "Model provider and endpoint"
url = "https://litellm.example"   # empty or omitted = directly at the provider
model = "claude-sonnet-5-5"

[server]                          # optional webhooks, see the setup guide
webhooks = false                  # default: both webhook routes answer 404
public_url = "https://cockpit.example"   # your tunnel; only shown in the assistant

[sync]
webhook_fallback_interval_s = 600 # poll interval of a source while its webhooks arrive (default: 5x interval_s, at most 600)
max_prs_per_repo = 200            # open pull requests loaded per repo (default 200, 1 to 2000)
max_prs_total = 500               # … over all repos together (default 500, 1 to 2000)
```

Only the keys shown here exist. A key the server does not know (for example a typo) stops the start: the message names
the key, its section and the file (`deny_unknown_fields`). Fix the name or delete the line; the setup assistant refuses
to save while the file has such a key. The only exception is a `projects` key in an `[[agent.repos]]` entry, which is
accepted and ignored.

A source only syncs if it is set up here **and** its key is in `config/.env`. Without a source the workspace stays empty.
Inputs are validated like at server start, whether they come from the file or from the assistant.

### Environment variables

| Variable | Meaning (default) |
|----------|-------------------|
| `COCKPIT_PORT` | port (8787) |
| `COCKPIT_DIST` | built app (`apps/cockpit/dist`) |
| `COCKPIT_DB` | SQLite file (`config/cockpit.db` in the project folder) |
| `COCKPIT_CONFIG` | path of `config.toml` (`config/config.toml`); an explicitly named file must exist |
| `COCKPIT_ENV_FILE` | path of the key file (`config/.env` in the project folder) |
| `COCKPIT_SEED` | `empty` or `demo` |
| `COCKPIT_CONTAINER` | `docker` or `podman` |
| `COCKPIT_AGENT_IO` | `bind` or `copy`: how folders reach the agent container; default `copy` inside the Cockpit container, otherwise `bind` ([container.md](container.md#no-paths-must-match-copy-instead-of-bind-mounts)) |
| `COCKPIT_AGENT_DIR` | folder with `Containerfile` and runner (same as `[agent] dir`) |
| `COCKPIT_GIT_BASE` | base address for clones instead of `https://github.com` (tests, local remotes) |
| `COCKPIT_DEV_PORT` | allow the `trunk serve` port (environment only, see [architecture.md](architecture.md#development)) |
| `COCKPIT_BIND` | listen address, default `127.0.0.1`; the container image sets `0.0.0.0` (environment only, see [container.md](container.md)) |
| `COCKPIT_PUBLIC_PORT` | published host port of a container if it differs from the server port (environment only) |
| `COCKPIT_IN_CONTAINER` | `1` says that Cockpit runs in its own image (the image sets it); without it, or with `0`, the marker files `/.dockerenv` and `/run/.containerenv` decide. Changes the container step of the assistant and the default of `COCKPIT_AGENT_IO` |

The variables of `compose.yaml` and `scripts/cockpit-container.sh` (`COCKPIT_IMAGE`, `COCKPIT_NAME`, `COCKPIT_VOLUME`,
`COCKPIT_PULL`, `DOCKER_SOCK`) are in [container.md](container.md); those of the agent runner (`COCKPIT_PROVIDER`,
`COCKPIT_MODEL`, `COCKPIT_JOB`, `COCKPIT_ROOT`) in [apps/agent/README.md](../apps/agent/README.md).

## Pull requests

As soon as GitHub is connected the sync runs; **no repo list is needed**. It searches (`GET /search/issues`) with
`is:pr is:open author:@me`, `is:pr is:open review-requested:@me` and `is:pr is:open involves:@me` across every repo the
credential can see and merges the hits without duplicates (three search calls per tick, at most 30 hits each; every hit
is then loaded as a pull request). Repos in `[github] repos` are *watched* repos on top of that: *all* their open pull
requests are loaded (most recently updated first, 100 per call), not only yours; their search hits are skipped. A failed
search (for example its separate rate limit) is only a hint in the source state; what was loaded before stays. Beyond
`max_prs_per_repo` / `max_prs_total` the rest is left out and the source state says "only the first N PRs per repo
loaded". Your relation to a pull request comes from the pull request itself (author, requested reviewers, assignees);
review requests to a *team* are not detected.

The number of a ticket links to `<site>/browse/<KEY>`. The site is `[jira] site`; with OAuth it is the address of the
chosen site, stored at sign-in. Without either there is no link. The number of a pull request links to GitHub.

## Jira

**Query.** Without `jql` the sync asks for the active sprints:
`sprint in openSprints() [AND project in (<projects>)] ORDER BY updated DESC`. If Jira rejects `openSprints()` (project
without sprints) or there is no active sprint, the same query runs with `statusCategory != Done` instead and the source
state says "This project has no sprints – showing all open tickets". A `jql` of your own is used exactly as written, is
never rewritten and its errors are shown as errors. The sprint field of the site is found through `/rest/api/3/field`
(type `gh-sprint`, otherwise the name "Sprint"), once per sync.

The project and sprint choice in the inbox's filter dialog changes the query (read queries only, always for all tickets of
the project, not only yours):

| Project | Sprint in the dialog | Query |
|---------|----------------------|-------|
| none | Current sprint (default) | `sprint in openSprints()` (restricted to `projects`), or your own `jql` |
| `APP` | Current sprint | `project = APP AND sprint in openSprints()` |
| `APP` or none | All open | `project in (APP) AND statusCategory != Done` |
| `APP` or none | Backlog | … `AND sprint is EMPTY` |
| `APP` or none | Recently updated | … `AND updated >= -14d` |

A chosen project takes precedence over `projects` and `jql`; "All projects" restores them. Without a chosen project a
sprint view other than "Current sprint" asks for the `projects` of the file (all projects if there are none); an own `jql`
is only replaced when a project is chosen. The choice is saved with the other filters; after a change the server syncs
immediately.

**Ticket details.** The sync asks Jira for all fields (`fields=*all`, without comments and work logs) with
`expand=names`, so custom fields have a name. The ticket detail shows the non-empty standard fields (status, type,
priority, assignee, reporter, labels, components, fix version, sprint, story points, due date, created, updated, parent,
linked tickets) and all non-empty custom fields under "Details"; fields of formatted text such as "Acceptance criteria"
are shown formatted, and the attachments are listed with name, size and type (pictures as a preview). `hidden_fields` and
`field_order` name fields by their name or ID (`customfield_10042`), in any case.

**Team field.** Several teams often share a Jira project. The sync reads the Atlassian "Team" field of each ticket (id and
name) so that the Overview and the task list can be limited to your teams. The field is found in `/rest/api/3/field`: the
field of the team type (`com.atlassian.teams:rm-teams-custom-field-team` or `…:atlassian-team`), otherwise the field named
"Team", once per sync. If the detection fails on your site, set `[jira] team_field`. A ticket without a team has "no
team"; a site without the field has no teams (nothing is filtered and the choice is not offered). Which teams you choose
("My team" on the Overview page) is stored in the database, not in this file. Nothing is written to Jira.

## Choosing the sign-in method (`auth`)

`[github] auth` and `[jira] auth` say which credentials the server uses: `"token"` (API token or personal access token
from `config/.env`) or `"oauth"` (the sign-in via the GitHub App or the Atlassian app). The choice is honoured everywhere:
sync, agent follow-ups, the repo list of the setup, and the connection status in `/api/health` (`sign_in.<service>.kind`,
`connected`, plus `choice` and `oauth_stored`). The rule lives in one place, `cockpit_core::AuthChoice::pick`.

- `token`: only the token counts. A stored OAuth sign-in is kept but not used (disconnect it in the setup if you do not
  need it). With GitHub the repo list then comes from `GET /user/repos`, so repos of organizations appear even if the
  GitHub App is not installed there.
- `oauth`: only a working sign-in counts; a stored token is kept but not used. Without a working sign-in the service shows
  as not connected.
- Not set: a working OAuth sign-in goes first, an expired one falls back to the token.

The setup assistant writes the key: saving a token sets `token`, completing an OAuth sign-in sets `oauth`, and switching
the picker to a way that is already set up stores that way. Any other value is a start error.

With `[atlassian]` the `[jira]` section needs neither `site` nor `email` (only `projects`/`jql` and the display options).

## Several repos

**No configuration is needed.** The repo of an agent run is chosen in the start dialog of the ticket, from every repo the
credential can see (`GET /user/repos`, or the installation repos with the GitHub App; kept for 5 minutes, at most 500).
Any `owner/name` is allowed. The dialog suggests repos first, each with a short reason: (1) the repo of a GitHub issue
ticket, (2) repos earlier runs used for tickets of the same Jira project, (3) repos of pull requests or branches linked to
the ticket, (4) `owner/name` or GitHub addresses in the ticket text, (5) the default repo `[agent] repo`. A run can work
in several repos (see [using-cockpit.md](using-cockpit.md#working-in-several-repos-at-once)).

`[[agent.repos]]` holds the **settings per repo** (owner/name, base branch, branch pattern, PR language). A repo without
an entry uses its default branch from GitHub as the base branch and the usual branch pattern (or the proposal from its
`CONTRIBUTING.md`). The first time the agent starts in such a repo, Cockpit stores an entry with these values
(`watch = false`) so you can edit it later. `watch` (default `true`) says whether the sync also loads *all* open pull
requests of that repo, as if it were in `[github] repos`; entries written by hand watch, entries stored on first use do
not.

**Which repo a start uses.** The start dialog lets you choose the repo. Without a choice the agent uses the default repo:
`[agent] repo`, or the only repo when exactly one is configured. With several repos and no default the start asks you to
choose. The button that creates a branch for a Jira ticket always uses the default repo (it has no choice of its own) and
asks you to set one if there is none. The Jira project of a ticket does not decide the repo.

In the assistant the connection test looks at each watched repo read-only (`GET /repos/{owner}/{name}`: exists, visible,
push allowed). Pull requests are identified by `owner/name#number`: equal numbers from two repos stay separate in the
inbox, review, agent view, log and notifications; addresses look like `#/review/acme/app/87`.

## Language of pull requests

`pr_language` of a `[[agent.repos]]` entry (`"en"` or `"de"`, default `"en"`) is the team language of that repo: the
commit message the runner creates and the title and text of the pull request Cockpit builds are written in it. It is a
setting of its own and does not depend on the language of the interface. The setup assistant has a switch for it in every
repo row. The agent's own texts for you (plan, summary, questions, review comments) follow the language of the interface.

## Branch names

`branch_pattern` of a `[[agent.repos]]` entry says how the agent names its branches in that repo. Without it the branch is
`claude/{ticket}-{slug}`.

| Placeholder | Becomes | Example |
| --- | --- | --- |
| `{ticket}` | the ticket key (**required**) | `APP-142` |
| `{slug}` | the ticket title as a short name (at most 24 characters) | `toggle-focus-ring-missing` |
| `{type}` | `feature`, `fix` or `chore`, from the issue type in any language (Bug → `fix`; Task and Sub-task → `chore`; Story, Epic and everything else, also an unknown type, → `feature`; without an issue type the ticket's labels decide) | `fix` |

A pattern such as `feature/{ticket}-{slug}` gives `feature/APP-142-toggle-focus-ring-missing`. A pattern is refused (when
the file is read and when the assistant saves it) if it is empty, has no `{ticket}` (so it would point at a fixed branch or
let two tickets share one), contains an unknown placeholder, glues two placeholders together, or has characters other than
letters, digits and `/ - _ .`. An empty value counts as not set.

In the assistant (step "GitHub", the list of repos) the button "Suggest from guidelines" reads `CONTRIBUTING.md`,
`.github/CONTRIBUTING.md` and `docs/CONTRIBUTING.md` of the repo (read-only,
`GET /repos/{owner}/{name}/contents/{path}`) and recognizes usual names (`feature/`, `bugfix/`, `PROJ-123`). It only
**proposes**: you confirm with "Accept" and save the list yourself. Without a finding you get a hint. If the clone has such
a file, the runner also tells the agent to follow the commit conventions in it.

The pattern is enforced by the backend, see [security.md](security.md#the-agent).

## GitHub Projects as the source of tickets

`[github_projects]` (see the example above): `tickets_from` is `"jira"`, `"github_projects"` or `"both"` (default `"both"`
when the section exists). Without the section Jira is the only source of tickets. With `tickets_from = "github_projects"`
Jira is not asked at all (its settings stay in the file). The credential is the GitHub one (token or sign-in), see the
[rights table](setup-guide.md#github-projects-tickets-without-jira). A project that GitHub does not show to the credential
is a hint in the "Sync" card of the last setup step ("Done"), and its tickets stay until it can be read again.

## Seed data

The server starts with an empty workspace. `COCKPIT_SEED=demo` (or `seed = "demo"`) loads sample data, **only if no state
has been stored yet**; for a fresh seed use a new database (`COCKPIT_DB=/path/to/new.db`). Sample data otherwise serves as
test data and for the Playground stories. The demo seed contains a sample GitHub project (`acme/3`) with an issue
(`acme/app#31`) and a draft issue. It is static: no agent runs on it, the agent shows its normal "not set up" state until
you set it up. A sync adds the entries of Jira and GitHub next to the sample data.

## Credentials: the `.env` file

All keys (Jira token, GitHub token, key of the agent endpoint, client secrets, sign-in tokens) live in the file
`config/.env`, next to `config.toml` and `cockpit.db`. They stay separate from `config.toml` on purpose. **The file is
never committed**: `.gitignore` excludes `config/*` and `.env` / `.env.*` anywhere; no `.env*` file belongs in the repo.

- **Create:** the assistant and `cockpit-server token set <name>` create the file when needed (mode `600`). By hand: create
  `config/.env` with the variables from the table below.
- **Location:** `config/.env` in the server's working directory. `COCKPIT_ENV_FILE=/path/to/file` redirects it. The
  assistant shows the path in the last step.
- **Format:** `NAME=value`, comments with `#`, other lines are preserved on write. Values with special characters go in
  single quotes. The sign-in token blobs are compact one-line JSON (`'{"access":"…"}'`). Writing is atomic (temporary
  file, then rename).
- **Precedence:** a real environment variable of the same name wins over the file. A value set that way cannot be
  overwritten or deleted in the assistant.
- **Variables:**

  | Name in the backend | Variable in `config/.env` |
  |---|---|
  | `jira` | `JIRA_TOKEN` |
  | `github` | `GITHUB_TOKEN` |
  | `agent-key` | `AGENT_KEY` |
  | `github-oauth` | `GITHUB_OAUTH_CLIENT_SECRET` |
  | `atlassian-oauth` | `ATLASSIAN_OAUTH_CLIENT_SECRET` |
  | tokens of the GitHub sign-in (written by the backend) | `GITHUB_OAUTH_TOKENS` |
  | tokens of the Atlassian sign-in (written by the backend) | `ATLASSIAN_OAUTH_TOKENS` |
  | webhook secrets (optional) | `GITHUB_WEBHOOK_SECRET`, `JIRA_WEBHOOK_SECRET` |

- **Safety net:** at start and in `/api/health` (`env_file`, visible in the last assistant step) the server warns when Git
  tracks `config/.env`, `.gitignore` does not cover it, or the permissions are looser than `600` (at start they are set to
  `600`). CI fails when any `.env*` file is tracked. If a real `.env` was ever committed: rotate the keys immediately and
  run `git rm --cached config/.env`.
- **Container:** `config/.env` never enters an image or an agent container (`apps/agent/.dockerignore`); the agent receives
  values only as `-e NAME` from the backend.

**Terminal commands.** The value always comes from standard input, never from the command line:

```bash
echo "$TOKEN" | cargo run -p cockpit-server -- token set github     # or jira, github-oauth, atlassian-oauth, agent-key
cargo run -p cockpit-server -- token delete github                  # remove a key
```

In the Cockpit container the same command runs through the engine; the value is again read from standard input:

```bash
echo "$TOKEN" | docker exec -i -u cockpit cockpit cockpit-server token set jira
```

## Model provider and endpoint

`[agent] provider` picks the tool in the container; the backend passes it as `COCKPIT_PROVIDER` and the runner starts the
matching adapter:

| `provider` | Tool in the container | Endpoint family | Names passed with `-e` (values only in the process environment) |
|------------|-----------------------|-----------------|------------------------------------------------------------------|
| `claude-agent-sdk` (default) | Claude Agent SDK | Anthropic-compatible | `ANTHROPIC_BASE_URL` + `ANTHROPIC_AUTH_TOKEN` (with `url`) or `ANTHROPIC_API_KEY` (direct), `ANTHROPIC_MODEL` |
| `claude-code` | Claude Code CLI (`claude -p --output-format stream-json`) | Anthropic-compatible | same as above |
| `codex` | Codex CLI (`codex exec --json`, sandbox `workspace-write` without network, approval `never`) | OpenAI-compatible | `OPENAI_BASE_URL` (with `url`; `/v1` is added), `OPENAI_API_KEY`, `COCKPIT_MODEL` |

All three run under the same rules (see [security.md](security.md#the-agent)); the CLI adapters cannot ask you questions or
take hints while running (hints reach the agent with the next run). `[agent.endpoint]` says where model requests go: with
`url` to a compatible endpoint (LiteLLM, another proxy), without `url` directly to the provider of the tool (Anthropic,
for `codex` OpenAI); `model` is the name as the endpoint offers it. The key is `agent-key` in `config/.env`.

"Load models" calls `GET /api/agent/models`; the backend then queries `GET <url>/v1/models` (proxy: `Authorization:
Bearer`; direct at Anthropic: `https://api.anthropic.com/v1/models` with `x-api-key` and `anthropic-version`; direct at
OpenAI, for `codex`: `https://api.openai.com/v1/models` with `Authorization: Bearer`) without following redirects, with a
timeout and a size limit, and returns only the IDs. "Test connection" calls `GET /api/agent/check` and reports
*reachable*, *key accepted* and *model present*, with a hint on errors. Without a key it does not call the endpoint.
`/api/health` reports under `endpoint` provider, address, model and whether a key exists (never the key). An endpoint that
offers no model list: enter the model name by hand.

## Container runtime

The backend chooses: `COCKPIT_CONTAINER`, else `[agent] container`, else it looks for `docker`, then `podman` in the `PATH`
and checks with `<runtime> info` whether the engine runs (a process call, never the socket; the result is cached for 10
seconds). `/api/health` reports under `container` `runtime`, `engine_running`, `source` (`env`, `config`, `detected`,
`none`) and a `hint`.

## Start at login (launchd, macOS)

`scripts/de.cockpit.server.plist` is a template for a LaunchAgent. **Nothing is installed automatically.**

1. Run `scripts/start.sh` once by hand so that everything is built.
2. Copy the template to `~/Library/LaunchAgents/de.cockpit.server.plist` and replace the placeholders `/PATH/TO/REPO` (the
   repo folder) and `/Users/YOUR_USER` (your home directory); see the comment in the file.
3. Load: `launchctl bootstrap gui/$(id -u) ~/Library/LaunchAgents/de.cockpit.server.plist`,
   unload: `launchctl bootout gui/$(id -u) ~/Library/LaunchAgents/de.cockpit.server.plist`.
   The log is at `~/Library/Logs/cockpit-server.log`.
