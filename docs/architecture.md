# Cockpit architecture

For everyone who works on Cockpit, people and agents alike: the basic rules, how the pieces fit, the container rules,
the HTTP API, what is synced and what goes outward, how the agent behaves, the layout, and how to develop (last
section).

## Basics

| Topic | Rule |
|----------|----------|
| Operation | **Local, for one person.** One service on your own computer. No hosting, no tenants. |
| Backend | Rust crate `apps/server/` (axum, tokio). It also serves the built web app. |
| Data | SQLite in one file, `config/cockpit.db` in the project folder. |
| Keys | Only in the backend, in the file `config/.env` (never committed). **Never in the repo, the log, the database or browser storage.** Details: [security.md](security.md). |
| Agent | Claude Agent SDK, Claude Code or Codex in a container per run, **with Docker (Rancher Desktop) or Podman**. The container gets only the cloned repo, never a GitHub token. |
| Freshness | Polling every 2 minutes by default (`interval_s`). Webhooks are an optional accelerator, see [setup-guide.md](setup-guide.md#live-updates-and-webhooks). |
| Sample data | `demo::workspace()` is static data: test data, the data of the Playground stories and the seed `COCKPIT_SEED=demo` (only on explicit request and only when no state is stored yet). It brings no agent of its own: with it the agent shows its normal "not set up" state until the agent is set up. The server starts empty. |
| Agent provider and endpoint | Selectable: tool `[agent] provider` (Claude Agent SDK, Claude Code CLI or Codex CLI, selected in the runner by `COCKPIT_PROVIDER`), endpoint `[agent.endpoint] url/model` (a proxy such as LiteLLM, another compatible endpoint, or without an address directly at Anthropic). LiteLLM is only one possible endpoint. With an address the container gets `ANTHROPIC_BASE_URL` + `ANTHROPIC_AUTH_TOKEN`, without one `ANTHROPIC_API_KEY` (Codex: `OPENAI_BASE_URL` + `OPENAI_API_KEY`, model in `COCKPIT_MODEL`); the model name is a setting chosen in the assistant from the endpoint's list. The key lives only in `config/.env` (`token set agent-key`). |
| Sign-in | Token or OAuth, chosen per service (`[github] auth`, `[jira] auth`, see [configuration.md](configuration.md#choosing-the-sign-in-method-auth)). API token is the default; OAuth is the alternative. |
| Setup | The setup assistant targets people without programming knowledge: everything that can be done locally by click is done by click (`PUT /api/config`, `PUT /api/secrets/{name}`, double-click start), the manual steps (creating your own OAuth apps, tokens) are guided and documented. Every person creates their **own** OAuth apps because client secrets cannot be shared. |
| Language | Code, naming and docs are English. The interface is German or English (see [Languages](#languages-i18n)); technical messages (logs, `expect`/`panic`, tests, CLI) are always English and are not translated. Texts that core and server produce for people are structured messages (code and parameters), rendered by the UI in the chosen language; see [Languages](#languages-i18n). A key in `config.toml` that the file types do not have stops the server at start (`deny_unknown_fields`); the message names the key, the section and the file. |

## Overview

```
Browser (apps/cockpit, WASM)            Backend (apps/server)
┌──────────────────────────┐   HTTP    ┌────────────────────────────────┐
│ Pages → store.dispatch   │ ────────▶ │ POST /api/actions              │──▶ GitHub, Jira
│ Workspace (signal)       │ ◀──────── │ GET  /api/workspace (snapshot) │──▶ Agent (container)
│                          │ ◀── SSE ─ │ GET  /api/events (changes)     │◀── polling
└──────────────────────────┘           │ SQLite · keys in config/.env          │
                                        └────────────────────────────────┘
          both use  crates/cockpit-core  (model, rules, Action)
```

Crates and apps:

| Path | Role |
|------|------|
| `crates/cockpit-core` | domain logic without UI and network: model, attention rules, review, agent state machine, usage and limits, rules per repo, sync merge, notifications, `Workspace`, `Action`, JSON shapes (`api`), persistence (`persist`). |
| `crates/integrations` | GitHub and Jira clients (read, write, sync), OAuth, outbound calls; native only; tested against recorded responses. |
| `crates/i18n` | message catalogs (`catalog/<lang>/*.txt`), the typed keys and `Msg`; see [Languages](#languages-i18n). |
| `crates/ui` | component library `feature-ui` with `tokens.css`, `components.css`, `extras.css`. |
| `apps/cockpit` | the Leptos WASM app (main application). |
| `apps/server` | `cockpit-server`: axum routes, SQLite, `config/.env` key store, sync loop, OAuth, container and agent processes (`agent_ops/` runs processes, `agent_run.rs` is pure, `agent_git.rs` clones/checks/pushes). |
| `apps/agent` | image (`Containerfile`) and runner (TypeScript, no build step) of the agent. |
| `apps/playground` | the playground for the component library. |

Principles:

1. **`cockpit-core` is the single source of truth.** Model and rules live there, without UI and network, with tests.
   Browser and backend use the same crate.
2. **Every change is an `Action`.** The UI calls `store.dispatch(action)`; the backend applies it with
   `Workspace::apply`, additionally triggers the effect at GitHub or Jira and writes it to the log with time and author.
   The browser holds nothing of it (no `localStorage`).
3. **Two rules are non-negotiable:** comments and reviews of the agent leave only after your approval. The agent never
   merges and never writes to the main branch.
4. **Every step stays runnable.** After every change the Playground and Cockpit build, and Cockpit runs with the
   server, from an empty start and with `COCKPIT_SEED=demo`.

Code layout (Rust): tests never stand inline. `foo.rs` ends with `#[cfg(test)] mod tests;` and the tests live in
`foo/tests.rs` (child module, so it sees private items); a big test file is split by topic into `foo/tests/<topic>.rs`
with shared helpers in `foo/tests.rs`. Logic files stay below roughly 500 lines: `foo.rs` keeps the core types and
re-exports (`pub use`) the topic modules in `foo/<topic>.rs`, so public paths do not change. Integration tests of a
crate with shared helpers use `tests/<name>/main.rs` plus topic files.

Rules that are easy to break: the periodic tasks (expired snoozes, auto-link, ticket status) run only in the server
(`apps/server/src/live.rs`, `tick`); the browser runs a clock for the attention rules (`store.now`,
`apps/cockpit/src/main.rs`) and nothing else. Look runs up through `run.key()`, not `ticket_key` (follow-up jobs have
their own keys). New local fields must not be overwritten by the sync (`crates/cockpit-core/src/sync.rs`, `merge_item`).

## Container rules

The agent container must run with **Docker** (for example from Rancher Desktop) **and** with **Podman**. Rules for
anything that builds or starts containers:

1. **Command line only.** The backend calls `docker` or `podman` as a process, never the Docker socket or a Docker
   library. Only commands and switches both understand the same way are used (`build -f`, `run --rm`, `-e`, `-v`,
   `--network`, `--add-host`, `--user`, `--label`, `create`, `start -a`, `cp`, `ps`, `rm -f`, `image inspect`).
2. **Runtime selectable.** Configuration `[agent] container = "docker" | "podman"`, overridable by `COCKPIT_CONTAINER`.
   Without either, the backend looks for `docker` first, then `podman` in the `PATH` and checks with `<cli> info` whether
   the engine runs. `/api/health` reports which one was found.
3. **Name images fully.** `docker.io/library/node:…` instead of `node:…`; Podman does not resolve short names, or only
   after asking. The build file is named `Containerfile`; both build it with `-f Containerfile`. No BuildKit features
   (`RUN --mount`, `# syntax=`).
4. **Reach the host.** A proxy (such as LiteLLM) on your own machine is reached from the container with
   `--add-host cockpit-host:host-gateway` (Docker 20.10+, Podman 5.x) and the address `http://cockpit-host:<port>`; not
   via `host.docker.internal` or `host.containers.internal`, which exist reliably for one runtime each.
5. **File permissions.** Natively the repo is mounted (`-v <clone>:/work`) without SELinux suffixes (`:Z`). The process
   runs as a non-root user (`USER` in the `Containerfile`). Podman (rootless) and Docker map users differently, so after
   the run only read what lies in the clone. When Cockpit itself runs in a container (`COCKPIT_IN_CONTAINER`, or
   `COCKPIT_AGENT_IO=copy`) there are no mounts at all: `create`, `cp` in, `start -a -i`, `cp` out into a fresh folder,
   `rm -f` (`agent_ops/copy_io.rs`, [container.md](container.md#no-paths-must-match-copy-instead-of-bind-mounts)).
6. **Secrets only as `-e NAME`** (value taken from the environment of the child process, not from the command line), so
   they do not appear in `ps` or logs.
7. **Rancher Desktop** must run with the engine "dockerd (moby)"; in "containerd" mode only `nerdctl` exists, which is not
   supported.
8. **Verify with both.** Tests that start real containers are `#[ignore]` and run with
   `COCKPIT_CONTAINER=docker cargo test -- --ignored` or `=podman`. The PR names the runtime it was checked with.
9. **Label per run:** `cockpit.run=<key>` lets the server remove orphaned containers at start (and mark their runs as
   cancelled). A run in several repos is still **one** container with one label, so cleaning up by label covers all its
   repos; the clone folders of the run (one per repo) are removed together with the run folder.
10. **Layout of the clones.** A run in one repo mounts its clone directly as `/work`. A run in several repos
    (`AgentRun::repos`, at most `MAX_REPOS_PER_RUN` = 10) mounts the folder that holds all clones as `/work`; each repo is
    cloned to `/work/<owner>__<name>` (`cockpit_core::repo_dir`) on its own branch off its own base branch. The job file
    lists them in `repos` (folder, branch, base branch, rules, guidelines file, PR language); the system text names them
    all and says which rules apply where. The agent changes only the repos the task needs.

## HTTP API

All routes live under `/api`; see [security.md](security.md) for the request checks.

| Route | Purpose |
|-------|---------|
| `GET /api/health` | `{status, version, connections, sign_in, sync, endpoint, container, agent, env_file, config_files, unreadable_state}`; per connection only yes/no |
| `GET /api/workspace` | state as JSON (shape of `cockpit_core::persist`, plus `revision`) |
| `POST /api/actions` | body `{"action": <Action>}`; applied as the user. Answer `{revision, summary, logged}`; a rejected action is `422` (unknown shape `400`) with `{"error": "…"}`; a failed outward call is `502` |
| `POST /api/sync` | "sync now" with Jira and GitHub. Answer `{active, revision, logged}`; `active: false` when no source is set up |
| `GET /api/events` | server-sent events: first the current revision, then one event per action with the new revision and new log entries, plus keep-alive |
| `GET /api/jira/attachment/{id}`, `GET /api/jira/attachment/{id}/thumbnail` | the bytes of an attachment of a synced ticket, read with the server's own key; rules in [security.md](security.md#jira-attachments-get-apijiraattachmentid) |
| `GET /api/jira/projects` | Jira projects for the filter dialog of the inbox: `{active, projects: [{key, name}]}` |
| `GET /api/jira/assignable?project=&query=` | People Jira lets you assign tickets of the project to (read only; you yourself come as `Person::me()`): `{active, people: [{name, initials}]}`. Serves the search field of the assignee in the ticket detail |
| `POST /api/jira/check` | connection test for Jira with an API token, read only (`GET /rest/api/3/myself`): `{ok, cause, message, name}`; `cause` is `ok`, `details`, `address`, `token`, `permission`, `reachable` or `other`; never the token |
| `GET /api/agent/image`, `POST /api/agent/image` | state of "build image" `{running, present, error, lines}`, or start the build in the background |
| `GET /api/agent/check` | connection test to the endpoint: `{configured, reachable, key_ok, model_present, hint}` |
| `GET /api/agent/pull-request?run=<key>` | preview of the pull request for a finished run: `{repo, head, base, title, body, draft, existing}`; creates nothing |
| `GET /api/agent/cleanup`, `POST /api/agent/cleanup` | "Clean up": preview `{containers: [{id, name}], folders, engine_missing}` (removes nothing), then `{containers: [id], folders: [name]}` removes what is confirmed and still an orphan → `{containers, folders}` |
| `GET /api/agent/branch?run=<key>`, `POST /api/agent/branch` | "Delete branch": `{branch, allowed, reason}` (reads only), then `{run}` deletes that one branch on GitHub after the same check → `{branch}` |
| `GET /api/review/commits?repo=&number=` | commits of a synced pull request `{commits: [{sha, title, body, author, date}], truncated}`; read only (`GET pulls/{n}/commits`) |
| `GET /api/review/commit?repo=&number=&sha=[&path=]` | diff of one commit `{sha, files: [{name, status, additions, deletions, lines, total_lines, truncated, no_patch}], truncated_files}`; every file cut at 300 lines, with `path` that one file up to 5000 lines; read only (`GET commits/{sha}`); only for pull requests the app knows |
| `GET /api/agent/check/container` | the same test from a container: `{checked, mac_reached, container_reached, key_ok, hint}` |
| `GET /api/agent/models` | model IDs of the endpoint for the drop-down: `{loaded, reachable, key_ok, models, hint}`; only IDs, sorted (Claude models first), at most 200 |
| `GET /api/github/projects` | the GitHub Projects the access can see `{connected, projects: [{id, owner, number, title, url, closed, origin, repos}], rights: {kind, scopes, missing}, problem, owners}`; reads only (GraphQL `viewer`); a refusal is in `problem`, with the exact missing right |
| `POST /api/github/projects/create` | body `{owner, title, repo?}`: creates a project (see below), links it to `repo`, gives it the four status columns; answer `{project}`; only on the click "Create project"; a refusal is `403` with the exact missing right |
| `GET /api/github/assignable?repo=&query=` | people GitHub lets you assign issues of the repo to (`GET /repos/{repo}/assignees`, read only; you come as `Du`): `{active, people}` |
| `GET /api/setup` | what the assistant prefills: Jira site/email/projects, client IDs, repos, provider, endpoint address and model, both callback addresses, and per key only `{name, set}` |
| `GET /api/github/repos` | the repos the credential can see (`GET /user/repos` for a token, the installations for an app; at most 500, kept 5 minutes) |
| `POST /api/github/check` | body `{repos:[…]}`: connection test for GitHub, read only (`GET /user`, `GET /repos/{owner}/{name}` per repo) |
| `GET /api/github/access` | whether the sign-in sees the watched repos: per repo `visible`, `not_visible`, `no_permission` or `unreachable`, plus the installations when something is hidden; read only |
| `POST /api/github/branch-suggestion` | proposal of a branch pattern from the `CONTRIBUTING.md` of a repo; reads only, saves nothing |
| `POST /api/github/repos/check` | read-only look at repos (`{repos:[…]}` → per repo `Ok`/`ReadOnly`/`NotFound`/`NoAccess`/`Failed` plus the default branch); only `GET /repos/{owner}/{name}`, never changes anything |
| `GET /api/agent/repo-suggestions?ticket=KEY` | repos to offer first for the ticket, best first, each with a reason (`issue`, `previous_run`, `linked_pr`, `mentioned`, `default`); only reads |
| `PUT /api/config` | writes client IDs, Jira, repos, `[github_projects]` (`github_projects`: `tickets_from`, `projects`, `status_field`, `team_field`), the agent's repos (`agent_repos`: list with base branch, branch pattern and language, plus default), agent provider and endpoint into `config.toml`; an empty `client_id` (`{"atlassian":{"client_id":""}}` or `{"github_oauth":{"client_id":""}}`) removes the whole OAuth section (including `site`); `{"atlassian_site": "…"}` sets `[atlassian] site` (empty removes it); answer `{stored, takes_effect_immediately, hint}`; invalid input is `422` with a message (`ErrorBody`) |
| `PUT /api/secrets/{name}` | stores a key in `config/.env`; body `{"value": "…"}`; names in [configuration.md](configuration.md#credentials-the-env-file) (plus `github-webhook`, `jira-webhook`); answer `{name, set}`, never the value |
| `DELETE /api/secrets/{name}` | "Disconnect" and "Remove client ID": deletes `jira`, `github`, `github-oauth` or `atlassian-oauth` from `config/.env` (other names: `404`); answer `{name, set: false}` |
| `POST /api/webhooks/github`, `POST /api/webhooks/jira` | optional (`[server] webhooks`), the only routes reachable with a foreign `Host`: HMAC-signed triggers for the sync; `202`/`200`/`204`/`401`/`404`/`413`/`415`/`429`, no body, payload never used as data |
| `POST /api/setup/webhook-secret/{source}` | generates a webhook secret (`github`\|`jira`), stores it in `config/.env`, returns it once: `{secret}` |
| `POST /api/setup/webhook-test/{source}` | signed test request from the server to itself over loopback: `{ok, message}` |
| `GET /api/oauth/{service}/start`, `GET …/callback`, `POST …/revoke` | OAuth sign-in for `github` and `atlassian` |

If the server is unreachable (not started, other port) the app shows a notice at the top with "Try again"; the pages stay
empty and nothing is lost.

**Connection state.** `GET /api/health` reports under `sign_in.github` and `sign_in.jira` `connected`, `kind` (`oauth`,
`token` or `none`), `oauth_possible` (app set up and secret present), `expired` (the sign-in ran out and cannot be renewed)
and, for Atlassian, the `site`. **This is the one place that decides whether a service is connected**
(`ServiceStatus::resolve` in `cockpit-core`, `crates/cockpit-core/src/connections.rs`): a working sign-in goes before the
token, an expired one falls back to the token, a token counts when its key is in `config/.env`. The summary, the
navigation, the inbox, review, the assistant steps and the sync state all read it, and the sentence and colour of a state
come from one function too (`ServiceStatus::summary`).

## Webhooks

`apps/server/src/webhooks.rs`. Two routes outside the `access` layer (router merge), own checks in this order:
switch, method, loopback peer, `application/json`, rate limit, secret present, body size, signature, replay,
event type. A valid relevant event calls `AppState::webhook_trigger(source)`: a `Gate` per source runs one sync at
once (leading edge) and merges further ones within 5 s into a single trailing sync; both call
`sync_sources(&[source])`, the same code as the regular sync (single-flight lock, retries on concurrent writes,
rate-limit pause), with the other source treated as paused. The sync tick uses `sources_due()`: a source whose
webhooks arrive regularly (`poll_due`, a pure function of elapsed times) is only asked every
`webhook_fallback_interval_s`. State (last received, gates, rate and replay windows) is in memory (`Hub`); the
switch and public address are replaceable at runtime like the other settings. The assistant step "Live" reads
`GET /api/setup` › `webhooks`, writes `[server]` through `PUT /api/config` and uses the two `/api/setup/webhook-*`
routes.

## Sync and outbound actions

**What is synced.** The backend polls both services on the interval (and on `POST /api/sync`: the button "Sync now" in the inbox, in the
banner of a source with a problem and in the Sync card of the last setup step) and merges the result through the action `SyncRemote`
(author "System") into the workspace, as a pure function: `cockpit_core::sync::merge`.

- New entries are added, removed ones disappear. Local state stays: read/done, snoozes, agent runs and hints,
  suggestions and decisions in a review, your own unsent line comments (they move with their line when the diff
  shifts), verdict and "sent".
- **GitHub Projects** (`Source::GitHubProject`, `cockpit_integrations::github::projects`): a source of its own, because
  the sync state, the removal of entries that vanished and the sync problems are per `Source`; the pull request sync
  must never remove an issue and the project sync never a pull request. GraphQL only (`POST /graphql`, for GitHub
  Enterprise Server `https://host/api/graphql`): `viewer` (own projects and organizations) and `repository(…){ projectsV2 }`
  list the projects; `repositoryOwner(login){ … on ProjectV2Owner { projectV2(number) } }` reads one with its fields; the
  items come in pages of 50 with `fieldValues`, assignees, labels and milestone (the *light* pass). Comments, the timeline
  (`CONNECTED_EVENT`, `CROSS_REFERENCED_EVENT`), `closedByPullRequestsReferences`, issue type, parent and sub-issues are the
  *heavy* pass (`nodes(ids:)`, 20 per call), asked only for issues that are new or whose `updatedAt` (of the item or the
  issue) moved; the others keep what the workspace has. A server without `issueType`/`subIssues` answers
  `undefinedField`; the pass is repeated without them and a hint says so. Every answer carries `rateLimit { cost remaining
  resetAt }`; below 15 points the client stops before the next call (`Error::RateLimit` with the reset time → the source
  pauses and keeps its data). A project that cannot be read this time keeps its old tickets. Refusals become
  `Error::MissingRight { right, kind }` (`cockpit_core::permissions`): `INSUFFICIENT_SCOPES` names the scope, `FORBIDDEN`
  / "Resource not accessible" the right the failed call needed (below `content` it is *Issues*, otherwise *Projects*);
  classic scopes are read from `X-OAuth-Scopes`. The key of a ticket is `owner/repo#123` (`cockpit_core::project`), the item
  keeps `ProjectItem` (node IDs for writing, field values, linked pull requests, parent, sub-issues); the project fields are
  in `SyncState::projects`. Auto-link, the "Development" section and the team filter share one rule for what a
  pull request names (`cockpit_core::link::PrTexts::ticket_keys`, described under *Status follows the pull request*):
  `#123`, `owner/repo#123` and `Fixes #123`; the timeline connects pull requests that never name the issue.
- **Writing to GitHub Projects** (`outbound::plan_project`, `github::projects::write`): each action on a ticket of the source
  becomes one call, and only after the rules of the workspace accepted it (as on a copy): `SetStatus` → the option of the
  matching column (`option_for`) through `updateProjectV2ItemFieldValue`; `SetPriority` → the option of the field `Priority`;
  `SetProjectField` → the same mutation with `singleSelectOptionId`, `iterationId`, `text`, `number` or `date`, or
  `clearProjectV2ItemFieldValue`; `EditTicket` → `PATCH /repos/{repo}/issues/{n}` (a draft: `updateProjectV2DraftIssue`);
  `AssignTicket` → `PATCH …/issues/{n}` with `assignees` (me, a login, or none); `SetLabel` → `POST …/labels` or `DELETE
  …/labels/{name}`; `Reply` → `POST …/comments`; `SetIssueClosed` → `PATCH` with `state`. A refusal (REST 403, `FORBIDDEN`)
  is `Error::MissingRight`: *Issues* for REST, *Projects* for GraphQL. Two actions need several calls and have their own
  course in `AppState::project_issue_flow` (the answer of GitHub, never the browser's `created`, is what gets booked):
  `CreateIssue` (`POST /repos/{repo}/issues`, `addProjectV2ItemById`, then the status) and `ConvertDraft`
  (`convertProjectV2DraftIssueItemToIssue`, with the repo's node ID). A ticket from a project is not changed locally when
  GitHub Projects is not connected (the next sync would undo it); tickets of the sample data stay local. "Status follows PR"
  is the same local tick as for Jira (`SyncTicketStatus`). **Create a project** (`POST /api/github/projects/create`,
  `GitHubClient::create_project`): `createProjectV2` for the owner's node ID, `linkProjectV2ToRepository`, then the `Status`
  field is created or its choices are replaced by Todo, In Progress, In Review and Done (only for the project that was just
  created). No query in the code deletes a project, an item or an issue (a test checks the requests).
- New comments and status, check and review changes from outside appear in the log with author "System" and trigger the
  notifications. `@your-login` in comments counts as a mention.
- GitHub: no repo list is needed. Three searches (`cockpit_core::sync::discovery::QUERIES`: `author:@me`,
  `review-requested:@me`, `involves:@me`, 30 hits each) find the pull requests across every repo the credential can see;
  `merge_hits` deduplicates and applies the limits, and each hit is loaded with `GET /repos/{owner}/{name}/pulls/{n}`. A
  failed search is a hint and keeps what was known. Per watched repo (`[github] repos`) `GET /repos/{owner}/{name}/pulls?state=open`
  loads all open pull requests (100 per page, newest update first; limits
  `[sync] max_prs_per_repo`/`max_prs_total` for both). `GET /user`
  runs once per sync. The relation to you comes from the pull request (`user`, `requested_reviewers`, `assignees`) and is
  stored on `PrRef` (`author`, `mine`, `requested_me`, `assigned_me`; all `#[serde(default)]` and omitted when empty, so
  `wire_compat` snapshots stay byte-identical). Pull requests with a relation get `Scope::Mine` and the full detail
  (comments, verdicts); the others get `Scope::Team`, a reason "Pull Request von …" (no attention, no notification) and
  only checks and diff (the diff is reloaded on a new head commit only). A repo that answers 403/404 is a hint; a repo
  or pull request that fails for another reason (5xx, timeout) keeps its last state and adds a hint; a rate limit or a
  rejected token fails the whole source. Webhooks (above) only trigger this same sync.
  Checks per pull request come from `GitHubClient::checks_load`: check runs first; a 403/404 (a fine-grained token has no
  "Checks" permission) falls back to the combined commit status (`status_to_checks`, no status at all = `Checks::Unknown`),
  and a denial is cached per repo and token for the process (`CHECKS_DENIED`).
- Jira: the default query asks for active sprints (see configuration). `Item.sprint` (`name`, `state`
  `active|closed|future`, `start`, `end`) and `Item.updated_ms` are optional on the wire; the status category already
  gives `TicketStatus`. Entries the sync no longer delivers (sprint ended) are removed by the normal merge; a failed
  source keeps its data.
- Inbox filters are data (`cockpit_core::filters::FILTERS`); `Workspace::inbox` applies ownership, `InboxFilter::matches`
  the rest. Ownership "All" is the only way a quiet entry reaches the inbox.
- Per source there is a state (`/api/health` › `sync`, in the app in the "Sync" card): `ok`, `unauthorized` (401, token
  expired), `no_permission` (403), `quota` (rate limit: the source pauses until the time from
  `Retry-After`/`X-RateLimit-Reset`), `offline`, `error`; plus `off` (not set up or no key) and `waiting` (no fetch
  yet). On an error the last data of the source stays. Each source state also carries `since_ms` (since when the
  problem lasts; unchanged while the same kind of problem repeats, cleared on recovery); `/api/health` › `sync` ›
  `details[]` lists `{source, status, since_ms, retry_at_ms, last_ok_ms}` per fetched source (no keys, no service
  messages). `since_ms` is `#[serde(default)]`.
- A 5xx or a timeout is a source problem (`error` / `offline`), not an error of the sync call: `POST /api/sync` still
  answers 200. A source in `quota` with a known time is not queried at all until then.
- A pull request is identified by `PrKey { repo: "owner/name", number }` (`cockpit_core::PrKey`, text form
  `owner/name#12`). `Review`, `PrRef`, `PrLink`, the actions (`review`, `pr`), `Target::Review`, follow-up runs
  (`review-acme~app-12`, `check-acme~app-12-lint`) and notification routes carry it, so equal numbers from two repos
  never mix. On the wire a key without repo is the plain number; both forms are read, and an empty repo is not
  written, so stored snapshots and clients stay byte-compatible (`wire_compat`). A key without repo matches any repo
  with that number; a key with repo matches only that repo or an entry whose repo is still unknown.
- **Ticket → repo.** The repo is chosen in the start dialog (`AgentOptions.repo`, any `owner/name`; the list comes from
  `GET /api/github/repos`, cached 5 minutes). `GET /api/agent/repo-suggestions?ticket=` ranks suggestions with
  `cockpit_core::repos::suggest` (earlier runs of the same Jira project, linked pull requests, mentions in the ticket
  text, the default repo). The first run in a repo without settings stores them in `[[agent.repos]]` (`watch = false`,
  default branch from GitHub, `CONTRIBUTING.md` proposal; `AppState::repo_settings_prepare`).
  `cockpit_core::AgentRepos` (`[[agent.repos]]`: repo, base branch, branch pattern, plus the default `[agent] repo`, or
  the only repo of the list) decides a start without a choice: the default repo, otherwise the user has to choose (the
  Jira project of a ticket plays no part). The backend applies it in `AppState::agent_route` before `StartAgent` (the
  user's `AgentOptions.repo` wins) and `GET /api/health` reports the same repos and default under `agent.repos`, so the
  interface preselects identically. A run stores its repo (`AgentRun.repo`), clones and pushes only there, and checks the
  repo name again at start. `CreateBranch` for a Jira ticket uses the default repo too; without one the plan fails with
  a message that asks for a default repo (the button offers no choice).

**What goes outward.** In server mode these actions go to the service first; **only when it confirms is the action
applied locally**. If the call fails, a message (status 502) comes back and nothing changes. The rules of the
workspace (also "review only after deciding on every suggestion") apply before every call. Every action that can become
a call needs `Actor::User`, merging included: `Action::required_actor` names the one actor an action needs, and
`Workspace::apply` refuses the agent and the system before anything else happens.

| Action | Call |
|--------|------|
| `Reply` | Jira: comment (`POST /rest/api/3/issue/{key}/comment`, ADF) · GitHub: `POST /repos/{repo}/issues/{n}/comments` |
| `SetStatus` | Jira: look for the transition that leads to the target status (`GET …/transitions`), then `POST …/transitions` |
| `CreateBranch` | GitHub: ref from the default branch of the default repo (`[agent] repo`, or the only configured repo; without one a message asks for it) (`POST /repos/{repo}/git/refs`) |
| `SendReview` | `POST /repos/{repo}/pulls/{n}/reviews` with verdict, text, approved suggestions and your own line comments as position comments (including `suggestion` blocks), bound to the commit seen |
| `MergePr` | `PUT /repos/{repo}/pulls/{n}/merge` with method and the commit seen (a new push makes GitHub refuse) |
| `RestartCheck` | `POST /repos/{repo}/check-runs/{id}/rerequest` |

All other actions, and every entry that came from a seed, stay local. The action `SyncRemote` may only be triggered by
the backend; `POST /api/actions` rejects it.

**Formatted Jira text.** Jira (REST v3) delivers descriptions and comments as ADF. The sync does not flatten it: `cockpit_core::adf::parse`
turns it into the typed model `adf::Doc` (paragraphs, headings 1–6, nested lists, code blocks with language, quotes, rules, tables, panels,
media placeholders; marks strong/em/code/strike/underline/link; mention, emoji, inline card, status, date). It is stored in
`Item.description_rich` and `Comment.rich` (`None` for pull requests, edited descriptions and states stored without it, which fall back to the plain text
in `description`/`text`; that text is always filled for search and the agent). Unknown nodes become the text of their content, never an
error. Links are kept only for `http`/`https`. The UI (`feature_ui::RichText`) builds real elements, never `inner_html`; links get
`target="_blank"` and `rel="noopener noreferrer"`; media show their file name only. No `expand=renderedFields` and no HTML from Jira.

## Empty, loading and error states

The browser decides what to show from three things it already has: the link to the server (`Link`: connecting,
online, offline, plus `navigator.onLine`), whether the first workspace has arrived (`Store::loaded`), and
`workspace.sync.sources` (per source: problem, `since_ms`, last success).

- `apps/cockpit/src/gate.rs`: before the first state arrives, placeholder rows or "server not reachable" with retry.
- `store.rs`: when the event stream or a fetch fails the store marks the link offline and retries with a back-off
  (`cockpit_core::notice::reconnect_delay_s`); the browser's `online` event retries at once. Nothing is lost: the
  last workspace stays on screen.
- `apps/cockpit/src/banner.rs`: link banners and one banner per source with a problem, built by
  `cockpit_core::notice::source_notices` (pure, tested). The cause decides the action: token rejected or no permission →
  "Setup", rate limit with time → wait (says when), otherwise "Sync now".
- `cockpit_core::notice::http_failure_msg` turns a failed request to the local backend (by HTTP status plus the
  backend's own message) into the notice message; pages never build error strings themselves.
- UI blocks: `EmptyState`, `ErrorState` (`role="alert"`), `StatusBanner` (`alert` for errors, `status` otherwise) and
  `Skeleton` (`aria-busy`) in `crates/ui/src/state.rs`, styled in `extras.css` with design tokens only.

## Agent behaviour

**The agent.** Every run is a **run in a container** (see the [setup guide](setup-guide.md#agent-container-image-model)). The run is `apps/server/src/agent_run.rs` (pure), `agent_ops.rs` and `agent_ops/` (`run`,
`watcher`, `image_build`, `container_check`, `lines`) for processes, `agent_git.rs` for clone/check/push; the core side
is `AgentReport` / `Action::ReportAgent`: the container reports and `AgentRun::report` changes the run, nothing else does.
The runner protocol is described in [../apps/agent/README.md](../apps/agent/README.md). Until the agent is set up (container, model, key,
GitHub access) `/api/health` lists what is missing and no run starts.

**Questions and steering.** The agent can stop at a step and ask a question (status "Waiting for answer", with
suggested answers to click or free text). The question counts as attention: it appears in the overview under "Stuck"
(button "Answer question") and in the navigation under "Agents". While the agent runs you can send a hint under "Steer";
the container confirms when it has it. "Pause" stops the run, "Resume" goes on. Everything said appears
in the history next to the timeline. The actions are `AnswerAgent`, `SteerAgent`, `PauseAgent`, `ResumeAgent`.

**Plan before start.** The agent presents a plan before the start and does nothing until you approve it (you may reject
it or ask again with a remark). A review is only sent once every suggestion has been decided on (your own line
comments need no approval). The agent never merges; its result is a pull request waiting for your review.

**Follow-up jobs on a pull request** (review page, right column; `RunKind::Followup`, actions `AddressReviewThreads` and
`FixFailedChecks`, only you may trigger them). At click time and again at run start the backend reads the inputs from GitHub with the user's
token (`GitHubClient::review_input` / `failed_checks`, read only; shortened by `github::followup`) and writes them into
`job.json` under `followup` (secrets of the agent key and GitHub token are redacted from it). The watcher starts a
container like for a normal run; the clone is the existing branch of the pull request (it has to fit the repo's `branch_pattern` or the default one)
(`agent_git::clone_branch`), the check and push are the same as always (fast-forward only, path rules, never a
force-push). The container never gets the GitHub token. The image has the fixed name `localhost/cockpit-agent:local`; whether it is current is detected by the label `cockpit.fingerprint` (SHA-256 over Containerfile, runner sources, the lockfiles of runner and CLIs, and the `CONTRACT` constant; details in `apps/agent/README.md`), and a run only starts with a current image. The image contains the Claude Code and Codex CLIs, pinned in `apps/agent/clis/package-lock.json`; the job carries
`followup` and `repo` (the `owner/name` the backend cloned); follow-up jobs and the pull request always use the repo of the
pull request / run, never "the" configured repo.

**Agent review of a pull request** (`RunKind::ReviewPr`, action `StartPrReview`, only you). The run key is `prreview-…`. The job
(`mode: "review"`) carries title, description and the diff from the workspace (`agent_run::review_input`, limited,
secrets redacted); the clone is the PR's branch (`agent_git::clone_for_review`, any safe branch name, token only during
the clone) and is mounted `:ro` like in plan mode (`RunCommand::clone_only_read`). The runner emits `review_comment` /
`review_summary` events (`agent_run::Event`, limited again), which become `AgentReport::ReviewComment` /
`ReviewSummary`. `Workspace::report_agent` books them on the review: `Review::add_agent_comment` accepts a draft only if
file, line and side exist in the diff, and replaces nothing but adds a `Suggestion` with a structured
`path`/`line`/`start_line`/`side`/`code` (all with serde defaults) and an id `a1`, `a2` …; the summary goes
to `Review::agent_summary`. A new run replaces the old suggestions (`begin_agent_review`). The drafts are decided with
`Decide` / `EditSuggestion`; `outbound::review_data` turns the approved ones into line
comments (range via `start_line`, `suggestion` block from `code`) of **one** `POST /repos/{repo}/pulls/{n}/reviews`, event
`COMMENT` unless you choose otherwise. Nothing is sent before `SendReview`, and not while a draft is undecided. The run
never pushes (no publish step; `finish_review_run` only marks the review as looked through).

**One run in several repos.** The start options carry `repos` (the user's choice, first = primary); the backend replaces
`targets` (base branch, pattern and PR language per repo, from the configuration, never from the request) and
`AgentRun::repos` keeps, per repo (`RunRepo`), its settings, rules, pushed files, `RunRepoState`
(`Pending` / `Unchanged` / `Pushed` / `PrCreated` / `Refused`), the refusal reason and its pull request. `AgentRun::repo`,
`rules` and `pull_request` describe the primary repo, and `AgentRun::repo_list` gives the list (a run without `repos` is a
one-entry list). Each repo's branch is named by that repo's own pattern from the same ticket
and title (`AgentRun::branch_in`). After the container ends, `publish_repos` (`agent_ops/run.rs`) treats every repo on its
own with the checks of `agent_git::publish_changes`: branch name against the repo's pattern, history built on the clone's
start, path rules, new branch, then push. A repo without changes gets nothing (`Unchanged`), a repo that fails a check is
`Refused` with its reason, the others are pushed anyway; only if nothing was pushed and something was refused (or nothing
changed at all) does the run fail (`cockpit_core::judge`). The result is booked as one `AgentReport::Repos` before `Done`.
Pull requests stay behind your click, **per repo**: `Action::CreatePullRequest` has a `repo`, `GET /api/agent/pull-request`
takes `?repo=`, and the text of a pull request lists the sibling repos of the run (their pull request if it exists, otherwise
the branch). Branch deletion (`/api/agent/branch`, `repo` in query and body) and the auto-link by ticket key work per repo
pull request; a run counts as "done" when its first pull request is merged or closed. Never pushed: the base branch;
never done: force-push, merge.

**Pull request of a finished run** (`Action::CreatePullRequest`, only you). The run is `Done` only after the
backend pushed the branch. `GET /api/agent/pull-request` returns the preview (`Workspace::pull_request_draft`, plus the
open pull request GitHub already has for the branch). The action is checked on a copy first (`check_pull_request`), then
`agent_followup.rs` looks for an open pull request for the branch, creates one if there is none
(`POST /repos/{repo}/pulls`, with the edited title and text) and books GitHub's answer on the run
(`AgentRun::pull_request`); a `pull` value sent by the browser is ignored. A 422 "already exists" after a race is
resolved by linking the existing one. A sync follows so the pull request appears as a review.

**Costs and limits.** Every step has a usage (input/output tokens, cost in whole euro cents, duration) shown at the
timeline, with the run total plus "today" and "this month" below. Prices are a setting; the runner reports its
values through `AgentRun::record_usage`. If a run reaches a limit (budget per run, monthly budget, time limit) it stops
(`RunState::Stopped(StopReason)`): the agent page offers "Raise by X and resume" (`ResumeStopped`) or "Cancel",
the run shows under "Stuck" in the overview and a notification "Agent stopped" fires. A new run above the parallel-run
limit **does not start** (`ActionError::ParallelLimit`); there is deliberately no queue. Day and month
count in UTC; cancelled runs stay in the ledger.

**Rules per repo.** Per repo (key: name) allowed and forbidden paths (glob patterns, `cockpit_core::rules::glob_match`),
required tests, instructions and a plan requirement apply. Plan and steps show the rules that apply. A plan that touches
forbidden paths cannot be approved (`ActionError::PlanViolatesRules`, with a reason); such a run always waits with a plan
first. Required tests cannot be switched off. Changed rules apply to new runs and waiting plans. The actions are
`SetAgentLimits` and `SetRepoRules`. The defaults, until rules are set: branches, pull requests and tests allowed;
comments and reviews only after your approval; the agent never merges.

**Status follows the pull request.** A ticket's status follows its linked pull request (draft/open → In progress, review
requested → Review, merged → Done); switchable in the inbox's filter dialog, author in the log: System. Ticket and pull request are
linked automatically; there is no manual linking. One rule says which tickets a pull request names
(`cockpit_core::link::PrTexts::ticket_keys`): a Jira key (`BAU-142`, any case, as a whole word) or a GitHub issue (`#123`
in the pull request's own repo, `owner/repo#123`, `Fixes #123`, the address of the issue at github.com) in its branch, title
or description. In a web address only a whole path segment or query value counts (`…/browse/BAU-142`), not a piece of a slug
(`/blog/bau-142-story`). The auto-link (`Workspace::auto_links`), the "Development" section (`Workspace::linked_reviews`,
`Review::belongs_to`) and the team filter (`Workspace::team_visible`) all ask it and add only their own explicit links:
the ticket the agent opened the pull request for, the pull request a ticket carries, and the pull requests on the timeline
of an issue. The auto-link also takes only a ticket that has no pull request yet.

**Review mechanics.** For a line comment, "Suggest a change" lets you enter replacement code for the line
(before/after diff, sent as a `suggestion` block). The checks of the pull request appear on the right with their log;
red ones can be restarted. "Merge" (merge/squash/rebase) works only with green checks, a sent review and without a
"Request changes" verdict, and only for you, never for the agent.

## Persistence

Everything the app shows lives in the backend (SQLite) as JSON with a version number, see
`crates/cockpit-core/src/persist.rs`. If the version does not match or the data is unreadable, the server first moves the
stored row, unchanged, into the table `workspace_unreadable` (with its revision, the reason and the time), logs a warning,
and only then starts with the seed (empty or sample data). `/api/health` reports it under `unreadable_state` as long as
such rows exist, and the last assistant step shows it; the rows are only ever added, never read back or deleted by the
program. If the row cannot be moved either, the server starts with the seed but saves nothing, so the stored row stays
as it is. To look at a kept row: `sqlite3 <database file> "SELECT id, revision, error, kept_at FROM workspace_unreadable"`. New fields in the types need `#[serde(default)]`, so that stored states keep loading. The
compatibility of stored JSON across renames is guarded by `crates/cockpit-core/tests/wire_compat.rs`
(`wire_*.json` are the current snapshots, `legacy_*.json` stored states in earlier shapes that must keep loading); regenerate the snapshots with
`BLESS=1 cargo test -p cockpit-core --test wire_compat` only on purpose.

## Notifications

Which events notify is derived purely from comparing the workspace before and after an action
(`crates/cockpit-core/src/notify.rs`, with tests). The settings live in the workspace and change through actions
(`SetNotifyChannel`, `SetQuietHours`). Notifications go to the browser only.

## Languages (i18n)

The interface is available in German and English and switches at once, without a reload.

- **Crate `crates/i18n`** (`cockpit-i18n`). Texts live in `catalog/<lang>/<area>.txt` (one line `id = text`, `{name}` for
  parameters, `id.one` / `id.other` for plurals with the parameter `n`, `\n` for a line break). `build.rs` turns the English
  files into the typed enum `Key` (`nav.more` becomes `Key::NavMore`), so a typo is a compile error. No dependency besides `serde`.
- **Use in the code.** `feature_ui::t!(NavMore)` or `t!(NavStuck, n = 3)`; `feature_ui::ago(then_ms, now_ms)` gives "5 minutes ago" /
  "vor 5 Minuten". The language is a signal in `crates/ui/src/i18n.rs`. `App` rebuilds the visible tree when it changes; store, workspace
  and route stay.
- **Which language.** Default is the browser language (`navigator.language`: `de*` is German, everything else English). The choice is
  stored in the workspace (`Workspace::language`, `#[serde(default)]`, action `SetLanguage`), not in `localStorage`. The switcher
  (`apps/cockpit/src/language.rs`) is in the "More" sheet and in the first step of the setup assistant; the playground has one too.
- **Content from Jira and GitHub** (titles, descriptions, comments) is never translated.
- **Tests** (`crates/i18n/src/tests.rs`): both catalogs have the same files, ids and parameters; plural keys have `one` and `other`;
  no key is unused; a scan test (`tests/scan.rs`) fails when `apps/cockpit/src` or `crates/ui/src` contain a visible string literal
  (names like "Cockpit" or "GitHub" are on an allowlist there).
- **German texts** are in simple German: short sentences, no word-for-word translated jargon.
- **Every text for people is a message.** Core, server and integrations produce log entries, errors, hints, notifications and
  setup checks as `cockpit_i18n::Msg` (`msg!(CoreActionStatusSet, key = ticket)`): a code (the catalog id) plus parameters, which
  may be nested messages. A text is one field, never a message with a plain-text copy beside it: API types carry the `Msg`
  (`ErrorBody { error }`, `hint`, `message`, `reason`, `warnings`), and only the place that shows it picks the language
  (`msg.render(lang())`). Stored types do the same (`LogEntry::msg`, `RunMessage::msg`, `Item::agent_note_msg`,
  `Comment::text_msg`); their plain-text fields (`LogEntry::text`, `RunMessage::text`, `Item::agent_note`,
  `SourceState::warnings`) stay readable and are shown where the message is missing or its code is not in the catalog
  (`Msg::render_or`, `LogEntry::text_in`), but new entries leave them empty. A stored field that holds a message and may hold
  a plain text (`AgentRun::ended`, `AgentRun::summary`) reads the text as `Msg::raw` (`cockpit_i18n::msg_or_text`). A code this
  build does not know renders as the code itself. Where a test compares text, it renders explicitly (`render(Lang::De)`).
  Texts from outside (a message of GitHub, git stderr) use `Msg::raw`. An error of Jira or GitHub (`cockpit_integrations::Error`) is passed on as `Error::msg()` (a nested message, never
  `to_string()` wrapped in `Msg::raw`), and a failed fetch is stored as `Problem::Other(Msg)` in the sync state; the `Display` of
  the error is English and only for logs. A scan test (`tests/scan_core.rs`) fails when core, server or integrations contain German
  literals (demo data and tests excepted).
- **Times, people and labels in the data.** The reason line of an entry is a message (`Item::reason_msg`; the kind is
  `Item::reason`). Times are timestamps (`Item::updated_ms`, `Comment::created_ms`), which the UI shows with `ago`, so they age.
  You and nobody are roles of `Person` (`Person::me()`, `Person::nobody()`): the data holds the flag, `name_in(lang)` and
  `initials_in(lang)` give the words. Where a plain name field marks you (reporter, mentions in comment texts) it holds
  `ME_MARK`. Core keeps ids and tones for statuses, priorities and filters; the UI maps them to catalog texts. The plain-text
  fields `reason_text` and `time` are fallbacks that the UI shows only where `reason_msg` and `updated_ms` are missing.
- **Agent texts.** `job.json` carries `language` (interface language: plan, summary, questions, review comments) and
  `pr_language` (team language of the repo: commit message, pull request title and text). The default of `pr_language` is English;
  it is set per repo (`pr_language` in `[[agent.repos]]`, a switch in the setup assistant) and stored in the run options when the run
  starts. The runner (`apps/agent/runner/src/texts.ts`) writes its own step titles, errors and the hint of the check in `language`.

## Layout and navigation

The interface works from 320 px width up to 2560 px and beyond. The CSS lives in `crates/ui/extras.css` (building
blocks) and `apps/cockpit/style.css` (Cockpit specifics), only with existing tokens (the few new ones, `--ui-*` such
as `--ui-pill-*`, `--ui-nav-space`, `--ui-overlay` and `--ui-shadow-float`, are defined in `extras.css`; `tokens.css` is
untouched; fixed sizes of Cockpit are named properties `--ck-*` at the top of `style.css`). Markup carries no `style`
attribute for a static value: it uses the utility classes of `extras.css` (`ui-gap-8`, `ui-justify-between`, …, see
[components.md](components.md#component-library-feature-ui)); a `style` stays only where the value is computed at run
time. The `viewport` meta tag is set and fixed elements respect `env(safe-area-inset-*)`.

**Shell.** From 768 px on the shell is exactly the viewport (`100dvh`, `overflow: hidden`, no page scroll). The columns of
`SplitView` are grid tracks of equal height; each pane scrolls on its own (`min-height: 0`, `overflow-y: auto`,
`overscroll-behavior: contain`). The shell ends above the navigation pill (`--ui-nav-space`), so no scroll area runs
behind it.

| Width | Columns |
| --- | --- |
| above 1280 px | up to three: picker or list `minmax(240px, 320px)` (inbox list 320–420 px), detail `1fr`, context `minmax(300px, 380px)`. Pages with one pane (overview, log, limits, notifications) use one centred column of at most 960 px. |
| 768–1280 px | The context column moves **below the detail** (picker on the left over the full height; detail and context share the right side, 3 : 2). Pages with two columns keep both beside each other. There is no back button. |
| below 768 px | One level at a time, like an iOS navigation stack (see below). The page scrolls. |

Which columns a page has: inbox = list + detail; pull requests = picker + diff + agent review; agent = picker + run +
activity (with no run: picker + empty state); the other pages = one pane. The setup assistant is one scrolling column.

**Navigation pill.** `BottomNav` (`crates/ui/src/shell.rs`) is a floating glass pill on **every** width, mounted once in
the app shell (`main.rs`, `AppNav`) so it survives page changes and its capsule can slide. It is centred, only as wide
as its slots (5 × 68 px up to 1100 px, 5 × 92 px above; never wider than the viewport minus 16 px) and sits
12 px above the bottom edge plus `env(safe-area-inset-bottom)`. There is no bar and no full-width tint. Material: a
translucent surface (72 % of the background token) with `backdrop-filter: blur(22px) saturate(180%)`, a 1 px light
edge, an inner top highlight and a soft outer shadow; the current slot has its own smaller glass capsule that slides
there (`--active`, only without `prefers-reduced-motion`). Real iOS Liquid Glass refracts what is behind it; that is
not possible in CSS, the pill only approximates it. Fallbacks: `prefers-reduced-transparency: reduce` and browsers
without `backdrop-filter` get an opaque pill (`BottomNav solid` shows it on purpose). Text contrast on the pill is at
least 5.5 : 1 (label) over black, white and five saturated backdrops, in light and dark. Hit areas are at least 52 px
high; in low windows (≤ 500 px high) icon and label sit side by side. On touch devices the pill steps aside while a text
field has the focus. `<nav aria-label>`, `aria-current="page"`, focus ring.

**Targets (one definition).** `TARGETS` in `apps/cockpit/src/sidebar.rs` lists all 8 pages once, each with a `Slot`:
four in the pill (Tasks, Reviews, Agents, Overview) and **More**, which opens a bottom sheet with the other targets (Log,
Limits & rules, Notifications, Setup). "Tasks" leads to `#/tasks`, which becomes the filter chosen last (`Route::resolve`,
`last_filter.rs`, default "Me"); its badge counts every unread entry of "Me" and "All" once (`Workspace::tasks_unread`).
Counters are badges; the badge of "More" is the sum behind it, and "More" is current when the current page lives in the
sheet. Big screens have
room for more targets, but all stay reachable through the sheet, so the pill is the same everywhere. The sheet is a
modal dialog (`role="dialog"`, `aria-modal`): focus moves in, Tab stays inside, Escape, a tap on the dimmed area, the
close button, a followed link and `hashchange` close it, focus returns to the button, the page behind does not scroll
(`body.ui-scroll-locked`).

**What else the navigation holds.** The sheet also has the brand "Cockpit" with the badge "Local" and the section
"Connections" (the state of Jira, GitHub and the agent; each leads to the setup). The inbox's attention rules and "Status
follows the pull request" are in the filter dialog of the inbox (button "Filter"). The pull-request list and the agent-run
list are the first column of their pages (`PickerPane`).

- **Phone details:** `viewport-fit=cover`, `dvh` instead of `vh`; notices (toasts) sit above the pill; long button
  labels and addresses wrap; the filter dialog is a sheet from the bottom, its chips wrap instead of scrolling.
- **List and detail as two levels (`StackLevel`):** below 768 px a page with list and detail shows one level. `SplitView`
  takes `level` (from `Route::level()`), `back_label` and `back_href`. The first pane is the list level, all further
  panes are the detail level. A tap on an entry goes to its own address, the detail slides in from the right
  (`--ui-stack-duration`, `--ui-stack-ease`; no slide with `prefers-reduced-motion`), a round glass button ("‹", tooltip: name of the list) floats top left over
  the content instead of a bar and stays visible while reading, and a swipe from the left edge does the same. Addresses: `#/item/<id>`, `#/mine/item/<id>`
  (inbox), `#/overview/<name>`, `#/review/<repo>/<nr>`, `#/agent/<ticket>`; the list is the address without the last
  part. Browser back and reload therefore work. Free text in an address is escaped (`route/segment.rs`).
- **Shortcuts:** the key hints of the inbox are hidden when narrow; every shortcut has a button.
- **Wide content scrolls inside its box:** the diff and logs scroll sideways inside their
  container, never the page. Segmented pickers wrap; status texts wrap.
- **Layout checks** (`scripts/layout-check.js` and `scripts/check-overflow.js`, run in the browser pane against a test
  server): check every page at 320, 375, 768, 1100, 1280×720, 1440×900, 1920×1080, 2560×1440, 1280×600 and 812×375. No
  horizontal page scroll; from 768 px on no page scroll, equal pane heights, panes end above the pill; below 768 px the
  last content ends above the pill; the pill stays inside the viewport. The checks do not cover real devices (touch
  behaviour, notch, iOS address bar, on-screen keyboard) or real glass refraction.

## Development

Repository layout:

```
apps/
  cockpit/      main application: UI (WASM), own index.html and Trunk.toml
  server/       backend: axum, SQLite, key storage (binary cockpit-server)
  agent/        container image and runner of the agent (Node/TypeScript)
  playground/   shows every UI building block on its own
crates/
  cockpit-core/ domain logic: model, rules, Action; no UI, no network, with tests
  integrations/ GitHub and Jira clients, native only
  i18n/         message catalogs (German, English)
  ui/           component library (crate `feature-ui`), tokens.css, components.css, extras.css
container/      entrypoint.sh and cockpit-server.sh of the Cockpit image
config/         config.toml, cockpit.db, .env (created at the first start, ignored by Git) and config.example.toml
scripts/        see below
docs/           these documents
Containerfile   the Cockpit image; compose.yaml and compose.agent.yaml start it
```

Scripts in `scripts/`:

| Script | Purpose |
|--------|---------|
| `start.sh` | builds the app if needed and starts the server (`--open`, `--release`) |
| `cockpit-container.sh` | starts or updates the Cockpit container without compose ([container.md](container.md)) |
| `check-tracked-files.sh` | fails when Git tracks a `.env` file or anything in `config/` except the example (first CI step) |
| `image-size.sh` | compressed size of every platform of an image; the image workflow fails at 450 MB |
| `prune-image-versions.sh` | deletes every package version except the image just pushed |
| `layout-check.js` | layout check for the navigation, pasted into the browser pane against a test server |
| `check-overflow.js` | overflow check for panes and the page, run in the console on the Playground story `#extreme` or the app |
| `de.cockpit.server.plist` | template of a launchd LaunchAgent ([configuration.md](configuration.md#start-at-login-launchd-macos)) |

Run before every PR (CI runs the same):

```bash
cargo fmt --all --check
cargo clippy --workspace --exclude cockpit-server --exclude cockpit-integrations \
  --all-targets --target wasm32-unknown-unknown -- -D warnings
cargo clippy -p cockpit-server -p cockpit-integrations --all-targets -- -D warnings
cargo test --workspace
(cd apps/playground && trunk build) && (cd apps/cockpit && trunk build)
(cd apps/agent/runner && node --test test/*.test.ts)
```

The agent runner tests need Node ≥ 26.13.1 (the current LTS). Tests that start real containers are `#[ignore]`d:
`COCKPIT_CONTAINER=docker cargo test -- --ignored` (or `podman`). `trunk serve` (`cd apps/cockpit && trunk serve --open`,
port 8081, with a running `cockpit-server`) proxies `/api` to the server; start the server with `COCKPIT_DEV_PORT=8081` so
that the `Host` and `Origin` of that port are accepted.

**Trying things out.** Never run anything against real Jira or GitHub boards while trying things out: always your own
database (`COCKPIT_DB`), your own key file (`COCKPIT_ENV_FILE`), an empty `config.toml` (`COCKPIT_CONFIG`) and another
port than 8787. Automated tests use fixtures and recorded responses; they never call a real service. The services are
therefore not exercised by the tests: token exchange, refresh and revocation of the OAuth sign-ins, the Atlassian
`code_verifier` handling, the GitHub App's PKCE and installation redirect, and the Codex sandbox inside a container with
dropped capabilities are covered only by mocked responses or not at all. Treat the first real run with care and use a test
account or read-only rights.

**CI** ([`ci.yml`](../.github/workflows/ci.yml), [`image.yml`](../.github/workflows/image.yml)). Actions minutes are
limited, so every run is meant to be needed:

- `ci.yml` runs once per commit: on `pull_request` for a PR, on `push` only for `main` (and by hand). A newer commit on
  the same PR cancels the run still in progress; `main` is never cancelled.
- Everything is one job, `check` (the tracked-files check first, the agent runner tests last), because Actions bills
  each job rounded up to a full minute: a 10-second job costs as much as a 60-second one.
- The `check` job builds Trunk as a dev build, not a release build: the same code is compiled, so it fails on the same
  errors. The release build (opt-level `z`, LTO, wasm-opt) is made once, by the image workflow on `main`. It also runs
  the static BuildKit checks of the `Containerfile` (`docker buildx build --check -f Containerfile .`, builds nothing and
  fails on any violation of the default rules).
- The Rust cache (`Swatinem/rust-cache`) is saved only by `main`; a PR restores it and never adds entries. The same
  goes for the image's layer cache. `[profile.dev]` keeps only line tables (`debug = "line-tables-only"`, none for
  dependencies): panics and backtraces still show `file:line`, `target/` and the cache stay small.
- [`image.yml`](../.github/workflows/image.yml) builds the image only on a push to `main` (and by hand), never on a PR.
  It builds before it pushes, so a broken `Containerfile` or a failed size check fails the run without publishing or
  pruning anything, and the previous `latest` stays. In the `Containerfile` the compiled dependencies are their own layer
  (cargo-chef), so a source change on `main` rebuilds only the workspace crates. If a PR adds a native dependency or
  touches the cross-compile setup, run the workflow by hand on the branch first (Actions › Container image › Run
  workflow): it builds both platforms and checks the size, and off `main` it publishes nothing.
