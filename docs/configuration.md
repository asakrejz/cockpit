# Configuration reference

The setup assistant ("Einrichten") writes most of it for you; this page is the
reference for editing by hand. Step-by-step instructions are in the [setup guide](setup-guide.md).

## config.toml and environment variables

Everything is optional. The file `config.toml` lives in the folder `config/` of the project, next to the database
`cockpit.db` (`config/config.toml`, `config/cockpit.db`). "Project folder" is the working directory of the server, the
same folder that holds `config/`; `scripts/start.sh`, `start-cockpit.command` (also in the release archive) and
the launchd template (`WorkingDirectory`) all start there. The server creates `config/` at the first start (mode `700`,
files `600`) and an empty `config.toml` if there is none. Git ignores everything in `config/` except
[`config/config.example.toml`](../config/config.example.toml), a fully commented example with dummy values to copy
from. Environment variables override the file:

| Variable | Meaning (default) |
|----------|-------------------|
| `COCKPIT_PORT` | port (8787) |
| `COCKPIT_DIST` | built app (`apps/cockpit/dist`) |
| `COCKPIT_DB` | SQLite file (`config/cockpit.db` in the project folder) |
| `COCKPIT_CONFIG` | path of `config.toml` (`config/config.toml`); an explicitly named file must exist |
| `COCKPIT_ENV_FILE` | path of the secrets file (`config/.env` in the project folder) |
| `COCKPIT_SEED` | `empty` or `demo` |
| `COCKPIT_CONTAINER` | `docker` or `podman` |
| `COCKPIT_AGENT_IO` | `bind` or `copy`: how folders reach the agent container; default `copy` inside the Cockpit container, otherwise `bind` ([container.md](container.md#no-paths-must-match-copy-instead-of-bind-mounts)) |
| `COCKPIT_AGENT_DIR` | folder with `Containerfile` and runner |
| `COCKPIT_GIT_BASE` | git base for clones (tests, local remotes) |
| `COCKPIT_DEV_PORT` | allow the `trunk serve` port (environment only, see [architecture.md](architecture.md#development)) |
| `COCKPIT_BIND` | listen address, default `127.0.0.1`; the container image sets `0.0.0.0` (environment only, see [container.md](container.md)) |
| `COCKPIT_PUBLIC_PORT` | published host port of a container if it differs from the server port (environment only) |

```toml
port = 8787
seed = "empty"                    # or "demo", see "Seed data"
interval_s = 120                  # sync interval, default 2 minutes, at least 30

[github]
# api_url = "https://ghe.example/api/v3"   # only for GitHub Enterprise
# repos = ["acme/app"]            # optional "watched" repos: all their open pull requests are loaded
# auth = "token"                  # "token" or "oauth": the sign-in the server uses (see below)
[jira]
site = "https://acme.atlassian.net"
email = "me@acme.example"
# auth = "token"                  # "token" or "oauth", as in [github]
[agent]
provider = "claude-agent-sdk"     # or "claude-code" or "codex"
container = "podman"              # or "docker"; detected when omitted
repo = "acme/app"                 # default repo of the agent (optional, see "Several repos")
[[agent.repos]]                   # optional settings per repo (the repo itself is chosen at the agent start)
repo = "acme/web"
base_branch = "develop"           # optional, default "main"
branch_pattern = "{type}/{ticket}-{slug}"   # optional, default "claude/{ticket}-{slug}", see "Branch names"
[[agent.repos]]
repo = "acme/app"
[agent.endpoint]                  # model access, see "Model provider and endpoint"
url = "https://litellm.example"   # empty or omitted = directly at Anthropic
model = "claude-sonnet-5-5"
[server]                          # optional webhooks, see [setup-guide.md](setup-guide.md#live-updates-and-webhooks)
webhooks = false                  # default: both webhook routes answer 404
public_url = "https://cockpit.example"   # your tunnel; only shown in the assistant
[sync]
webhook_fallback_interval_s = 600 # poll interval of a source while its webhooks arrive (default: 5x interval_s, at most 600)
max_prs_per_repo = 200            # open pull requests loaded per repo (default 200, 1 to 2000)
max_prs_total = 500               # … over all repos together (default 500, 1 to 2000)
```

**Pull requests.** As soon as GitHub is connected the sync runs – **no repo list is needed**. It searches
(`GET /search/issues`) with `is:pr is:open author:@me`, `is:pr is:open review-requested:@me` and
`is:pr is:open involves:@me` across every repo the credential can see and merges the hits without duplicates (three
search calls per tick, at most 30 hits each; every hit is then loaded as a pull request). Repos in `[github] repos` are
*watched* repos on top of that: *all* their open pull requests are loaded (most recently updated first, 100 per call),
not only yours; their search hits are skipped. A failed search (e.g. its separate rate limit) is only a hint in the
source state; what was loaded before stays. The limits below also bound the search results. Beyond `max_prs_per_repo` / `max_prs_total` the rest is left out and the source state says
"Nur die ersten N PRs je Repo geladen". Your relation to a pull request comes from the pull request itself (author,
requested reviewers, assignees); review requests to a *team* are not detected.

**Jira query.** Without `jql` the sync asks for the active sprints:
`sprint in openSprints() [AND project in (<projects>)] ORDER BY updated DESC`. If Jira rejects `openSprints()` (project
without sprints) or there is no active sprint, the same query runs with `statusCategory != Done` instead and the source
state says "Dieses Projekt hat keine Sprints – zeige alle offenen Tickets". A `jql` of your own is used exactly as
written, is never rewritten and its errors are shown as errors. The sprint field of the site is found through
`/rest/api/3/field` (type `gh-sprint`, otherwise the name "Sprint"), once per sync.

**Ticket details.** The sync asks Jira for all fields (`fields=*all`, without comments and work logs) with
`expand=names`, so custom fields have a name. The ticket detail shows the non-empty standard fields (status, type,
priority, assignee, reporter, labels, components, fix version, sprint, story points, due date, created, updated, parent,
linked tickets) and all non-empty custom fields under "Details"; fields of formatted text such as "Acceptance
criteria" are shown formatted, and the attachments are listed with name, size and type (pictures as a preview). To change
this, name fields by their name or ID (`customfield_10042`), in any case:

```toml
[jira]
hidden_fields = ["Rank", "customfield_10099"]   # never shown
field_order = ["Acceptance criteria", "Team"]   # shown first, in this order; the rest follows
```

**Team field ("Mein Team").** Several teams often share a Jira project. The sync reads the Atlassian "Team" field of each
ticket (id and name) so the Overview and the task list can be limited to your teams. The field is found in
`/rest/api/3/field`: the field of the team type (`com.atlassian.teams:rm-teams-custom-field-team`, older sites
`…:atlassian-team`), otherwise the field named "Team", once per sync. If the detection fails on your site, set the ID:

```toml
[jira]
team_field = "customfield_10400"   # optional; replaces the detection
```

A ticket without a team has "no team"; a site without the field simply has no teams (nothing is filtered, and the
choice is not offered). The choice of teams itself is made in the app (Overview, "Mein Team") and stored in the
database, not in this file. Nothing is written to Jira.

The number of a ticket links to `<site>/browse/<KEY>`. The site is `[jira] site`; with OAuth it is the address of the chosen
site, stored at sign-in (a sign-in from an older version stores none until you sign in again – then `[jira] site` is
used, and without either there is no link). The number of a pull request links to GitHub.


## Choosing the sign-in method (`auth`)

`[github] auth` and `[jira] auth` say which credentials the server uses: `"token"` (API token or personal access
token from `config/.env`) or `"oauth"` (the sign-in via the GitHub App or the Atlassian app). The choice is honoured
everywhere: sync, agent follow-ups, the repo list of the setup, and the connection status in `/api/health`
(`sign_in.<service>.kind`, `connected`, plus `choice` and `oauth_stored`). The rule lives in one place,
`cockpit_core::AuthChoice::pick`.

- `token`: only the token counts. A stored OAuth sign-in is kept but not used (disconnect it in the setup if you do
  not need it). With GitHub the repo list then comes from `GET /user/repos`, so repos of organizations appear even
  if the GitHub App is not installed there.
- `oauth`: only a working sign-in counts; a stored token is kept but not used. Without a working sign-in the
  service shows as not connected.
- Not set (older configs): a working OAuth sign-in goes first, an expired one falls back to the token.

The setup assistant writes the key: saving a token sets `token`, completing an OAuth sign-in sets `oauth`, and
switching the picker to a way that is already set up stores that way. Any other value is a start error.

## Where the files live, and moving from older versions

Older versions kept `config.toml` in `~/Library/Application Support/cockpit/` (macOS) or `~/.config/cockpit/`
(`$XDG_CONFIG_HOME`, Linux) and the database next to it (Linux: `~/.local/share/cockpit/`, `$XDG_DATA_HOME`). On the
first start without `COCKPIT_CONFIG` and `COCKPIT_DB`, the server **copies** these files to `config/` if the new ones do
not exist yet (the database through SQLite's `VACUUM INTO`, which also takes committed data from a `-wal` file), sets
the modes and logs `Copied config from … to …; the old files were left in place and can be deleted`. It never
overwrites a file in `config/` and deletes nothing at the old place. While old files still exist, `/api/health`
(`config_files.notices`) and the last assistant step say so ("Alte Konfiguration liegt noch unter …, du kannst sie
löschen"); delete them yourself when you are happy. Setting `COCKPIT_CONFIG` or `COCKPIT_DB` switches off creating and
migrating. Stop a still running server of the old version before the first start of the new one.

**Safety net for `config/`:** at start and in `/api/health` (`config_files`, shown in the last assistant step) the
server warns when Git tracks a file in `config/` other than the example, when one is not ignored, or when permissions
are looser than `700`/`600` (at start they are tightened). CI (`scripts/check-tracked-files.sh`, first step of the `check` job)
fails when anything but `config/config.example.toml` is tracked there. The folder never enters the agent image: the
build context is `apps/agent`, and its `.dockerignore` excludes `config` as well. The release archive contains only
`config/config.example.toml`.

## Several repos

**No configuration is needed.** The repo of an agent run is chosen in the start dialog of the ticket, from every repo
the credential can see (`GET /user/repos`, or the installation repos with the GitHub App; kept for 5 minutes). Any
`owner/name` is allowed. The dialog suggests repos first, each with a short reason: (1) repos earlier runs used for
tickets of the same Jira project (remembered from the runs in the database), (2) repos of pull requests or branches
linked to the ticket, (3) `owner/name` or GitHub addresses in the ticket text, (4) the default repo `[agent] repo` and, in older files, `projects` lists (see below). A run can work in several repos (see [using-cockpit.md](using-cockpit.md#working-in-several-repos-at-once)).

`[[agent.repos]]` holds the **settings per repo** (owner/name, base branch, branch pattern, PR language). It is
optional. A repo without an entry uses its default branch from GitHub as the base branch and the
usual branch pattern (or the proposal from its `CONTRIBUTING.md`). The first time the agent starts in such a repo,
Cockpit stores an entry with these values (`watch = false`) so you can edit it later. The branch pattern and path
checks before the push stay as they are. `watch` (default `true`) says whether the sync also loads *all* open pull
requests of that repo, as if it were in `[github] repos`; entries written by hand watch, entries stored on the first
use do not. Older configs keep working unchanged: `[github] repos` and `[[agent.repos]]` entries are watched repos as
before, and `[agent] repo` still preselects the repo. Jira projects are **no longer tied to repos**: the setup does not ask for them,
and an old `projects = [...]` list in `[[agent.repos]]` is read without error (an invalid key is skipped, a key may appear
in several repos) and only serves as a quiet suggestion in the start dialog; the assistant keeps it when it saves the list.
Which pull requests the Reviews tab shows is a filter in the app (see [using-cockpit.md](using-cockpit.md#reviews-filter)), not configuration. In the assistant the connection test looks at each watched repo
read-only (`GET /repos/{owner}/{name}`: exists, visible, push allowed) – it changes nothing. Pull requests are
identified by `owner/name#number`: equal numbers from two repos stay separate in the inbox, review, agent view,
log and notifications; addresses look like `#/review/acme/app/87`.

## Language of pull requests

`pr_language` of a `[[agent.repos]]` entry (`"en"` or `"de"`, default `"en"`) is the team language of that repo: the commit
message the runner creates and the title and text of the pull request Cockpit builds are written in it. It is a setting of its
own and does not depend on the language of the interface. The setup assistant has a switch for it in every repo row.
The agent's own texts for you (plan, summary, questions, review comments) follow the language of the interface instead.

## Branch names

`branch_pattern` of a `[[agent.repos]]` entry says how the agent names its branches in that repo. Without it the
branch is `claude/{ticket}-{slug}`, as before.

| Placeholder | Becomes | Example |
| --- | --- | --- |
| `{ticket}` | the Jira key (**required**) | `BAU-142` |
| `{slug}` | the ticket title as a short name (at most 24 characters) | `toggle-fokus-ring-fehlt` |
| `{type}` | `feature`, `fix` or `chore`, from the Jira issue type (Bug/Fehler → `fix`; Task/Sub-task/Aufgabe → `chore`; everything else, also an unknown type, → `feature`; without an issue type the ticket's labels decide) | `fix` |

A pattern such as `feature/{ticket}-{slug}` gives `feature/BAU-142-toggle-fokus-ring-fehlt`. A pattern is refused
(when the file is read and when the assistant saves it) if it is empty, has no `{ticket}` (so it would point at a
fixed branch or let two tickets share one), contains an unknown placeholder, glues two placeholders together, or has
characters other than letters, digits and `/ - _ .`. An empty value counts as not set.

In the assistant (step "GitHub", the list of repos) the button "Aus Richtlinien vorschlagen" reads `CONTRIBUTING.md`,
`.github/CONTRIBUTING.md` and `docs/CONTRIBUTING.md` of the repo (read-only, `GET /repos/{owner}/{name}/contents/{path}`)
and recognizes usual names (`feature/`, `bugfix/`, `PROJ-123`). It only **proposes**: you confirm with "Übernehmen" and
save the list yourself. Without a finding you get a hint. If the clone has such a file, the runner also tells the agent
to follow the commit conventions in it.

The pattern is enforced by the backend, see [security.md](security.md#the-agent).

## GitHub Projects as the source of tickets

```toml
[github_projects]
tickets_from = "github_projects"     # "jira", "github_projects" or "both" (default "both" when the section exists)
projects = ["acme/3", "mkeller/1"]   # owner/number, as in the project's address
status_field = "Status"              # optional: the single-select field that is the ticket status (default "Status")
team_field = "Team"                  # optional: the field that holds the team (default "Team")
```

Without the section Jira is the only source of tickets, as before. With `tickets_from = "github_projects"` Jira is not
asked at all (its settings stay in the file). The credential is the GitHub one (token or sign-in), see the
[rights table](setup-guide.md#github-projects-tickets-without-jira). A project that GitHub does not show to the credential
is a hint under "Einrichten" › Übersicht › Abgleich, and its old tickets stay until it can be read again.

## Seed data

The server starts with an empty workspace. `COCKPIT_SEED=demo` (or `seed = "demo"`) loads sample data, **only if no state
has been stored yet**; for a fresh seed use a new database (`COCKPIT_DB=/path/to/new.db`). Sample data otherwise serves as
test data and for the Playground stories. The demo seed contains a sample GitHub project (`acme/3`) with an issue
(`acme/app#31`) and a draft issue.

## Credentials: the `.env` file

The README lists the names for `token set`. All keys (Jira token, GitHub token, key of the agent endpoint, client secrets, sign-in tokens) live in the file
`config/.env`, next to `config.toml` and `cockpit.db`. They stay separate from `config.toml` on purpose. The risk of a
file inside the repository folder was a deliberate choice. **It is never committed**: `.gitignore` excludes
`config/*` and `.env` / `.env.*` anywhere; no `.env*` file belongs in the repo.

- **Create:** the assistant and `cockpit-server token set <name>` create the file when needed (mode `600`). By
  hand: create `config/.env` with the variables from the table below.
- **Location:** `config/.env` in the server's working directory (the repo folder; `scripts/start.sh` and
  `start-cockpit.command` change into it). `COCKPIT_ENV_FILE=/path/to/file` redirects it (operation, tests). The
  assistant shows the path in the last step.
- **Move from the old place:** older versions kept the file at `<project>/.env`. If `config/.env` is missing and
  the old file exists, the server moves it (rename, mode `600` stays) at the start and logs it. If both exist, it
  touches neither, uses `config/.env` and shows a notice (`config_files.notices`, last assistant step) that the old
  file can be deleted.
- **Format:** `NAME=value`, comments with `#`, other lines are preserved on write. Values with special
  characters go in single quotes. The sign-in token blobs are compact one-line JSON (`'{"access":"…"}'`). Writing is
  atomic (temporary file, then rename).
- **Precedence:** a real environment variable of the same name wins over the file. A value set that way cannot be
  overwritten or deleted in the assistant.
- **Variables:**

  | Name in the backend | Variable in `config/.env` |
  |---|---|
  | `jira` | `JIRA_TOKEN` |
  | `github` | `GITHUB_TOKEN` |
  | `agent-key` (older: `litellm`) | `AGENT_KEY` |
  | `github-oauth` | `GITHUB_OAUTH_CLIENT_SECRET` |
  | `atlassian-oauth` | `ATLASSIAN_OAUTH_CLIENT_SECRET` |
  | tokens of the GitHub sign-in (written by the backend) | `GITHUB_OAUTH_TOKENS` |
  | tokens of the Atlassian sign-in (written by the backend) | `ATLASSIAN_OAUTH_TOKENS` |
  | webhook secrets (optional) | `GITHUB_WEBHOOK_SECRET`, `JIRA_WEBHOOK_SECRET` |

- **Safety net:** at start and in `/api/health` (`env_file`, visible in the last assistant step) the server warns
  when Git tracks `config/.env`, `.gitignore` does not cover it, or the permissions are looser than `600` (at start they
  are set to `600`). CI fails when any `.env*` file is tracked. If a real `.env` was ever
  committed: rotate the keys immediately and run `git rm --cached config/.env`.
- **Container:** the `config/.env` never enters the image or container (`apps/agent/.dockerignore`); the agent receives
  values only as `-e NAME` from the backend.

Terminal commands (the value always comes from standard input, never from the command line):

```bash
echo "$TOKEN" | cargo run -p cockpit-server -- token set github     # or jira, github-oauth, atlassian-oauth, agent-key
cargo run -p cockpit-server -- token delete github                  # remove a key
cargo run -p cockpit-server -- token import                         # copy old keychain entries into config/.env, once
```

`token import` reads the system keychain (where older versions kept keys), deletes nothing there, overwrites
nothing that already has a value and never runs by itself.

## Model provider and endpoint

`[agent] provider` picks the tool in the container; the backend passes it as
`COCKPIT_PROVIDER` and the runner starts the matching adapter:

| `provider` | Tool in the container | Endpoint family | Names passed with `-e` (values only in the process environment) |
|------------|-----------------------|-----------------|------------------------------------------------------------------|
| `claude-agent-sdk` (default) | Claude Agent SDK | Anthropic-compatible | `ANTHROPIC_BASE_URL` + `ANTHROPIC_AUTH_TOKEN` (with `url`) or `ANTHROPIC_API_KEY` (direct), `ANTHROPIC_MODEL` |
| `claude-code` | Claude Code CLI (`claude -p --output-format stream-json`) | Anthropic-compatible | same as above |
| `codex` | Codex CLI (`codex exec --json`, sandbox `workspace-write` without network, approval `never`) | OpenAI-compatible | `OPENAI_BASE_URL` (with `url`; `/v1` is added), `OPENAI_API_KEY`, `COCKPIT_MODEL` |

All three run under the same rules (see [security.md](security.md#the-agent)); the CLI adapters cannot ask you questions or take hints
while running (hints reach the agent with the next run). `[agent.endpoint]` says where model requests go: with `url`
to a compatible endpoint (LiteLLM, another proxy), without `url` directly to the provider of the tool (Anthropic, for
`codex` OpenAI); `model` is the name as the endpoint offers it. "Modelle laden" calls
`GET /api/agent/models`; the backend then queries `GET <url>/v1/models` (proxy: `Authorization: Bearer`; direct at
Anthropic: `https://api.anthropic.com/v1/models` with `x-api-key` and `anthropic-version`; direct at OpenAI, for
`codex`: `https://api.openai.com/v1/models` with `Authorization: Bearer`) without following redirects, with a
timeout and a size limit, and returns only the IDs. "Verbindung testen" calls `GET /api/agent/check` and reports
*reachable*, *key accepted* and *model present*, with a hint on errors. Without a key it does not call the endpoint.
`/api/health` reports under `endpoint` provider, address, model and whether a key exists (never the key); the setup
summary names the chosen tool. An endpoint that offers no model list: enter the model name by hand.

## Container runtime

The backend chooses: `COCKPIT_CONTAINER`, else `[agent] container`, else it looks for
`podman`, then `docker` in the `PATH` and checks with `<runtime> info` whether the engine runs (a process call, never
the socket; the result is cached for 10 seconds). `/api/health` reports under `container` `runtime`, `engine_running`,
`source` (`env`, `config`, `detected`, `none`) and a `hint`.

## Start at login (launchd, macOS)

`scripts/de.cockpit.server.plist` is a template for a LaunchAgent. **Nothing is installed automatically.**

1. Run `scripts/start.sh` once by hand so that everything is built.
2. Copy the template to `~/Library/LaunchAgents/de.cockpit.server.plist` and replace the placeholders
   `/PATH/TO/REPO` (the repo folder) and `/Users/YOUR_USER` (your home directory); see the comment in the file.
3. Load: `launchctl bootstrap gui/$(id -u) ~/Library/LaunchAgents/de.cockpit.server.plist`,
   unload: `launchctl bootout gui/$(id -u) ~/Library/LaunchAgents/de.cockpit.server.plist`.
   The log is at `~/Library/Logs/cockpit-server.log`.
