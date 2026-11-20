# Agent image

The Cockpit agent runs in its own container per run (Docker or Podman). This directory holds the image:

- `Containerfile`: Node 26, Git, the runner and the agent CLIs; runs as the user `node` (not root).
- `clis/`: `package.json` + `package-lock.json` with the exact versions of `@anthropic-ai/claude-code` and `@openai/codex`
  (installed with `npm ci`, never "latest"; no secret file is written during the install). To update a CLI, change the
  version in `clis/package.json`, run `npm install --package-lock-only` there, review and rebuild (the fingerprint changes by itself).
- `runner/`: the runner (TypeScript, no build step: Node executes `.ts` directly). It reads the job from
  `/job/job.json` and executes it with an **adapter** (see below).

Build through "Image bauen" (or "Neu bauen") in the setup assistant. The backend runs
`<docker|podman> build -f apps/agent/Containerfile --label cockpit.fingerprint=<hex> -t localhost/cockpit-agent:local apps/agent`.
A build by hand works without the label (`docker build -f apps/agent/Containerfile -t localhost/cockpit-agent:local apps/agent`,
or `podman build …`), but then the image counts as outdated and the agent does not start with it.

### Fixed name and fingerprint

The image name never changes: `localhost/cockpit-agent:local` (one constant, `IMAGE` in
`apps/server/src/agent_image.rs`). A rebuild overwrites it. Whether the image is up to date is decided by a
fingerprint, not by a number in the tag:

- The fingerprint is a SHA-256 over these inputs, sorted by relative path and hashed together with the file names:
  `Containerfile`, `runner/package.json`, `runner/package-lock.json`, everything under `runner/src/`
  (what the Containerfile copies), `clis/package.json`, `clis/package-lock.json`, and the constant `CONTRACT` in
  `agent_image.rs`. **Not included:** `runner/test/` (not part of the image), `node_modules`, `.DS_Store`, file times and
  permissions. `\r\n` counts as `\n`.
- The build stores it as the image label `cockpit.fingerprint`. The server reads the label
  (`image inspect --format '{{ index .Config.Labels "cockpit.fingerprint" }}'`) and compares: no image = `missing`, no
  label or other value = `outdated`, same value = `current`, sources not found = `unknown` ("nicht prüfbar").
  A run only starts with a `current` image.
- Raise `CONTRACT` (rarely) when the job or event format between server and runner changes **without** any file of the
  image changing, so that old images are still recognised as outdated.
- Images from the time of numbered tags are no longer used. Remove them by hand if wanted (Cockpit never does), for
  example with `docker image prune` (`podman` likewise).

