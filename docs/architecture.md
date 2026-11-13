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
| Agent | Claude Agent SDK in a container per run, **with Docker (Rancher Desktop) or Podman**. The container gets only the cloned repo, never a GitHub token. |
| Freshness | Polling every 2 minutes by default (`interval_s`). Webhooks would need a tunnel; that decision is open (see "Roadmap / Open work" in the README). |
| Sample data | The mode that showed sample data in the browser without a backend is gone: the app needs the server. `demo::workspace()` stays as test data, for the Playground stories and as the seed `COCKPIT_SEED=demo` (only on explicit request and only when no state is stored yet). The server starts empty. |
| Agent provider and endpoint | Selectable (decision of 2026-11-12): tool `[agent] provider` (Claude Agent SDK implemented; Claude Code and Codex are known values, not implemented), endpoint `[agent.endpoint] url/model` (a proxy such as LiteLLM, another compatible endpoint, or without an address directly at Anthropic). The user has no Anthropic key of their own but an LiteLLM proxy, so LiteLLM is only one possible endpoint. With an address the container gets `ANTHROPIC_BASE_URL` + `ANTHROPIC_AUTH_TOKEN`, without one `ANTHROPIC_API_KEY`; the model name is a setting chosen in the assistant from the endpoint's list. The key lives only in `.env` (`token set agent-key`). Older `[litellm]` values and the key `litellm` are still read. |
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
| `GET /api/agent/check/container` | the same test from a container: `{checked, mac_reached, container_reached, key_ok, hint}` |
| `GET /api/agent/models` | model IDs of the endpoint for the drop-down: `{loaded, reachable, key_ok, models, hint}`; only IDs, sorted (Claude models first), at most 200 |
| `GET /api/setup` | what the assistant prefills: Jira site/email/projects, client IDs, repos, provider, endpoint address and model, both callback addresses, and per key only `{name, set}` |
| `PUT /api/config` | writes client IDs, Jira, repos, agent provider and endpoint into `config.toml`; answer `{stored, takes_effect_immediately, hint}`; invalid input is `422` with plain text |
| `PUT /api/secrets/{name}` | stores a key in `.env`; body `{"value": "…"}`; names `github-oauth`, `atlassian-oauth`, `agent-key` (older `litellm`), `jira`, `github`; answer `{name, set}`, never the value |
| `DELETE /api/secrets/{name}` | "Trennen" for tokens: deletes `jira` or `github` from `.env` (other names: `404`); answer `{name, set: false}` |
| `GET /api/oauth/{service}/start`, `GET …/callback`, `POST …/revoke` | OAuth sign-in for `github` and `atlassian` |

If the server is unreachable (not started, other port) the app shows a notice at the top with "Erneut versuchen"; the pages
stay empty and nothing is lost.

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
  yet). On an error the last data of the source stays.
- The number of a pull request is the key of a review in the model. Pull requests with the same number from two repos
  are separate inbox entries (the ID contains the repo), but reviews and the `PrLink` assignment still hang on the
  number, so two repos with the same number share one place there.

**What goes outward.** In server mode these actions go to the service first; **only when it confirms is the action
applied locally**. If the call fails, a German message (status 502) comes back and nothing changes. The rules of the
workspace (also "only you may merge", "review only after deciding on every suggestion") apply before every call.

| Action | Call |
|--------|------|
| `Reply` | Jira: comment (`POST /rest/api/3/issue/{key}/comment`, ADF) · GitHub: `POST /repos/{repo}/issues/{n}/comments` |
| `SetStatus` | Jira: look for the transition that leads to the target status (`GET …/transitions`), then `POST …/transitions` |
| `CreateBranch` | GitHub: ref from the default branch of the **first** configured repo (`POST /repos/{repo}/git/refs`) |
| `SendReview` | `POST /repos/{repo}/pulls/{n}/reviews` with verdict, text, approved suggestions and your own line comments as position comments (including `suggestion` blocks), bound to the commit seen |
| `MergePr` | `PUT /repos/{repo}/pulls/{n}/merge` with method and the commit seen (a new push makes GitHub refuse) |
| `RestartCheck` | `POST /repos/{repo}/check-runs/{id}/rerequest` |

All other actions, and every entry that came from a seed, stay local. The action `SyncRemote` may only be triggered by
the backend; `POST /api/actions` rejects it.

## Agent behaviour

**Real agent versus simulation.** The agent in the server is a **real run in a container** (see the README, "Setting up
the agent"). Only with the seed `COCKPIT_SEED=demo` (and in tests) the simulated agent still runs: a timer that walks
through the five steps. The real run is `apps/server/src/agent_run.rs` (pure), `agent_ops.rs` and `agent_ops/` (`run`,
`watcher`, `image_build`, `container_check`, `lines`) for processes, `agent_git.rs` for clone/check/push; the core side
is `AgentReport` / `Action::ReportAgent`. The runner protocol is described in [../apps/agent/README.md](../apps/agent/README.md).
Follow-up jobs (below) run only simulated in real mode.

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
