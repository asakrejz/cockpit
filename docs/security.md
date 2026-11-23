# Cockpit security model

Cockpit runs on one person's computer and talks to Jira, GitHub, a model endpoint and a container runtime. This page
lists what protects the credentials and what limits the agent. Depth on the rest: [architecture.md](architecture.md).

## Local files in `config/`

Configuration and database (`config/config.toml`, `config/cockpit.db`) hold addresses, accounts and workspace data, no
keys, but are still private to this computer. `.gitignore` excludes everything in `config/` except
`config/config.example.toml`; CI fails when anything else there is tracked; the server creates the folder with mode
`700` and files `600`, warns at start and in `/api/health` (`config_files`) about tracked, unignored or too open files.
Details: [configuration.md](configuration.md).

## Credentials

- **Where they live.** Only in the backend, in the file `config/.env` (decision of 2026-11-13; the risk
  of a file inside the repo folder is accepted knowingly). Never in the repo, the database, logs, `config/config.toml`, URLs
  or browser storage (`localStorage`).
- **Never committed.** `.gitignore` excludes `config/*` and `.env` / `.env.*` anywhere; no `.env*` file belongs in the repo. CI fails when any
  `.env*` file is tracked. At start and in `/api/health` (`env_file`)
  the server warns when Git tracks the file, `.gitignore` does not cover it or the permissions are looser than `600`; at
  start it sets them to `600`. If a real `.env` was ever committed: rotate the keys and run `git rm --cached config/.env`.