Rules for the `Containerfile` (they hold for Docker and Podman alike): a fully qualified base image, no BuildKit
features (`# syntax=`, `RUN --mount`), the runner never works as root. See "Container rules" in
[../../docs/architecture.md](../../docs/architecture.md#container-rules).

## Adapters

Every agent tool is an adapter in `runner/src/adapter/` with the shared interface from `interface.ts`: job in (plus a
sink, Git, a stdin input and the environment), JSON-line events out (table below), exit code as the result.
`COCKPIT_PROVIDER` (default `claude-agent-sdk`; the image sets the default, the backend passes the configured
`[agent] provider` with `-e COCKPIT_PROVIDER`) selects the adapter; unknown names produce an `error` event in plain
language.

| Adapter | Tool | Endpoint and key (environment) |
|---------|------|--------------------------------|
| `claude-agent-sdk` (`adapter/claude-agent-sdk.ts`) | Claude Agent SDK; asks through `canUseTool`, tool `ask_user` | `ANTHROPIC_*` |
| `claude-code` (`adapter/claude-code.ts`) | `claude -p --output-format stream-json --verbose`, prompt on stdin | `ANTHROPIC_BASE_URL` + `ANTHROPIC_AUTH_TOKEN` or `ANTHROPIC_API_KEY`, `ANTHROPIC_MODEL` |
| `codex` (`adapter/codex.ts`) | `codex exec --json … -`, prompt on stdin | `OPENAI_API_KEY`, optional `OPENAI_BASE_URL` (`/v1` is added), `COCKPIT_MODEL` |

The CLI adapters share `adapter/cli.ts` (child process, JSON lines, secret scrubbing, checks after the run); the flags
live in one place per adapter (`CLAUDE` / `CODEX` and `claudeArgs` / `codexArgs`).

### Claude Code (`claude-code`)

Flags: `-p --output-format stream-json --verbose --model <m> --append-system-prompt <rules> --permission-mode default
--allowedTools Read,Glob,Grep[,Write,Edit,MultiEdit,Bash,TodoWrite] --disallowedTools WebFetch,WebSearch --max-turns <n>
--settings <file> --setting-sources user --strict-mcp-config` (plan mode: only the reading tools). Never a bypass or
"accept all" mode. The `--settings` file (temporary, outside `/work`, no secret) registers a `PreToolUse` hook
(`src/hook.ts`) that applies `policy.ts` (`decide()`) to every tool call, like `canUseTool` does for the SDK: it
fails closed (a hook that cannot read its policy denies everything). Settings of the repo are not loaded, MCP servers are
ignored. The stream lines have the same shape as the SDK messages (`adapter/claude-messages.ts` translates both).

### Codex (`codex`)

Flags: `exec --json --color never --sandbox workspace-write -C /work -m <model> -c approval_policy="never"
-c sandbox_workspace_write.network_access=false -c model_provider="cockpit" -c model_providers.cockpit.base_url=…
-c model_providers.cockpit.env_key="OPENAI_API_KEY" -c model_providers.cockpit.wire_api="responses" -`
(plan mode: `--sandbox read-only`). `CODEX_HOME` is a temporary folder. Codex has no per-path hook: the path rules
are enforced by the runner after the run (below). Event translation: `item.completed` (`agent_message` = answer,
`command_execution` = step, `file_change` = step + `files`), `turn.completed` (usage = step "Überlegt"),
`turn.failed` = error; a bare `error` event only counts if the turn never completes.

### Checks after every CLI run (`adapter/cli.ts`)

* Every changed file (working tree and commits the agent made itself) is checked with `policy.ts`
  (`violations()`): `.git`, forbidden paths, outside the allowed paths. A violation ends the run with an `error`
  event and nothing is committed. In plan mode any changed file discards the plan.
* The child process gets a clean environment (only `PATH`, `HOME`, locale and the endpoint names of its family);
  the prompt goes through stdin, never an argument; values of variables named like `*KEY*`/`*TOKEN*` are replaced by
  `[entfernt]` in every event. Broken JSON lines are skipped and counted; a stream without a final result, a non-zero
  exit status and the time limit (`max_seconds`) become `error` events with the end of stderr.
* Not supported by the CLI adapters: questions to you (`ask_user`) and hints while running. Cost figures of the CLIs are
  not forwarded (the events carry tokens only).

A further adapter does not need to change anything in the backend: same event shapes, same rules (no push, write only to
`/work`, commit through `git.ts`).

## Endpoint in the environment

The backend passes the values to the container only as `-e NAME` (never in the argument list). With an endpoint
address (a proxy such as LiteLLM, another compatible endpoint): `ANTHROPIC_BASE_URL` and `ANTHROPIC_AUTH_TOKEN`
(bearer). Without an address (directly at Anthropic): `ANTHROPIC_API_KEY` (`x-api-key`). Always also
`ANTHROPIC_MODEL`. Names that are not used are removed by the backend from the inherited environment so that no key
from the person's shell ends up in the container.

## Events (stdout, one JSON line per event)

| `t` | Fields | Meaning |
|-----|--------|---------|
| `plan` | `summary`, `files`, `steps` | plan (mode `plan`) |
| `step` | `title`, `text`, `tokens_in`, `tokens_out`, `ms` | a work step with usage |
| `files` | `files` | files changed so far |
| `question` | `text`, `suggestions` | question; the runner waits for `answer` on stdin |
| `hints_arrived` | none | a hint from the user reached the agent |
| `review_comment` | `path`, `line`, `start_line?`, `side` (`RIGHT`\|`LEFT`), `text`, `suggestion?` | a draft review comment (mode `review`); the backend checks it against the diff |
| `review_summary` | `text` | the agent's summary of the pull request (mode `review`); stays local |
| `result` | `text`, `branch`, `commit`, `repos?` | done; the commit is on the branch in the clone (mode `review`: no commit, `commit` is empty). In a job in several repos `repos` lists `{repo, branch, commit}` of every repo that has a commit; repos without changes are missing and get nothing |
| `check` | `reachable`, `key_ok`, `hint` | result of the check mode |
| `error` | `text` | abort with a reason |

Lines arriving on stdin: `{"t":"answer","text":…}` and `{"t":"hint","text":…}`.

## Languages

`job.json` carries three language codes (`"de"` or `"en"`):

- `language`: the language of the interface. The system prompt ends with "Answer in English." or "Antworte auf Deutsch."; plan,
  summary and questions are written in it (review runs use `review_language`, below). Step titles and error texts of the runner (`src/texts.ts`) are in it
  too. Missing: `"de"` (older backends).
- `pr_language`: the language of the text the agent creates for a pull request. The commit message the runner creates is
  written in it. The backend picks it per run: detected from the ticket description, else the repo's `pr_language`
  (`[[agent.repos]]`), else English. Missing: `"en"`.
- `review_language`: only for mode `review`. The language of the summary, every comment and every `suggestion` text. The
  system prompt and the task both say so ("Write the summary and all review comments in English/auf Deutsch …"); file names,
  identifiers and JSON keys stay as they are. The backend picks it in this order: the switch in the review panel, the
  language detected from the pull request title and description (plus the ticket description if the description is short),
  the repo's `pr_language`, English. It does not change the language of step titles (that is `language`). Missing: `language`.
  No `CONTRACT` bump: the runner sources changed, so the image fingerprint changes anyway.

The prompt itself stays as it is; only the answer language is set. Technical output (stderr, logs, `expect`) is English.

## Follow-up jobs

For a follow-up job on an existing pull request `job.json` carries `followup`:
`{kind: "review_comments" | "failed_checks", pull_request, threads: [{path, line, comments: [{author, text}]}],
notes: [{author, state, text}], checks: [{name, excerpt}]}`; `branch` is the existing `claude/…` branch, already
checked out in `/work`. The runner puts the texts into the prompt as data of third parties, limits them again
(`LIMITS` in `job.ts`) and commits with a message that names the task. Everything else (events, no push, rules) is
unchanged; old jobs without `followup` work as before.

## Review runs (mode `review`)

For "PR reviewen" `job.json` has `mode: "review"` and `review`: `{pull_request, title, description, branch, base,
files: [{name, lines: [{no, kind: "add" | "del" | "ctx", text}]}], omitted_files}`. The diff comes from the state the
backend already holds (nothing is fetched at run time) and is limited in size (`REVIEW_LIMITS` in `job.ts`; the backend
limits first). The runner puts title, description and diff into the prompt as data of third parties, marks every line
as `[R12]` (line 12 of the new file) or `[L10]` (line 10 of the old file), and asks for one JSON object
`{"summary", "comments": [{path, line, start_line?, side, text, suggestion?}]}`.

* **Read only.** `/work` is the clone of the pull request's branch (any branch, not only `claude/…`), mounted `:ro`.
  Claude Agent SDK: permission mode `plan`, tools `Read`/`Glob`/`Grep`, no `ask_user`. Claude Code: only the reading
  tools. Codex: `--sandbox read-only`. A CLI that changes a file anyway discards the whole review. No commit, no push.
* **Events.** The runner emits one `review_comment` per usable entry (at most 40; malformed ones are dropped, a
  `suggestion` only with `side: "RIGHT"`), then `review_summary`, then `result`. The backend again checks every comment:
  the file must be in the diff, the line must exist on that side, texts and counts are limited, duplicates dropped.
  What passes becomes a **draft** of the review; the runner and the container never talk to GitHub.

## Rules in the container

The container has **no GitHub token**: it cannot push. It commits to a branch in the mounted
clone, named by the repo's `branch_pattern` (`claude/<ticket>-…` by default); the backend checks and pushes afterwards
(name and path rules of the repo, never the main branch, never a merge). If the repo has a `CONTRIBUTING.md`, the job
names it (`contributing`) and the system text tells the agent to follow its commit conventions.
The runner additionally blocks writing outside `/work`, `.git`, forbidden paths and commands such as `git push` and
`curl` (see the adapters above for how each tool enforces it). A review run is stricter still: read-only clone, no
commit. The agent asks questions through the tool `ask_user`.

## Jobs in several repos

If the job has `repos` (two or more entries `{repo, dir, branch, base_branch, rules, contributing, pr_language}`), the backend
has cloned each repo to `/work/<owner>__<name>` (`dir`) instead of cloning one repo into `/work`. The runner then works
from `/work`:

- the system text (`multiSystemText`) names every repo with folder, base branch and branch and repeats each repo's rules
  and guidelines file under its own name;
- the path policy (`policyOf`) takes the first folder of a path to find the repo and applies **that repo's** allowed and
  forbidden paths; a path outside every repo folder, the root itself and any `.git` are denied; the check after CLI runs
  uses the same policy on `<dir>/<file>` paths;
- after the run `recordJob` commits each repo that changed on its own branch (commit text in that repo's `pr_language`)
  and leaves the others untouched; if nothing changed anywhere the usual `error` is sent. The `result` event carries
  `repos`; its `branch`/`commit` are those of the first changed repo.

The job is checked again in the runner (folder must be `<owner>__<name>`, branch names, at most 10 repos). A job without
`repos` behaves exactly as before. This is `CONTRACT` 7.

## Tests

```bash
cd apps/agent/runner && npm install && npm test   # Node >= 26.13.1 (current LTS), no network; the SDK, `claude` and `codex` are stand-ins
```
