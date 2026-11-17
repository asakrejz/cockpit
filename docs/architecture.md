# Cockpit architecture

Shared ground for everyone who works on Cockpit, people and agents alike: the decisions that were made, how the
pieces fit, the container rules, the HTTP API, what is synced and what goes outward, and how the agent behaves.
The README is the entry page; this document holds the depth.

## Decisions

| Question | Decision |
|----------|----------|
| Operation | **Local, for one person.** One service on your own computer (the author's checkout lives under `~/Documents/Web/rust-playground`). No hosting, no tenants. The repository stays private (no GitHub Pages). |
| Backend | Rust crate `apps/server/` (axum, tokio). It also serves the built web app. |
| Data | SQLite in one file in the user's data directory. |
| Keys | Only in the backend, in the `.env` file in the project folder (decision of 2026-11-13, risk accepted knowingly; never committed; older versions used the system keychain, `cockpit-server token import` copies those entries). **Never in the repo, the log, the database or browser storage.** Details: [security.md](security.md). |
| Agent | Claude Agent SDK, Claude Code or Codex in a container per run, **with Docker (Rancher Desktop) or Podman**. The container gets only the cloned repo, never a GitHub token. |
| Freshness | Polling every 2 minutes by default (`interval_s`). Webhooks are an optional accelerator, see [setup-guide.md](setup-guide.md#live-updates-and-webhooks). |
| Sample data | The mode that showed sample data in the browser without a backend is gone: the app needs the server. `demo::workspace()` stays as test data, for the Playground stories and as the seed `COCKPIT_SEED=demo` (only on explicit request and only when no state is stored yet). The server starts empty. |
| Agent provider and endpoint | Selectable (decision of 2026-11-12): tool `[agent] provider` (Claude Agent SDK, Claude Code CLI or Codex CLI, selected in the runner by `COCKPIT_PROVIDER`), endpoint `[agent.endpoint] url/model` (a proxy such as LiteLLM, another compatible endpoint, or without an address directly at Anthropic). The user has no Anthropic key of their own but an LiteLLM proxy, so LiteLLM is only one possible endpoint. With an address the container gets `ANTHROPIC_BASE_URL` + `ANTHROPIC_AUTH_TOKEN`, without one `ANTHROPIC_API_KEY` (Codex: `OPENAI_BASE_URL` + `OPENAI_API_KEY`, model in `COCKPIT_MODEL`); the model name is a setting chosen in the assistant from the endpoint's list. The key lives only in `.env` (`token set agent-key`). Older `[litellm]` values and the key `litellm` are still read. |
| Jira sign-in | API token by default (the company does not approve OAuth apps); OAuth is the folded-away alternative. |
| Setup | The setup assistant targets people without programming knowledge: everything that can be done locally by click is done by click (`PUT /api/config`, `PUT /api/secrets/{name}`, double-click start), the manual steps (creating your own OAuth apps, tokens) are guided and documented. Every person creates their **own** OAuth apps because client secrets cannot be shared. |
| Language | Code, naming and docs are English. Visible UI texts, plain-language errors and hints, the activity log and texts of the agent for humans stay German. Old German wire and config names are still read as aliases (see `crates/cockpit-core/tests/wire_compat.rs`, `apps/server/src/config/tests.rs`, `apps/cockpit/src/route.rs`). |

## Overview

```
Browser (apps/cockpit, WASM)            Backend (apps/server)
┌──────────────────────────┐   HTTP    ┌────────────────────────────────┐
│ Pages → store.dispatch   │ ────────▶ │ POST /api/actions              │──▶ GitHub, Jira
│ Workspace (signal)       │ ◀──────── │ GET  /api/workspace (snapshot) │──▶ Agent (container)
│                          │ ◀── SSE ─ │ GET  /api/events (changes)     │◀── polling
└──────────────────────────┘           │ SQLite · keys in .env          │
                                        └────────────────────────────────┘
          both use  crates/cockpit-core  (model, rules, Action)
```

Crates and apps:

| Path | Role |
|------|------|
| `crates/cockpit-core` | domain logic without UI and network: model, attention rules, review, agent state machine, usage and limits, rules per repo, sync merge, notifications, `Workspace`, `Action`, JSON shapes (`api`), persistence (`persist`). |
| `crates/integrations` | GitHub and Jira clients (read, write, sync), OAuth, outbound calls; native only; tested against recorded responses. |
| `crates/ui` | component library `feature-ui` with `tokens.css`, `components.css`, `extras.css`. |
| `apps/cockpit` | the Leptos WASM app (main application). |
| `apps/server` | `cockpit-server`: axum routes, SQLite, `.env` key store, sync loop, OAuth, container and agent processes (`agent_ops/` runs processes, `agent_run.rs` is pure, `agent_git.rs` clones/checks/pushes). |
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
4. **Every step stays runnable.** After every PR the Playground and the Cockpit build and the Cockpit runs with the
   server, with an empty start and with `COCKPIT_SEED=demo`.

Code layout (Rust): tests never stand inline. `foo.rs` ends with `#[cfg(test)] mod tests;` and the tests live in
`foo/tests.rs` (child module, so it sees private items); a big test file is split by topic into `foo/tests/<topic>.rs`
with shared helpers in `foo/tests.rs`. Logic files stay below roughly 500 lines: `foo.rs` keeps the core types and
re-exports (`pub use`) the topic modules in `foo/<topic>.rs`, so public paths do not change. Integration tests of a
crate with shared helpers use `tests/<name>/main.rs` plus topic files.

Gaps that were found *between* PRs and are worth remembering: tick tasks must run in the browser
(`apps/cockpit/src/main.rs`) **and** in the server (`apps/server/src/live.rs`, `tick`); look runs up through
`run.key()`, not `ticket_key` (follow-up jobs have their own keys); new local fields must not be overwritten by the sync
(`sync.rs`, `merge_item`).

## Container rules

The agent container must run with **Docker** (for example from Rancher Desktop) **and** with **Podman**. Rules for
anything that builds or starts containers:

1. **Command line only.** The backend calls `docker` or `podman` as a process, never the Docker socket or a Docker
   library. Only commands and switches both understand the same way are used (`build -f`, `run --rm`, `-e`, `-v`,
   `--network`, `--add-host`, `--user`, `--label`, `ps`, `rm -f`, `image inspect`).
2. **Runtime selectable.** Configuration `[agent] container = "docker" | "podman"`, overridable by `COCKPIT_CONTAINER`.
   Without either, the backend looks for `podman` first, then `docker` in the `PATH` and checks with `<cli> info` whether
   the engine runs. `/api/health` reports which one was found.
3. **Name images fully.** `docker.io/library/node:…` instead of `node:…`; Podman does not resolve short names, or only
   after asking. The build file is named `Containerfile`; both build it with `-f Containerfile`. No BuildKit features
   (`RUN --mount`, `# syntax=`).
4. **Reach the host.** A proxy (such as LiteLLM) on your own machine is reached from the container with
   `--add-host cockpit-host:host-gateway` (Docker 20.10+, Podman 5.x) and the address `http://cockpit-host:<port>`; not
   via `host.docker.internal` or `host.containers.internal`, which exist reliably for one runtime each.
5. **File permissions.** The repo is mounted (`-v <clone>:/work`) without SELinux suffixes (`:Z`). The process runs
   as a non-root user (`USER` in the `Containerfile`). Podman (rootless) and Docker map users differently, so after the
   run only read what lies in the clone.
6. **Secrets only as `-e NAME`** (value taken from the environment of the child process, not from the command line), so
   they do not appear in `ps` or logs.
7. **Rancher Desktop** must run with the engine "dockerd (moby)"; in "containerd" mode only `nerdctl` exists, which is not
   supported.
8. **Verify with both.** Tests that start real containers are `#[ignore]` and run with
   `COCKPIT_CONTAINER=docker cargo test -- --ignored` or `=podman`. The PR names the runtime it was checked with.
9. **Label per run:** `cockpit.run=<key>` lets the server remove orphaned containers at start (and mark their runs as
   cancelled).

## HTTP API

All routes live under `/api`; see [security.md](security.md) for the request checks.

| Route | Purpose |
|-------|---------|
| `GET /api/health` | `{status, version, connections, sign_in, sync, endpoint, container, agent, env_file}`; per connection only yes/no |
| `GET /api/workspace` | state as JSON (shape of `cockpit_core::persist`, plus `revision`) |
| `POST /api/actions` | body `{"action": <Action>}`; applied as the user. Answer `{revision, summary, logged}`; a rejected action is `422` (unknown shape `400`) with `{"error": "…"}`; a failed outward call is `502` |
| `POST /api/sync` | "sync now" with Jira and GitHub. Answer `{active, revision, logged}`; `active: false` when no source is set up |
| `GET /api/events` | server-sent events: first the current revision, then one event per action with the new revision and new log entries, plus keep-alive |
| `GET /api/jira/projects` | Jira projects for the inbox drop-down: `{active, projects: [{key, name}]}` |
| `POST /api/jira/check` | connection test for Jira with an API token, read only (`GET /rest/api/3/myself`): `{ok, cause, message, name}`; `cause` is `ok`, `details`, `address`, `token`, `permission`, `reachable` or `other`; never the token |
| `GET /api/agent/image`, `POST /api/agent/image` | state of "build image" `{running, present, error, lines}`, or start the build in the background |
| `GET /api/agent/check` | connection test to the endpoint: `{configured, reachable, key_ok, model_present, hint}` |
| `GET /api/agent/pull-request?run=<key>` | preview of the pull request for a finished real run: `{repo, head, base, title, body, draft, existing}`; creates nothing |
| `GET /api/agent/check/container` | the same test from a container: `{checked, mac_reached, container_reached, key_ok, hint}` |
| `GET /api/agent/models` | model IDs of the endpoint for the drop-down: `{loaded, reachable, key_ok, models, hint}`; only IDs, sorted (Claude models first), at most 200 |
| `GET /api/setup` | what the assistant prefills: Jira site/email/projects, client IDs, repos, provider, endpoint address and model, both callback addresses, and per key only `{name, set}` |
| `POST /api/github/repos/check` | read-only look at repos (`{repos:[…]}` → per repo `Ok`/`ReadOnly`/`NotFound`/`NoAccess`/`Failed` plus the default branch); only `GET /repos/{owner}/{name}`, never changes anything |
| `PUT /api/config` | writes client IDs, Jira, repos, the agent's repos (`agent_repos`: list with base branch and projects, plus default), agent provider and endpoint into `config.toml`; answer `{stored, takes_effect_immediately, hint}`; invalid input is `422` with plain text |
| `PUT /api/secrets/{name}` | stores a key in `.env`; body `{"value": "…"}`; names `github-oauth`, `atlassian-oauth`, `agent-key` (older `litellm`), `jira`, `github`, `github-webhook`, `jira-webhook`; answer `{name, set}`, never the value |
| `DELETE /api/secrets/{name}` | "Trennen" for tokens: deletes `jira` or `github` from `.env` (other names: `404`); answer `{name, set: false}` |
| `POST /api/webhooks/github`, `POST /api/webhooks/jira` | optional (`[server] webhooks`), the only routes reachable with a foreign `Host`: HMAC-signed triggers for the sync; `202`/`200`/`204`/`401`/`404`/`413`/`415`/`429`, no body, payload never used as data |
| `POST /api/setup/webhook-secret/{source}` | generates a webhook secret (`github`\|`jira`), stores it in `.env`, returns it once: `{secret}` |
| `POST /api/setup/webhook-test/{source}` | signed test request from the server to itself over loopback: `{ok, message}` |
| `GET /api/oauth/{service}/start`, `GET …/callback`, `POST …/revoke` | OAuth sign-in for `github` and `atlassian` |

If the server is unreachable (not started, other port) the app shows a notice at the top with "Erneut versuchen"; the pages
stay empty and nothing is lost.

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

**What is synced.** The backend polls both services on the interval (and on `POST /api/sync`, in the app under
"Einrichten" › Übersicht › Abgleich › "Jetzt abgleichen") and merges the result through the action `SyncRemote`
(author "System") into the workspace, as a pure function: `cockpit_core::sync::merge`.

- New entries are added, removed ones disappear. Local state stays: read/done, snoozes, agent runs and hints,
  suggestions and decisions in a review, your own unsent line comments (they move with their line when the diff
  shifts), verdict and "sent".
- New comments and status, check and review changes from outside appear in the log with author "System" and trigger the
  notifications. `@your-login` in comments counts as a mention.
- Per source there is a state (`/api/health` › `sync`, in the app under "Abgleich"): `ok`, `unauthorized` (401, token
  expired), `no_permission` (403), `quota` (rate limit: the source pauses until the time from
  `Retry-After`/`X-RateLimit-Reset`), `offline`, `error`; plus `off` (not set up or no key) and `waiting` (no fetch
  yet). On an error the last data of the source stays. Each source state also carries `since_ms` (since when the
  problem lasts; unchanged while the same kind of problem repeats, cleared on recovery); `/api/health` › `sync` ›
  `details[]` lists `{source, status, since_ms, retry_at_ms, last_ok_ms}` per fetched source (no keys, no service
  messages). `since_ms` is `#[serde(default)]`, so older snapshots stay valid.
- A 5xx or a timeout is a source problem (`error` / `offline`), not an error of the sync call: `POST /api/sync` still
  answers 200. A source in `quota` with a known time is not queried at all until then.
- A pull request is identified by `PrKey { repo: "owner/name", number }` (`cockpit_core::PrKey`, text form
  `owner/name#12`). `Review`, `PrRef`, `PrLink`, the actions (`review`, `pr`), `Target::Review`, follow-up runs
  (`review-acme~app-12`, `check-acme~app-12-lint`) and notification routes carry it, so equal numbers from two repos
  never mix. On the wire a key without repo is the plain number (the format before several repos); both forms are
  read, and an empty repo is not written, so old snapshots and clients stay byte-compatible (`wire_compat`).
  A key without repo matches any repo with that number (old data, single-repo callers); a key with repo matches only
  that repo or an entry whose repo is still unknown. `Workspace::fill_repos` migrates old states at load: the repo
  comes from the sync link, from the inbox entry, or from the only configured repo (`Live::fill_repos`).
- **Ticket → repo.** `cockpit_core::AgentRepos` (`[[agent.repos]]`: repo, base branch, Jira projects, plus the default)
  chooses the repo of a ticket (`choose(project)`: rule, else default, else none). The backend applies it in
  `AppState::agent_route` before `StartAgent` (user's `AgentOptions.repo` wins if it is a configured repo) and
  `GET /api/health` reports the same rules under `agent.repos`, so the interface preselects identically. A run
  stores its repo (`AgentRun.repo`), clones and pushes only there, and checks the repo against the configuration
  again at start. `CreateBranch` for a Jira ticket uses the same routing.

**What goes outward.** In server mode these actions go to the service first; **only when it confirms is the action
applied locally**. If the call fails, a German message (status 502) comes back and nothing changes. The rules of the
workspace (also "only you may merge", "review only after deciding on every suggestion") apply before every call.

| Action | Call |
|--------|------|
| `Reply` | Jira: comment (`POST /rest/api/3/issue/{key}/comment`, ADF) · GitHub: `POST /repos/{repo}/issues/{n}/comments` |
| `SetStatus` | Jira: look for the transition that leads to the target status (`GET …/transitions`), then `POST …/transitions` |
| `CreateBranch` | GitHub: ref from the default branch of the repo the ticket's project is routed to, otherwise the **first** configured repo (`POST /repos/{repo}/git/refs`) |
| `SendReview` | `POST /repos/{repo}/pulls/{n}/reviews` with verdict, text, approved suggestions and your own line comments as position comments (including `suggestion` blocks), bound to the commit seen |
| `MergePr` | `PUT /repos/{repo}/pulls/{n}/merge` with method and the commit seen (a new push makes GitHub refuse) |
| `RestartCheck` | `POST /repos/{repo}/check-runs/{id}/rerequest` |

All other actions, and every entry that came from a seed, stay local. The action `SyncRemote` may only be triggered by
the backend; `POST /api/actions` rejects it.

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
  "Einrichten", rate limit with time → wait (says when), otherwise "Jetzt abgleichen".
- `cockpit_core::notice::http_failure_text` turns a failed request to the local backend (by HTTP status plus the
  backend's own German message) into the notice text; pages never build error strings themselves.
- UI blocks: `EmptyState`, `ErrorState` (`role="alert"`), `StatusBanner` (`alert` for errors, `status` otherwise) and
  `Skeleton` (`aria-busy`) in `crates/ui/src/state.rs`, styled in `extras.css` with design tokens only.

## Agent behaviour

**Real agent versus simulation.** The agent in the server is a **real run in a container** (see the [setup guide](setup-guide.md#agent-container-image-model)). Only with the seed `COCKPIT_SEED=demo` (and in tests) the simulated agent still runs: a timer that walks
through the five steps. The real run is `apps/server/src/agent_run.rs` (pure), `agent_ops.rs` and `agent_ops/` (`run`,
`watcher`, `image_build`, `container_check`, `lines`) for processes, `agent_git.rs` for clone/check/push; the core side
is `AgentReport` / `Action::ReportAgent`. The runner protocol is described in [../apps/agent/README.md](../apps/agent/README.md).
The simulated follow-up jobs (below) do not exist in real mode; there the two real follow-up jobs apply.

**Questions and steering.** The agent can stop at a step and ask a question (status "Wartet auf Antwort", with
suggested answers to click or free text). The question counts as attention: it appears in the overview under "Hängt"
(button "Frage beantworten") and in the sidebar under "Agenten". While the agent runs you can send a hint under
"Nachsteuern"; it confirms at the next step. "Pausieren" stops the run, "Fortsetzen" goes on. Everything said appears
in the history next to the timeline. The actions are `AnswerAgent`, `SteerAgent`, `PauseAgent`, `ResumeAgent`.

**Plan before start.** The agent presents a plan before the start and does nothing until you approve it (you may reject
it or ask again with a remark). A review is only sent once every suggestion has been decided on (your own line
comments need no approval). The agent never merges; its result is a pull request waiting for your review.

**Follow-up jobs on a pull request** (review page, right column):

- *Implement review comments.* Under "Review-Kommentare umsetzen" the open line comments are listed (yours and the
  reviewers', also with a suggested change), all selected by default. "Agent: n Kommentare umsetzen" starts the run
  `review-<nr>` (`RunKind::AddressReview`) with its own steps (read comments, check out branch, changes, tests, push). It
  pushes to the **same branch**, no new pull request. Plan approval, questions, steering and pausing work as for the
  ticket run. At the end the comments are "Erledigt" (with a reference to the run), suggested changes appear in the diff,
  the entry reports the new commit and returns to "Review angefragt". Comments of others never go out with your review
  and cannot be edited.
- *Investigate a red check.* Under every red check with a log: "Agent untersuchen lassen" (run `check-<nr>-<check>`,
  `RunKind::Diagnose`). The agent reads the log and presents a **diagnosis**: cause in one sentence, file and line, a
  fix as a diff. Until you decide, the branch stays untouched. "Fix übernehmen" pushes the proposal to the branch and
  restarts the check, "Verwerfen" ends the run.

The actions are `AddressReview`, `InvestigateCheck`, `ApplyFix`, `DiscardFix`. A waiting diagnosis counts as attention
and triggers "Agent wartet" / "Agent fertig". The agent never merges here either.

**Follow-up jobs of the real agent** (`RunKind::Followup`, actions `AddressReviewThreads` and `FixFailedChecks`, only
you may trigger them). At click time and again at run start the backend reads the inputs from GitHub with the user's
token (`GitHubClient::review_input` / `failed_checks`, read only; shortened by `github::followup`) and writes them into
`job.json` under `followup` (secrets of the agent key and GitHub token are redacted from it). The watcher starts a
container like for a normal run (`RunKind::runs_in_container`); the clone is the existing `claude/…` branch
(`agent_git::clone_branch`), the check and push are the same as always (fast-forward only, path rules, never a
force-push). The container never gets the GitHub token. The image has the fixed name `localhost/cockpit-agent:local`; whether it is current is detected by the label `cockpit.fingerprint` (SHA-256 over Containerfile, runner sources, the lockfiles of runner and CLIs, and the `CONTRACT` constant; details in `apps/agent/README.md`), and a run only starts with a current image. The image contains the Claude Code and Codex CLIs, pinned in `apps/agent/clis/package-lock.json`; the job contract gained
`followup`, and `repo` – the `owner/name` the backend cloned; follow-up jobs and the pull request always use the repo of the
pull request / run, never "the" configured repo).

**Pull request of a finished real run** (`Action::CreatePullRequest`, only you). The run is `Done` only after the
backend pushed the branch. `GET /api/agent/pull-request` returns the preview (`Workspace::pull_request_draft`, plus the
open pull request GitHub already has for the branch). The action is checked on a copy first (`check_pull_request`), then
`agent_followup.rs` looks for an open pull request for the branch, creates one if there is none
(`POST /repos/{repo}/pulls`, with the edited title and text) and books GitHub's answer on the run
(`AgentRun::pull_request`); a `pull` value sent by the browser is ignored. A 422 "already exists" after a race is
resolved by linking the existing one. A sync follows so the pull request appears as a review.

**Costs and limits.** Every step has a usage (input/output tokens, cost in whole euro cents, duration) shown at the
timeline, with the run total plus "today" and "this month" below. Prices are a setting; the real agent reports its
values through `AgentRun::record_usage`. If a run reaches a limit (budget per run, monthly budget, time limit) it stops
(`RunState::Stopped(StopReason)`): the agent page offers "Um X erhöhen und fortsetzen" (`ResumeStopped`) or "Abbrechen",
the run shows under "Hängt" in the overview and a notification "Agent angehalten" fires. A new run above the parallel-run
limit **does not start** (`ActionError::ParallelLimit`, German message); there is deliberately no queue. Day and month
count in UTC; cancelled runs stay in the ledger.

**Rules per repo.** Per repo (key: name) allowed and forbidden paths (glob patterns, `cockpit_core::rules::glob_match`),
required tests, instructions and a plan requirement apply. Plan and steps show the rules that apply. A plan that touches
forbidden paths cannot be approved (`ActionError::PlanViolatesRules`, with a reason); such a run always waits with a plan
first. Required tests cannot be switched off. Changed rules apply to new runs and waiting plans. The actions are
`SetAgentLimits` and `SetRepoRules`. The defaults, until rules are set: branches, pull requests and tests allowed;
comments and reviews only after your approval; the agent never merges.

**Status follows the pull request.** A ticket's status follows its linked pull request (draft/open → In Arbeit, review
requested → Review, merged → Fertig); switchable in the sidebar, author in the log: System. Ticket and pull request are
linked automatically by the key in branch or title (`BAU-142`), otherwise by hand.

**Review mechanics.** For a line comment, "Änderung vorschlagen" lets you enter replacement code for the line
(before/after diff, sent as a `suggestion` block). The checks of the pull request appear on the right with their log;
red ones can be restarted. "Mergen" (merge/squash/rebase) works only with green checks, a sent review and without
"Änderungen" verdict, and only for you, never for the agent.

## Persistence

Everything the app shows lives in the backend (SQLite, migration 2) as JSON with a version number, see
`crates/cockpit-core/src/persist.rs`. If the version does not match or the data is unreadable, the server starts with the
seed (empty or sample data). New fields in the types need `#[serde(default)]` so older states stay readable. The
compatibility of stored JSON across renames is guarded by `crates/cockpit-core/tests/wire_compat.rs`
(`wire_*.json` current snapshots, `legacy_*.json` older ones that must keep loading); regenerate the snapshots with
`BLESS=1 cargo test -p cockpit-core --test wire_compat` only on purpose.

## Notifications

Which events notify is derived purely from comparing the workspace before and after an action
(`crates/cockpit-core/src/notify.rs`, with tests). The settings live in the workspace and change through actions
(`SetNotifyChannel`, `SetQuietHours`). Email and Slack are prepared only; addresses and webhook URLs never live in the
browser.

## Narrow screens (phone and tablet)

The interface is usable from 320 px width. The CSS lives in `crates/ui/extras.css` (building blocks) and
`apps/cockpit/style.css` (Cockpit specifics), only with existing tokens; the `viewport` meta tag is set and fixed
elements respect `env(safe-area-inset-*)`.

| Breakpoint | What changes |
| --- | --- |
| `max-width: 1100px` | Phone and tablet layout. The three columns of `SplitView` stack, the sidebar dissolves (`display: contents`) and the fixed `BottomNav` takes over the navigation. The bar is at most 640 px wide and centred, so it stays one row on a tablet (768–1100 px) too: a persistent sidebar would leave about 500 px for list and detail there. |
| `max-height: 500px` (with the above) | Landscape phone: flatter bar, `Mehr` sheet scrolls inside itself. |
| `max-width: 640px` | Tighter segmented pickers. |
| `max-width: 1100px` or `pointer: coarse` | Touch sizes: buttons, segmented pickers, selects, steppers and sidebar entries are at least 44 px (`--sys-size-hit`), selectable diff lines 34 px. |
| above 1100px | Desktop layout, unchanged: sidebar, list and detail side by side. |

- **Navigation (one definition, two presentations):** `TARGETS` in `apps/cockpit/src/sidebar.rs` lists all 11 pages once,
  each with a mobile slot (`Slot::Bar` or `Slot::More`). Wide screens show all of them in the sidebar. Narrow screens
  show a fixed `BottomNav` with five equal slots: Inbox, Aufgaben, Reviews, Agenten and **Mehr**. Nothing scrolls
  sideways. "Mehr" opens a bottom sheet with the other targets (Team, Zurückgestellt, Übersicht, Protokoll, Grenzen &
  Regeln, Benachrichtigungen, Einrichten) and the connection states (Jira, GitHub, agent). Counters stay visible as
  badges; the badge of "Mehr" is the sum of what is new behind it (team, stuck tickets), and "Mehr" is marked as the
  current page when the current page lives in the sheet. Moving a page between bar and sheet means changing one `slot`.
  The sheet is a modal dialog (`role="dialog"`, `aria-modal`): focus moves in, Tab stays inside, Escape, a tap on the
  dimmed area, the close button, a followed link and `hashchange` close it, focus returns to the button, and the page
  behind does not scroll (`body.ui-scroll-locked`). Sidebar groups carry a `SectionRole`: `Primary` (the navigation,
  hidden when narrow), `Picker` (selects the item of a page, e.g. a pull request or an agent run; a chip strip above
  the content that fades out on the right) and `Default` (rules; a card below the content).
- **Phone details:** `viewport-fit=cover` plus `env(safe-area-inset-*)` on the bar, the sheet and the page; `dvh` instead
  of `vh` for full-height areas (iOS address bar); the bar is at least 52 px high with 44 px+ hit areas and steps aside
  while a text field has the focus on touch devices (the on-screen keyboard would put the bar over the field); in
  landscape (height up to 500 px) the bar is flatter (icon beside the label) and the sheet scrolls inside itself. Long
  button labels and addresses wrap; the filter bar of the inbox wraps instead of scrolling.
- **List and detail as two levels:** the inbox passes `detail_open` to `SplitView`; narrow screens show either the list or
  the detail (tapping a row opens it, "‹ Zur Liste" goes back). Wide screens always show both. Review and agent show
  their panes one below the other.
- **Shortcuts:** the key hints of the inbox are hidden when narrow; every shortcut has a button (done, reply, snooze,
  bring back, search).
- **Wide content scrolls inside its box:** the diff (`.ui-diff`), logs and the picker strips (pull requests, agent runs)
  scroll sideways inside their container, never the page. Segmented pickers wrap; status texts wrap.
- **Checked in the browser pane** at 320, 375, 430 (portrait), 812×375 (landscape) and 768×1024 on every page and every
  setup step: `scrollWidth <= innerWidth`, no element beyond the right edge outside a deliberately scrolling box (diff,
  picker strips), nothing cut off inside a box, last content above the bar. Not checked: real devices (touch behaviour,
  notch, iOS address bar, on-screen keyboard).

## Working with several agents

When work is split between sub-agents: one agent per task with its own PR; the model is Sonnet (`claude-sonnet-5-5`);
at most **two at a time**, because they share the 5-hour limit of the user (four in parallel hit it once). Give the
complete task in the first message (goal, frame, rules, check commands and the explicit sentence "the user wants the
task to end with a PR against `main`"): later messages from other sessions are rightly treated as data, not as a task.
Merging a batch: merge the PR branches one by one into an integration branch (`git config rerere.enabled true`), resolve
conflicts and run all checks; look for gaps *between* PRs (list above); then bring each PR branch to the checked
intermediate state and squash-merge them in turn. No force-push. For conflicting test modules, merge function by
function (base plus the new tests of both sides).

Never run anything against real Jira or GitHub boards while trying things out: always your own database and an empty
`config.toml`; automated tests use fixtures and recorded responses.