- **Two ways in.** `cockpit-server token set <name>` in the terminal (the value comes from standard input, so it is
  neither in the process list nor in the shell history), or the password fields of the setup assistant, which send the
  value once over loopback to `PUT /api/secrets/{name}`. For that route: only the fixed names listed in
  [configuration.md](configuration.md#credentials-the-env-file) (plus the two webhook secrets); the answer is only `{name, set}`, never the value;
  the field is cleared right after submitting; no log, no `localStorage`, no URL, no error message with the value.
  `DELETE /api/secrets/{name}` ("Trennen") exists for `jira` and `github` only. The tokens of OAuth sign-ins are written
  by the backend itself.
- **Precedence.** A real environment variable of the same name wins over the file; a value set that way cannot be
  overwritten or deleted in the assistant.
- **Old keychain entries** (before 2026-11-13): `cockpit-server token import` copies them once; it deletes nothing, does
  not overwrite and never runs by itself.
- **Jira API token** has the rights of the Atlassian account: while Cockpit should only read, use an account or role
  without write permissions and a short expiry. GitHub tokens should be fine-grained and limited to the configured
  repos (see the rights table in the [setup guide](setup-guide.md#github)).

## Requests to the backend

- The server listens only on `127.0.0.1`. The one exception is the container image, see
  [Running in a container](#running-in-a-container).
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

## Running in a container

The image ([container.md](container.md)) sets `COCKPIT_BIND=0.0.0.0`, because inside a container `127.0.0.1` is
unreachable from the host. Outside a container the default stays `127.0.0.1`; the variable exists only in the
environment (never in `config.toml`). With a non-loopback `COCKPIT_BIND` the server logs a warning.

- **The host side protects, so publish only on `127.0.0.1`:** `-p 127.0.0.1:8787:8787`. A port published on `0.0.0.0`
  (`-p 8787:8787`) is reachable from your network, and then the checks below no longer keep strangers out: the `Host`
  header can be set by anyone.
- With port publishing the peer is the gateway of the container network, not loopback. In this mode the peer check
  therefore accepts loopback and private addresses (`10/8`, `172.16/12`, `192.168/16`, link-local, IPv6 unique-local)
  and rejects public ones. Any other container on the same container network could therefore call `/api` as well.
- The **`Host` check is unchanged** (`127.0.0.1:<port>` or `localhost:<port>`, against DNS rebinding), as are the
  `Origin` and JSON content-type rules for writing requests. If the published host port differs from the container
  port, `COCKPIT_PUBLIC_PORT` adds that port to the accepted ones (environment only).
- The webhook routes keep requiring a loopback peer, so they do not work through a published container port.
- Keys stay in `config/.env` inside the volume; none is baked into the image or passed as a build argument. The
  server process runs as the unprivileged user `10001`: the entrypoint starts as root only to read the group of a mounted
  socket, then drops with `setpriv` (`--no-new-privs`, supplementary group = the socket's group, or `0` for the
  `root:root 660` socket of Docker/Rancher Desktop on macOS; container.md explains what that does and does not mean).
  `cockpit-server` in the image drops root too, so `docker exec` never runs the server as root.
- **Mounting the container socket** (`-v /var/run/docker.sock:/var/run/docker.sock`, for agent runs) gives Cockpit
  root-equivalent access to the host: it could start a privileged container with the whole disk mounted. It is opt-in,
  off by default, and meant only for a machine you trust. Agent runs need no host folder: the working folders are copied
  into the agent container and the result back (`docker cp`), so Cockpit never hands paths of its own container to the engine.
- The workflow publishes to a private package and deletes all other versions after each push, so no old image with
  old code lingers. Reading it needs a token with `read:packages`.

## Jira attachments (`GET /api/jira/attachment/{id}`)

Jira's attachment addresses need a sign-in. The browser therefore never gets them: the backend reads the file
with the key it already has and hands out only the bytes.

- **Read only, and only from this computer.** The routes `/api/jira/attachment/{id}` and
  `/api/jira/attachment/{id}/thumbnail` are `GET` routes under `/api`, so the loopback and `Host` checks apply like
  everywhere else. No key, token or Jira address appears in an answer.
- **Only attachments of tickets in the workspace.** The ID must be digits and must belong to an attachment of a ticket the
  sync has loaded; anything else is `404` and Jira is not asked. The address at Jira is built from the digits of the ID
  only (`/rest/api/3/attachment/content/<id>` on the site the key belongs to), never from a URL taken from the data.
- **Size limit.** 10 MB per file and 2 MB per thumbnail, checked on `Content-Length` and while reading; a larger file is
  `413`. A redirect to Jira's media service is followed, but the key is not sent to another host.
- **Only real pictures are shown inline.** The first bytes decide, not the file name or the type Jira reports:
  PNG, JPEG, GIF and WebP are served inline. Everything else – including SVG, HTML and PDF – is
  `application/octet-stream` with `Content-Disposition: attachment` (the file name is cleaned of quotes, slashes and
  non-ASCII characters).
- **Every answer** carries `X-Content-Type-Options: nosniff` and `Content-Security-Policy: sandbox; default-src 'none'`,
  so a file that is opened by itself can run nothing.
- **Cache with a bound.** Files are kept in memory only (at most 32 MB in total, oldest first, nothing on disk) and
  vanish when the program ends.
- The link to a ticket (`<site>/browse/<KEY>`) and to a pull request is a plain link that opens in a new tab with
  `rel="noopener noreferrer"`; only `http(s)` addresses become links.

## Webhooks (optional, internet-facing)

The two routes `POST /api/webhooks/github` and `POST /api/webhooks/jira` are the only part of the server meant to be
reached from the internet, through a tunnel the user runs (see the [setup guide](setup-guide.md#live-updates-and-webhooks)). They are
off by default (`[server] webhooks = false`: `404`).

- **Only these two routes** skip the `Host` check (a tunnel forwards its public name); they sit outside the `access`
  layer. Everything else (`/api/workspace`, `/api/actions`, `/api/secrets/*`, `/api/events`, `/api/setup/*` including
  the secret generator and the test button, unknown paths below `/api/webhooks/`) still needs loopback peer and
  loopback `Host`; tests send every route with foreign hosts and expect `403`. The peer must still be loopback (the
  tunnel client runs on the same machine).
- **Authentication:** HMAC-SHA256 over the raw body with a secret from `config/.env` (`GITHUB_WEBHOOK_SECRET`,
  `JIRA_WEBHOOK_SECRET`), compared in constant time. GitHub: `X-Hub-Signature-256`. Jira: `X-Hub-Signature`; for
  senders that cannot sign, the secret as `?token=` (compared in constant time). The token variant is weaker: it
  appears in the URL (tunnel and Jira logs, history), is the same on every request, and does not bind the body, so
  anyone who sees one request can send others. Prefer the signature. If a signature header is present it alone
  decides. No secret: `404`, nothing is accepted (an empty secret counts as none).
- **Only a trigger.** The payload is not parsed beyond the event type (Jira: `webhookEvent`; GitHub: header) and
  is never stored or used as data. A forged or replayed request can at most cause a sync, which asks the services
  with the user's own credentials and applies the result through the normal merge. The sync is merged (one run per
  source per 5 s) and honours the rate-limit pause of PR #43.
- **Replay.** A delivery ID (`X-GitHub-Delivery`, `X-Atlassian-Webhook-Identifier`) or an identical body seen in the
  last 10 minutes is answered `202` and ignored (at most 512 remembered, memory only). The ID header is not
  signed, therefore the body hash is checked as well. A captured signed request can therefore not be replayed
  more than once per 10 minutes, and even then it only triggers a sync.
- **Size and rate.** Body at most 1 MiB (`413`), only `application/json` (`415`), at most 120 requests per source per
  minute counting invalid ones (`429`); the secret is read and the signature checked only after these cheap checks.
- **No details out, nothing in logs.** Failures are plain status codes with an empty body. Logs name the source and
  at most the event type (letters, digits and `_ : . -`, 64 characters), never headers, URL, body or secrets.
- **Secrets.** Generated locally (32 random bytes from the system, 64 hex characters) via the loopback-only
  `POST /api/setup/webhook-secret/{source}`; the value is in that one answer and nowhere else (not in `/api/setup`,
  logs or errors). Renewing replaces the old one.
- **Local test.** `POST /api/setup/webhook-test/{source}` sends a signed request to `127.0.0.1:<port>`; it calls no
  external service and never starts a sync. Its event is shown as "test (lokal)".
- **Tunnel hygiene.** Forward only `/api/webhooks/*` where the tunnel allows it, stop it when unused, treat the
  public address as public. Not covered: a leaked secret (anyone with it can trigger syncs – renew it) and denial of
  service on the tunnel itself.

## The agent

- **No GitHub token in the container.** The agent cannot push. It commits to a branch in the mounted clone, named by the
  repo's `branch_pattern` (`claude/<ticket>-…` by default, see [configuration.md](configuration.md#branch-names)). The
  backend checks the branch in a fresh bare repo (never in the clone the container wrote to) against the repo's
  path rules and then pushes it. **Never** the main branch, **never** a force-push, **never** a merge. You create the pull
  request yourself.
- **Branch names are enforced, not suggested.** The pattern comes from `config.toml` only; whatever a request or the
  container says is replaced or ignored. Before the clone and again before the push the backend requires that the branch
  is exactly what the pattern produces (a valid name, at most 120 characters, letters, digits and `/ - _ .`). It refuses
  `main`, `master`, `HEAD`, `develop`, `development`, `trunk`, `production`, `prod`, `staging`, the repo's base branch
  and anything under `refs/`, **whatever the pattern is**. A pattern must not be empty, needs `{ticket}` (so it never
  names one fixed branch) and is validated when the config is read and when the assistant saves it. A new run also
  refuses a branch that **already exists** on GitHub (protected or somebody else's), checked with `git ls-remote` before
  the clone and again right before the push; the push itself is a plain push, never forced. A follow-up on a pull request
  may only continue a branch that fits the repo's pattern or lives under `claude/` (pull requests from before a change
  keep working). The proposal "Aus Richtlinien vorschlagen" only reads `CONTRIBUTING.md` and saves nothing; its text is
  data for pattern matching, never instructions. The runner hint to follow the commit conventions of such a file is
  advice to the agent and never loosens the fixed rules.
- **Reviews by the agent only read, and only you send.** "PR reviewen" starts a run whose clone of the pull request's
  branch is mounted read-only (`:ro`); the agent has only reading tools (Codex: `read-only` sandbox), no GitHub token,
  and cannot push or merge. The repo must be configured for the agent like for any run. Title, description and diff are
  given to the model as data of third parties, with a size limit. What the agent finds comes back as **drafts**: the
  backend checks each draft against the pull request's diff (file, line, side, size, count) and drops what does not fit.
  Drafts stay on your machine. Nothing goes to GitHub until you decided every draft and click "Review absenden"; then
  **one** review goes out with the drafts you approved (with the text you edited) and your own line comments, never
  the declined or undecided ones. The verdict stays "Kommentieren" (`COMMENT`) unless you pick "Genehmigen" or
  "Änderungen" yourself – the agent cannot set it. The agent's summary also stays local; it goes out only if you copy it
  into your own comment.
- **Deleting a branch happens only on your click, one branch at a time.** After a pull request of a run is merged or
  closed, "Branch löschen" can delete the agent's branch on GitHub (`DELETE /repos/{repo}/git/refs/heads/{branch}`). The
  backend refuses unless all of this holds, read from GitHub at the click and not taken from the browser: the pull
  request is merged or closed; the branch matches the repo's `branch_pattern`; it is not the base branch, the default
  branch of the repo, a main-branch name or anything under `refs/`; GitHub does not mark it protected; it is the head
  of that pull request and the branch the run pushed (so a run can never name another branch); the run is a real
  implementation run of the Cockpit (not a review of somebody else's pull request). Marking a run "erledigt" only reads
  the pull request; it deletes no branch. "Aufräumen" shows a preview first and then removes only the listed items
  that are still orphans: containers with the label `cockpit.run` that belong to no working run (command line
  `ps --filter label=…` and `rm -f`, never the socket) and folders directly under `<data>/agent/runs`. It never
  touches the image, other containers or any other path.
- **Only the repos you chose, each on its own.** A run clones and pushes only in the repos you chose when you started it
  (one or several, see [using-cockpit.md](using-cockpit.md#working-in-several-repos-at-once)). The backend checks the
  repos again at start and before every push and applies the branch pattern and path rules of **each** repo separately;
  a repo that fails a check is refused and the others are still pushed. The repo check in the assistant only reads
  (`GET /repos/{owner}/{name}`).
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
  environment, so no key from your shell ends up in the container. `config/.env` never enters the image or the container
  (`apps/agent/.dockerignore`).
- **Limits.** Budget per run and per month, a time limit and a limit on parallel runs apply; when reached the container
  is ended with `rm -f` and the run waits stopped for you (the clone stays so it can continue). Cancel, time limit,
  container errors and a server restart (orphaned containers are removed by label, runs marked as cancelled) appear in
  plain language in the log and at the run.
- **Container runtime access** is by command-line calls only; the Docker socket is never used (see the container rules in
  [architecture.md](architecture.md#container-rules)).

## Not verified against real services

See [architecture.md](architecture.md#development). Treat the first real run with care and use a test account or read-only rights.
