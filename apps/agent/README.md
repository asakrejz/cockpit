# Agent image

The Cockpit agent runs in its own container per run (Docker or Podman). This directory holds the image:

- `Containerfile`: Node 24, Git, the runner and the agent CLIs; runs as the user `node` (not root).
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
- Images from the time of numbered tags are no longer used. Remove them by hand if wanted (Cockpit never does):
  `docker image rm localhost/cockpit-agent:5`, or `docker image prune` for dangling layers (`podman` likewise).

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
| `result` | `text`, `branch`, `commit` | done; the commit is on the branch in the clone |
| `check` | `reachable`, `key_ok`, `hint` | result of the check mode |
| `error` | `text` | abort with a reason |

Lines arriving on stdin: `{"t":"answer","text":…}` and `{"t":"hint","text":…}`.

## Follow-up jobs

For a follow-up job on an existing pull request (image tag `:3` and later) `job.json` carries `followup`:
`{kind: "review_comments" | "failed_checks", pull_request, threads: [{path, line, comments: [{author, text}]}],
notes: [{author, state, text}], checks: [{name, excerpt}]}`; `branch` is the existing `claude/…` branch, already
checked out in `/work`. The runner puts the texts into the prompt as data of third parties, limits them again
(`LIMITS` in `job.ts`) and commits with a message that names the task. Everything else (events, no push, rules) is
unchanged; old jobs without `followup` work as before.

## Rules in the container

The container has **no GitHub token**: it cannot push. It commits to a branch `claude/<ticket>-…` in the mounted
clone; the backend checks and pushes afterwards (path rules of the repo, never the main branch, never a merge).
The runner additionally blocks writing outside `/work`, `.git`, forbidden paths and commands such as `git push` and
`curl` (see the adapters above for how each tool enforces it). The agent asks questions through the tool `ask_user`.

## Tests

```bash
cd apps/agent/runner && npm install && npm test   # Node >= 22.18, no network; the SDK, `claude` and `codex` are stand-ins
```
