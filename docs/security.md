# Cockpit security model

Cockpit runs on one person's computer and talks to Jira, GitHub, a model endpoint and a container runtime. This page
lists what protects the credentials and what limits the agent. Entry page: [../README.md](../README.md); depth on the
rest: [architecture.md](architecture.md).

## Credentials

- **Where they live.** Only in the backend, in the `.env` file in the project folder (decision of 2026-11-13; the risk
  of a file inside the repo folder is accepted knowingly). Never in the repo, the database, logs, `config.toml`, URLs
  or browser storage (`localStorage`).
- **Never committed.** `.gitignore` excludes `.env` and `.env.*`; only `.env.example` with dummy values belongs in the
  repo. CI fails when any `.env*` file other than `.env.example` is tracked. At start and in `/api/health` (`env_file`)
  the server warns when Git tracks the file, `.gitignore` does not cover it or the permissions are looser than `600`; at
  start it sets them to `600`. If a real `.env` was ever committed: rotate the keys and run `git rm --cached .env`.
- **Two ways in.** `cockpit-server token set <name>` in the terminal (the value comes from standard input, so it is
  neither in the process list nor in the shell history), or the password fields of the setup assistant, which send the
  value once over loopback to `PUT /api/secrets/{name}`. For that route: only the fixed names `github-oauth`,
  `atlassian-oauth`, `agent-key` (older `litellm`), `jira`, `github`; the answer is only `{name, set}`, never the value;
  the field is cleared right after submitting; no log, no `localStorage`, no URL, no error message with the value.
  `DELETE /api/secrets/{name}` ("Trennen") exists for `jira` and `github` only. The tokens of OAuth sign-ins are written
  by the backend itself.
- **Precedence.** A real environment variable of the same name wins over the file; a value set that way cannot be
  overwritten or deleted in the assistant.
- **Old keychain entries** (before 2026-11-13): `cockpit-server token import` copies them once; it deletes nothing, does
  not overwrite and never runs by itself.
- **Jira API token** has the rights of the Atlassian account: while Cockpit should only read, use an account or role
  without write permissions and a short expiry. GitHub tokens should be fine-grained and limited to the configured
  repos (see the rights table in the README).

## Requests to the backend

- The server listens only on `127.0.0.1`.
- Every route under `/api` checks the peer (only loopback) and the `Host` header (`127.0.0.1:<port>` or
  `localhost:<port>`), which protects against DNS rebinding.
- Writing requests additionally need `Content-Type: application/json` and a missing or own `Origin`. This also applies
  to `PUT /api/config` and `PUT /api/secrets/{name}`.
- The OAuth callback is a browser navigation: loopback and host checks apply, a JSON content type is not required; the
  one-time `state` (valid 10 minutes) and PKCE protect the flow.
- `COCKPIT_DEV_PORT` relaxes the `Host`/`Origin` check for the `trunk serve` dev port. It is read from the environment
  only, never from `config.toml`.
- Endpoint calls of the backend (`GET <url>/v1/models`, connection tests) do not follow redirects and have a timeout and
  a size limit; they are only made when a key exists and they return IDs and booleans, never the key.

## The agent

- **No GitHub token in the container.** The agent cannot push. It commits to a branch `claude/<ticket>-…` in the mounted
  clone. The backend checks the branch in a fresh bare repo (never in the clone the container wrote to) against the repo's
  path rules and then pushes it. **Never** the main branch, **never** a force-push, **never** a merge. You create the pull
  request yourself.
- **One repo per run, only configured ones.** A run clones and pushes in exactly one repo. The repo comes from the
  routing rules or from your choice in the interface, and the backend refuses (at start and again when the container
  starts) any repo that is not in `[[agent.repos]]` / `[agent] repo` / `[github] repos`. The repo check in the assistant
  only reads (`GET /repos/{owner}/{name}`).
- **Approval.** Comments and reviews of the agent go out only after your approval. Only you may merge. A plan that
  touches forbidden paths cannot be approved.
- **Sandbox in the runner.** The agent writes only to `/work`; the runner blocks writing outside `/work`, `.git`,
  forbidden paths and commands such as `git push` and `curl`. How each tool enforces this:
  - *Claude Agent SDK:* `canUseTool` (`policy.ts`).
  - *Claude Code CLI:* restrictive `--allowedTools`/`--disallowedTools` (no web tools), `--permission-mode default`
    (never a bypass mode), a `PreToolUse` hook that applies the same `policy.ts` to every call and fails closed, no
    settings from the repo (`--setting-sources user`), no MCP servers (`--strict-mcp-config`).
  - *Codex CLI:* sandbox `workspace-write` (plan mode `read-only`) with `network_access=false` and approval policy `never`.
  - *All tools, after the run:* the runner checks every changed file (working tree and commits the agent made itself)
    against allowed and forbidden paths and `.git`; a violation aborts with an error and nothing is committed. The
    backend checks the branch once more in a fresh bare repo before pushing.

  **Limits of this.** Codex has no per-path hook, so a write to a forbidden path is only caught after the run (the
  change never leaves the container, but the run is lost). The command rules for Claude Code's `Bash` tool are regular
  expressions and can be circumvented by an adversarial model (`python -c`, shell tricks); what holds regardless is that
  the container has no GitHub token, `--cap-drop ALL`, and the backend's check before any push. Claude Code can still
  reach the network through `Bash` commands that the patterns do not know; the container's network is not restricted
  to the model endpoint (that needs a firewall in the container runtime, which Cockpit does not set up). Whether Codex's
  own sandbox works inside a container with dropped capabilities depends on the host kernel (it is unverified here).
  Codex cannot ask questions and neither CLI adapter takes hints while running.
- **CLIs in the image.** `@anthropic-ai/claude-code` and `@openai/codex` are installed with exact versions from
  `apps/agent/clis/package-lock.json` (`npm ci`), without a login or `.npmrc`; the image holds no key. The adapters start
  them with a clean environment (only the endpoint names of their family), pass the prompt through stdin and scrub key
  values from every event and error text.
- **Environment.** Endpoint address, key and model reach the container only as `-e NAME`, with the value in the child
  process environment, never in the argument list, `ps` or logs. Names that are not used are removed from the inherited
  environment, so no key from your shell ends up in the container. `.env` never enters the image or the container
  (`apps/agent/.dockerignore`).
- **Limits.** Budget per run and per month, a time limit and a limit on parallel runs apply; when reached the container
  is ended with `rm -f` and the run waits stopped for you (the clone stays so it can continue). Cancel, time limit,
  container errors and a server restart (orphaned containers are removed by label, runs marked as cancelled) appear in
  plain language in the log and at the run.
- **Container runtime access** is by command-line calls only; the Docker socket is never used (see the container rules in
  [architecture.md](architecture.md#container-rules)).

## Not verified against real services

Everything that needs real credentials (GitHub and Jira sync and writes, OAuth exchange and refresh, the model endpoint,
a real container run) has only been tested against recorded or mocked responses and local stand-ins. Treat the first
real run with care and use a test account or read-only rights.
