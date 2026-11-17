# Configuration reference

Everything here is optional. The setup assistant ("Einrichten") writes most of it for you; this page is the
reference for editing by hand. Back to the [README](../README.md).

## config.toml and environment variables

Everything is optional. The file `config.toml` lives in the folder `config/` of the project, next to the database
`cockpit.db` (`config/config.toml`, `config/cockpit.db`). "Project folder" is the working directory of the server, the
same folder in which `.env` is searched; `scripts/start.sh`, `start-cockpit.command` (also in the release archive) and
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
| `COCKPIT_ENV_FILE` | path of the `.env` file (`.env` in the working directory) |
| `COCKPIT_SEED` | `empty` or `demo` |
| `COCKPIT_CONTAINER` | `docker` or `podman` |
| `COCKPIT_AGENT_DIR` | folder with `Containerfile` and runner |
| `COCKPIT_GIT_BASE` | git base for clones (tests, local remotes) |
| `COCKPIT_DEV_PORT` | allow the `trunk serve` port (environment only, see [Developing with `trunk serve`](#developing-with-trunk-serve)) |

```toml
port = 8787
seed = "empty"                    # or "demo", see "Seed data"
interval_s = 120                  # sync interval, default 2 minutes, at least 30

[github]
repos = ["acme/app", "acme/web"]  # owner/name, several allowed; api_url only for GitHub Enterprise
[jira]
site = "https://acme.atlassian.net"
email = "me@acme.example"
[agent]
provider = "claude-agent-sdk"     # or "claude-code" or "codex"
container = "podman"              # or "docker"; detected when omitted
repo = "acme/app"                 # default repo of the agent (optional, see "Several repos")
[[agent.repos]]                   # the repos the agent may work in; a Jira project goes to one repo
repo = "acme/web"
base_branch = "develop"           # optional, default "main"
projects = ["WEB", "SHOP"]        # Jira projects whose tickets go to this repo
[[agent.repos]]
repo = "acme/app"                 # without projects: only a choice
[agent.endpoint]                  # model access, see "Model provider and endpoint"
url = "https://litellm.example"   # empty or omitted = directly at Anthropic
model = "claude-sonnet-5-5"
[server]                          # optional webhooks, see [setup-guide.md](setup-guide.md#live-updates-and-webhooks)
webhooks = false                  # default: both webhook routes answer 404
public_url = "https://cockpit.example"   # your tunnel; only shown in the assistant
[sync]
webhook_fallback_interval_s = 600 # poll interval of a source while its webhooks arrive (default: 5x interval_s, at most 600)
```


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
are looser than `700`/`600` (at start they are tightened). CI (`scripts/check-tracked-files.sh`, job `no-env-files`)
fails when anything but `config/config.example.toml` is tracked there. The folder never enters the agent image: the
build context is `apps/agent`, and its `.dockerignore` excludes `config` as well. The release archive contains only
`config/config.example.toml`.

## Several repos

`[[agent.repos]]` lists the repos the agent may work in (owner/name, base branch, Jira
projects). When a run starts, the backend picks the repo: the first rule whose `projects` names the ticket's Jira
project, otherwise the default (`[agent] repo`, or the only repo). In the agent view the repo is preselected the same way
and can be changed in a drop-down before the start; without a matching rule and without a default there is a hint
and the start waits for your choice. A repo outside the list is refused. Without `[[agent.repos]]` the default applies (`[agent] repo`, or the only `[github] repos` entry; with several `[github] repos` and no default
you choose at every start). A project may belong to one repo only. In the assistant "Repos prüfen" looks at each repo
read-only (`GET /repos/{owner}/{name}`: exists, visible, push allowed) – it changes nothing. Pull requests are
identified by `owner/name#number`: equal numbers from two repos stay separate in the inbox, review, agent view,
log and notifications; addresses look like `#/review/acme/app/87`.

## Seed data

The server starts with an empty workspace. `COCKPIT_SEED=demo` (or `seed = "demo"`) loads sample data, **only if no state
has been stored yet**; for a fresh seed use a new database (`COCKPIT_DB=/path/to/new.db`). Sample data otherwise serves as
test data and for the Playground stories.

## Credentials: the `.env` file

All keys (Jira token, GitHub token, key of the agent endpoint, client secrets, sign-in tokens) live in a `.env`
file in the project folder. The risk of a file inside the repository folder was a deliberate choice. **It is never
committed**: `.gitignore` excludes `.env` and `.env.*`; only `.env.example` (dummy values only) belongs in the repo.

- **Create:** the assistant and `cockpit-server token set <name>` create the file when needed (mode `600`). By
  hand: `cp .env.example .env` and fill in the values.
- **Location:** `.env` in the server's working directory (the repo folder; `scripts/start.sh` and
  `start-cockpit.command` change into it). `COCKPIT_ENV_FILE=/path/to/file` redirects it (operation, tests). The
  assistant shows the path in the last step.
- **Format:** `NAME=value`, comments with `#`, other lines are preserved on write. Values with special
  characters go in single quotes. The sign-in token blobs are compact one-line JSON (`'{"access":"…"}'`). Writing is
  atomic (temporary file, then rename).
- **Precedence:** a real environment variable of the same name wins over the file. A value set that way cannot be
  overwritten or deleted in the assistant.
- **Variables:**

  | Name in the backend | Variable in `.env` |
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
  when Git tracks the `.env`, `.gitignore` does not cover it, or the permissions are looser than `600` (at start they
  are set to `600`). CI fails when any `.env*` file other than `.env.example` is tracked. If a real `.env` was ever
  committed: rotate the keys immediately and run `git rm --cached .env`.
- **Container:** the `.env` never enters the image or container (`apps/agent/.dockerignore`); the agent receives
  values only as `-e NAME` from the backend.

Terminal commands (the value always comes from standard input, never from the command line):

```bash
echo "$TOKEN" | cargo run -p cockpit-server -- token set github     # or jira, github-oauth, atlassian-oauth, agent-key
cargo run -p cockpit-server -- token delete github                  # remove a key
cargo run -p cockpit-server -- token import                         # copy old keychain entries into .env, once
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

## Developing with `trunk serve`

`apps/cockpit/Trunk.toml` forwards `/api` to `http://127.0.0.1:8787` (`[[proxy]]`). Server and dev server together:

```bash
COCKPIT_DEV_PORT=8081 COCKPIT_SEED=demo cargo run -p cockpit-server   # terminal 1
(cd apps/cockpit && trunk serve --open)                               # terminal 2, http://localhost:8081
```

Trunk sends `Host` without the port and keeps the `Origin` of the dev page; the server allows both only when
`COCKPIT_DEV_PORT` is set (environment variable only, never in the file). Without it the strict check in
[docs/security.md](security.md) applies.

## Start at login (launchd, macOS)

`scripts/de.cockpit.server.plist` is a template for a LaunchAgent. **Nothing is installed automatically.**

1. Run `scripts/start.sh` once by hand so that everything is built.
2. Copy the template to `~/Library/LaunchAgents/de.cockpit.server.plist` and replace the placeholders
   `/PATH/TO/REPO` (the repo folder) and `/Users/YOUR_USER` (your home directory); see the comment in the file.
3. Load: `launchctl bootstrap gui/$(id -u) ~/Library/LaunchAgents/de.cockpit.server.plist`,
   unload: `launchctl bootout gui/$(id -u) ~/Library/LaunchAgents/de.cockpit.server.plist`.
   The log is at `~/Library/Logs/cockpit-server.log`.
