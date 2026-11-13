# Agent image

The Cockpit agent runs in its own container per run (Docker or Podman). This directory holds the image:

- `Containerfile`: Node 24, Git and the runner; runs as the user `node` (not root).
- `runner/`: the runner (TypeScript, no build step: Node executes `.ts` directly). It reads the job from
  `/job/job.json` and executes it with an **adapter** (see below).

Build by hand (the backend does the same through "Image bauen" in the setup assistant):

```bash
docker build -f apps/agent/Containerfile -t localhost/cockpit-agent:2 apps/agent   # or: podman build …
```

Rules for the `Containerfile` (they hold for Docker and Podman alike): a fully qualified base image, no BuildKit
features (`# syntax=`, `RUN --mount`), the runner never works as root. See "Container rules" in
[../../docs/architecture.md](../../docs/architecture.md#container-rules).

## Adapters

Every agent tool is an adapter in `runner/src/adapter/` with the shared interface from `interface.ts`: job in (plus a
sink, Git, a stdin input and the environment), JSON-line events out (table below), exit code as the result.
`COCKPIT_PROVIDER` (default `claude-agent-sdk`; set in the image) selects the adapter; unknown and not yet implemented
names produce an `error` event in plain language.

| Adapter | State | Tool |
|---------|-------|------|
| `claude-agent-sdk` | implemented (`adapter/claude-agent-sdk.ts`) | Claude Agent SDK; model through `ANTHROPIC_MODEL` |
| `claude-code` | **open** | `claude -p --output-format stream-json`: only translate its lines (assistant messages, tool calls, result with usage) into the events below |
| `codex` | **open** | `codex exec --json`: translate its events (messages, commands, file changes, usage) into the same events |

A new adapter does not need to change anything in the backend: same event shapes, same rules (no push, write only to
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

## Rules in the container

The container has **no GitHub token**: it cannot push. It commits to a branch `claude/<ticket>-…` in the mounted
clone; the backend checks and pushes afterwards (path rules of the repo, never the main branch, never a merge).
`canUseTool` in the runner additionally blocks writing outside `/work`, `.git`, forbidden paths and commands such as
`git push` and `curl`. The agent asks questions through the tool `ask_user`.

## Tests

```bash
cd apps/agent/runner && npm install && npm test   # Node >= 22.18, no network, the SDK is a stand-in
```
