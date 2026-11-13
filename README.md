# Cockpit

Cockpit is a local dashboard that brings **Jira, GitHub and coding agents** together in one place:
an inbox of tickets and pull requests that need your attention, a review page with a diff, and an agent
that implements a ticket in a container and hands you a branch to review. It runs on your own computer, for
one person. Credentials never leave your machine.

The repository also contains the **Playground**, a Storybook-like page that shows every UI building block
on its own. Cockpit is the main application; the Playground is where the component library is developed
and learned.

- UI: [Leptos](https://leptos.dev) (Rust compiled to WebAssembly, runs entirely in the browser)
- Build tool: [Trunk](https://trunkrs.dev) (Rust → WASM → ready-made page)
- Backend: axum + SQLite (`apps/server`, binary `cockpit-server`)
- Look: the design "Baukasten" (iOS 26 / Liquid Glass), driven by design tokens
- Visible UI texts are German; code, naming and documentation are English.

Contents: [Quick start](#quick-start) · [Quick start for beginners](#quick-start-for-beginners) ·
[Setting up the agent](#setting-up-the-agent) · [Running the backend](#running-the-backend) ·
[Connecting real services](#connecting-real-services) · [Architecture](#architecture) ·
[Commands and checks](#commands-and-checks) · [Security model](#security-model) ·
[Container rules](#container-rules) · [The Cockpit app](#the-cockpit-app) ·
[The component library](#the-component-library-feature-ui) · [Roadmap / Open work](#roadmap--open-work)

More depth: [docs/architecture.md](docs/architecture.md) (decisions, API, sync, agent runs) and
[docs/security.md](docs/security.md) (credentials, request checks, agent sandbox).

## Quick start

For developers who already have a terminal and Rust. Everyone else: see the next section.

```bash
# once
rustup target add wasm32-unknown-unknown   # also happens automatically through rust-toolchain.toml
cargo install trunk --locked                # or: brew install trunk

scripts/start.sh --open                     # build the app if needed, start the server, open the browser
(cd apps/playground && trunk serve --open)  # Playground on http://localhost:8080
cargo test                                  # tests run natively, no browser needed
```

`scripts/start.sh` (bash, macOS) checks that `cargo` and `trunk` exist, builds `apps/cockpit` into
`apps/cockpit/dist` when the output is missing or a source is newer, and starts `cockpit-server` on
`http://127.0.0.1:8787`. It installs nothing. `--release` builds the server as a release build. Without the
script: `(cd apps/cockpit && trunk build) && cargo run -p cockpit-server`.

While learning, run `trunk serve` in `apps/playground`, change a file and save: the browser reloads and the
compiler tells you immediately what does not fit.

## Quick start for beginners

For everyone who has never used a terminal. You need about **one hour**, most of it waiting for downloads, a
Mac (Apple Silicon or Intel) and an internet connection. Pre-built archives are attached to releases in the
repository (see "Ready-made archive" below); this guide builds from source.

**1. Install the programs** (once; follow the instructions on each page):

| What | What for | Where | Time |
|------|----------|-------|------|
| **Rust** (via "rustup") | compiles Cockpit | [rustup.rs](https://rustup.rs): paste the one command shown there into the Terminal (Applications › Utilities › Terminal) | 5–10 min |
| **Trunk** | builds the UI | in the Terminal: `cargo install trunk --locked` (after Rust), or with [Homebrew](https://brew.sh) `brew install trunk` | 5–10 min |
| **Rancher Desktop** *or* **Podman** | container runtime for the agent (only needed later) | [rancherdesktop.io](https://rancherdesktop.io) (on first start choose the engine "dockerd (moby)"), or [podman.io](https://podman.io/docs/installation) | 10 min |
| **Git** | fetches the code | asks for itself on first use, otherwise run `xcode-select --install` in the Terminal | 5–10 min |

**2. Get Cockpit.** In the Terminal, one command at a time, confirming each with Enter:

```bash
git clone https://github.com/asakrejz/rust-playground.git ~/Documents/Web/rust-playground
```

The repository is private: you need access and have to sign in to GitHub (Git asks for it).

**3. Start with a double-click.** In Finder open `Documents/Web/rust-playground` and double-click
**`start-cockpit.command`**. The first time macOS asks whether you want to open the file (right-click › "Open" ›
"Open" again). A Terminal window opens, builds the app (5–15 minutes the first time, seconds afterwards) and
opens the browser at `http://127.0.0.1:8787`. Close the window to quit Cockpit.

**4. Follow the assistant.** On first start the page **"Einrichten"** (setup) opens by itself; later it is in the
sidebar. It guides you through five steps (container, Jira, GitHub, model access, summary) and explains every
click. You can skip any step and come back later.

**What only you can do** (it cannot be automated; the assistant walks you through each step):

- **Atlassian account and API token.** You need access to your Jira site. The easy way is an **API token**: create
  it yourself at [id.atlassian.com](https://id.atlassian.com/manage-profile/security/api-tokens) ("Create API
  token", pick a name and an expiry date, copy) and enter site, email and token in the assistant. Only if you
  prefer signing in in the browser, create your own OAuth app in the
  [Atlassian developer console](https://developer.atlassian.com/console/myapps/) (under "Erweitert"). Every person
  creates their **own** app because the client secret cannot be shared.
- **GitHub App.** Create your own GitHub App in your account (or organisation) and install it on the repos.
  Again: your own app, your own secret.
- **Access to a model.** Either an Anthropic API key (direct), or the address and key of a proxy such as LiteLLM
  that your team runs. Without access Cockpit keeps working, but the real agent does not.
- **Container runtime:** install and start it (table above).

Keys (client secrets, tokens, the key of the model endpoint) are typed or pasted into the password fields of the
assistant. They go only to the server on your computer, which stores them in the **`.env` file in the project
folder**; never in browser storage, never in a tracked file. If you prefer the terminal, use
`cockpit-server token set <name>` (see [Credentials: the `.env` file](#credentials-the-env-file)).

**Ready-made archive instead of building yourself.** For every version tag (`v…`) `.github/workflows/release.yml`
builds an archive for macOS (Apple Silicon) with `cockpit-server`, `apps/cockpit/dist`, the agent image sources
and `start-cockpit.command` in one folder, no Rust or Trunk needed. Unzip, right-click `start-cockpit.command` ›
"Open". The programs are not signed, hence the macOS prompt. Intel Macs, Windows and Linux build themselves
(steps above). The workflow has never run yet (there was no tag); check that the archive starts on the first tag.

## Setting up the agent

The agent implements a ticket in a **container** (Claude Agent SDK, one container per run). One-time setup:

1. **Start a container runtime:** Rancher Desktop with the engine "dockerd (moby)" (not "containerd") *or* Podman
   (`podman machine start`). The assistant ("Einrichten" › "Container") shows whether it runs.
2. **Build the image:** click "Image bauen" there (or `POST /api/agent/image`). The build downloads packages and
   takes a few minutes; progress and errors appear below in plain text. `/api/health` reports under `agent`
   whether the image exists (`image inspect`). By hand:
   `docker build -f apps/agent/Containerfile -t localhost/cockpit-agent:2 apps/agent` (or `podman`).
3. **Provider and endpoint:** in the "Modell" step choose the tool (Claude Agent SDK; Claude Code and Codex are
   shown as "soon"), the endpoint (directly at Anthropic, your own proxy such as LiteLLM, or another compatible
   endpoint), store the key (`.env`, name `agent-key`, variable `AGENT_KEY`) and pick the model from the list that
   "Modelle laden" fetches from the endpoint (free text as a fallback). "Aus dem Container testen" checks that a
   container can reach the endpoint. If the proxy is only reachable over a **VPN**, the container machine of
   Rancher Desktop or Podman may not know the VPN route or name; the test then says that your Mac reaches the
   proxy but the container does not. Connect the VPN first and restart Rancher Desktop or the Podman machine.
4. **Connect GitHub and name the repos:** the agent clones through the backend, one repo per run. If `[github]
   repos` has exactly one repo (or `[agent] repo` names one) it is the default. With several repos you can route
   Jira projects to repos (`[[agent.repos]]`, see "Several repos" below) – in the assistant: step "GitHub" ›
   "In welchen Repos arbeitet der Agent?". The folder `apps/agent/` is looked up next to `apps/cockpit/dist`
   (`[agent] dir` or `COCKPIT_AGENT_DIR` overrides).

If something is not set up, no run starts; the message says in plain language what is missing and where in the
assistant to fix it.

**Anatomy of a run.** "Agent starten" first (optionally) presents a plan from a short read-only container. After
your approval the backend clones the repo fresh, creates the branch `claude/<ticket>-…` and starts the container
(`run --rm`, label `cockpit.run=<key>`, `--add-host cockpit-host:host-gateway`, repo mounted as `-v <clone>:/work`,
runner without root; address, key and model only as `-e NAME`, so the values are in the process environment and
never in the argument list or the log). Plan, questions, usage and log arrive as JSON lines from the container
([apps/agent/README.md](apps/agent/README.md)) and are applied as actions of the agent; `cockpit-core` computes
costs from the prices in "Grenzen & Regeln". Answers and hints go back over stdin; "Pausieren" is
`pause`/`unpause`.

What the agent may and may never do is listed under [Security model](#security-model).

## Running the backend

The backend (`apps/server`, binary `cockpit-server`) is a local service for one person. It listens only on
`127.0.0.1`, serves the built Cockpit app and holds data (SQLite) and keys (`.env` file in the project folder);
keys never leave the backend. **The app only works with the server**: there is no mode without a backend.

```bash
scripts/start.sh                # build if needed and start: http://127.0.0.1:8787
scripts/start.sh --open         # additionally open the browser
curl localhost:8787/api/health  # {"status":"ok","version":"…","connections":{…},"sync":{…},"endpoint":{…},"container":{…},…}
```

Without a terminal, a double-click on `start-cockpit.command` does the same (`--open`) and shows errors in plain
language.

**Everything is empty on first start.** The server begins with an empty workspace; the "Einrichten" page shows
the state and next step per service. Sample data exists only on request: `COCKPIT_SEED=demo` (or
`seed = "demo"` in `config.toml`) loads it, **but only if no state has been stored yet**; an existing state (even
an empty one changed by actions) wins. For a fresh seed use a new database (`COCKPIT_DB=/path/to/new.db`).
Sample data (`crates/cockpit-core/src/demo.rs`) otherwise serves as test data and for the Playground stories.

```bash
COCKPIT_SEED=demo COCKPIT_DB=/tmp/cockpit-demo.db scripts/start.sh --open   # sample data, own database
```

### Configuration

Everything is optional. The file `config.toml` lives in the configuration directory
(`~/.config/cockpit/` on Linux, `~/Library/Application Support/cockpit/` on macOS). Environment variables override
the file:

| Variable | Meaning (default) |
|----------|-------------------|
| `COCKPIT_PORT` | port (8787) |
| `COCKPIT_DIST` | built app (`apps/cockpit/dist`) |
| `COCKPIT_DB` | SQLite file (`cockpit/cockpit.db` in the user's data directory) |
| `COCKPIT_CONFIG` | path of `config.toml` |
| `COCKPIT_ENV_FILE` | path of the `.env` file (`.env` in the working directory) |
| `COCKPIT_SEED` | `empty` or `demo` |
| `COCKPIT_CONTAINER` | `docker` or `podman` |
| `COCKPIT_AGENT_DIR` | folder with `Containerfile` and runner |
| `COCKPIT_GIT_BASE` | git base for clones (tests, local remotes) |
| `COCKPIT_DEV_PORT` | allow the `trunk serve` port (environment only, see below) |

```toml
port = 8787
seed = "empty"                    # or "demo", see above
interval_s = 120                  # sync interval, default 2 minutes, at least 30

[github]
repos = ["acme/app", "acme/web"]  # owner/name, several allowed; api_url only for GitHub Enterprise
[jira]
site = "https://acme.atlassian.net"
email = "me@acme.example"
[agent]
provider = "claude-agent-sdk"     # implemented; "claude-code" and "codex" are known but not implemented
container = "podman"              # or "docker"; detected when omitted
repo = "acme/app"                 # default repo of the agent (optional, see "Several repos")
[[agent.repos]]                   # the repos the agent may work in; a Jira project goes to one repo
repo = "acme/web"
base_branch = "develop"           # optional, default "main"
projects = ["WEB", "SHOP"]        # Jira projects whose tickets go to this repo
[[agent.repos]]
repo = "acme/app"                 # without projects: only a choice
[agent.endpoint]                  # model access, see "Setting up the agent"
url = "https://litellm.example"   # empty or omitted = directly at Anthropic
model = "claude-sonnet-5-5"
```

**Several repos.** `[[agent.repos]]` lists the repos the agent may work in (owner/name, base branch, Jira
projects). When a run starts, the backend picks the repo: the first rule whose `projects` names the ticket's Jira
project, otherwise the default (`[agent] repo`, or the only repo). In the agent view the repo is preselected the same way
and can be changed in a drop-down before the start; without a matching rule and without a default there is a hint
and the start waits for your choice. A repo outside the list is refused. Without `[[agent.repos]]` everything
works as before (`[agent] repo`, or the only `[github] repos` entry; with several `[github] repos` and no default
you choose at every start). A project may belong to one repo only. In the assistant "Repos prüfen" looks at each repo
read-only (`GET /repos/{owner}/{name}`: exists, visible, push allowed) – it changes nothing. Pull requests are
identified by `owner/name#number`: equal numbers from two repos stay separate in the inbox, review, agent view,
log and notifications; addresses look like `#/review/acme/app/87` (the older `#/review/87` still opens the first
match).

Older files keep working: `[litellm] url/model` and the key `litellm` are read as aliases for
`[agent.endpoint]` and `agent-key` (the assistant writes the new format when saving). The older German names
`intervall_s`, `[agent] anbieter`, `[agent.endpunkt]` with `modell`, `COCKPIT_GIT_BASIS` and the seed `leer` are
read as well.

### Credentials: the `.env` file

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

`token import` reads the system keychain (where keys lived before 2026-11-13), deletes nothing there, overwrites
nothing that already has a value and never runs by itself. The older spellings `setzen`, `loeschen` and
`uebernehmen` are still accepted.

### Developing with `trunk serve`

`apps/cockpit/Trunk.toml` forwards `/api` to `http://127.0.0.1:8787` (`[[proxy]]`). Server and dev server together:

```bash
COCKPIT_DEV_PORT=8081 COCKPIT_SEED=demo cargo run -p cockpit-server   # terminal 1
(cd apps/cockpit && trunk serve --open)                               # terminal 2, http://localhost:8081
```

Trunk sends `Host` without the port and keeps the `Origin` of the dev page; the server allows both only when
`COCKPIT_DEV_PORT` is set (environment variable only, never in the file). Without it the strict check in
[docs/security.md](docs/security.md) applies.

### Start at login (launchd, macOS)

`scripts/de.cockpit.server.plist` is a template for a LaunchAgent. **Nothing is installed automatically.**

1. Run `scripts/start.sh` once by hand so that everything is built.
2. Copy the template to `~/Library/LaunchAgents/de.cockpit.server.plist` and replace the placeholders
   `/PFAD/ZUM/REPO` (the repo folder) and `/Users/BENUTZER` (your home directory); see the comment in the file.
3. Load: `launchctl bootstrap gui/$(id -u) ~/Library/LaunchAgents/de.cockpit.server.plist`,
   unload: `launchctl bootout gui/$(id -u) ~/Library/LaunchAgents/de.cockpit.server.plist`.
   The log is at `~/Library/Logs/cockpit-server.log`.

## Connecting real services

**The easy way is the assistant:** the "Einrichten" page walks through container, Jira, GitHub and model access.
It writes client IDs, Jira site, repos, agent provider and endpoint into `config.toml` and stores keys in `.env`,
without you touching a file or the terminal. **The terminal route stays available** as an alternative (edit
`config.toml`, run `cockpit-server token set <name>`); both lead to the same result and can be mixed.

Without setup the workspace stays empty. As soon as **a source is configured and its token is in `.env`**, the
backend syncs: tickets, pull requests, comments, checks and diffs come from the services; the (unchanged)
sample data of a seed disappears at the first successful fetch. A source without configuration or token stays off.
What is synced and which actions go out is described in [docs/architecture.md](docs/architecture.md).

### Jira

**Default: API token (three entries).** In the app under "Einrichten" › "Jira" enter (1) the address of your Jira
site (`acme.atlassian.net`; a pasted address with `https://` and a path is understood), (2) the email of your
Atlassian account, (3) the token. "Token-Seite öffnen" opens `https://id.atlassian.com/manage-profile/security/api-tokens`
in a new tab: create a token, pick a name and expiry, copy and paste it into the password field. The token goes to
`PUT /api/secrets/jira` (into `.env`), site and email to `PUT /api/config` (into `config.toml`). "Verbindung prüfen"
(`POST /api/jira/check`, read only: `GET /rest/api/3/myself`) then tests whether "Jira antwortet und dich kennt"
and otherwise names the cause in plain language (wrong address, token rejected, no permission, unreachable). Afterwards the
assistant suggests the Jira projects to choose from (or "Alle Projekte"), like the drop-down in the inbox. The
summary shows whether the sign-in is "mit API-Token" or "mit OAuth"; "Token erneuern" returns to the step and
"Trennen" deletes the token from `.env` (`DELETE /api/secrets/jira`).

**Security.** The token has the rights of your Atlassian account. As long as Cockpit should only read, use an
account or role without write permissions; a short expiry limits the damage. The token lives only in `.env` and never
in browser storage, URL, database, `config.toml` or log.

**Choosing a project.** If the Jira site has several projects (boards), pick one in the inbox drop-down "Jira-Projekt".
The list comes from Jira (`GET /api/jira/projects`, behind it `GET /rest/api/3/project/search`: all projects your token
may see) plus the projects of the tickets already present. The choice is the action `SelectJiraProject` and is
stored. With a project selected the inbox shows only its tickets (pull requests stay visible, they belong to no
project); the sync fetches **all open tickets of the project** (`project = KEY AND statusCategory != Done`), not only
yours (what is neither assigned to nor watched by you appears under "Team" with "Im Projekt"); and the choice takes
precedence over `projects` and `jql` from `config.toml`. "Alle Projekte" restores those. After a change the server
syncs immediately.

**Terminal route.** `config.toml`:

```toml
[jira]
site = "https://acme.atlassian.net"
email = "me@acme.example"
projects = ["BAU", "WEB"]          # optional: assigned to me or watched, open, only in these projects
# jql = "project = BAU AND labels = cockpit ORDER BY updated DESC"   # optional: own query instead of the projects
```

Without `projects` and `jql` Jira returns all open tickets assigned to or watched by you. For GitHub the pull
requests of the listed repos come in where a review of you is requested or that belong to you.

### GitHub

The token (`echo "$GITHUB_TOKEN" | cargo run -p cockpit-server -- token set github`) is read anew on every use, so a
new token works without restart.

| Service | Token | Rights |
|---------|-------|--------|
| GitHub | *fine-grained personal access token*, limited to the configured repos | repository permissions **Contents** (read and write: create a branch), **Pull requests** (read and write: read, send a review, merge), **Checks** (read and write: read check status, re-request a check), **Issues** (read and write: comments), **Metadata** (read, automatic) |
| Jira | *API token* of the Atlassian account | the account needs "Browse projects", "Add comments" and "Edit issues / Transition issues" in the projects; sign-in with the email from the configuration |

### OAuth (alternative to tokens)

Instead of a token you sign in in the browser. Tokens and client secret live only in the backend's `.env`, never in
browser, repo, database or log. In the assistant this is folded under "Erweitert: Mit OAuth statt Token". If both
exist, OAuth wins over the token.

1. *Register the apps.* GitHub: a **GitHub App** (callback `http://127.0.0.1:8787/api/oauth/github/callback`, "Expire
   user authorization tokens" on, repository permissions as in the table above, install the app on the repos).
   Atlassian: an **OAuth 2.0 (3LO) app** (callback `http://localhost:8787/api/oauth/atlassian/callback`, Jira scopes
   `read:jira-work`, `write:jira-work`, `read:jira-user`). Atlassian apps are *resource-level*: access applies only
   to the site you choose when you consent.
2. *Client ID into `config.toml`* (not secret; with `[atlassian]`, `[jira]` needs neither `site` nor `email`):

   ```toml
   [github]
   repos = ["acme/app"]             # the sync still needs the repos

   [github_oauth]
   client_id = "Iv23…"

   [atlassian]
   client_id = "7dNimh…"
   # site = "acme"                  # only needed when you grant several sites (name, address or id)
   ```

3. *Client secret into `.env`* (standard input, like the tokens):

   ```bash
   echo "$GITHUB_CLIENT_SECRET"    | cargo run -p cockpit-server -- token set github-oauth
   echo "$ATLASSIAN_CLIENT_SECRET" | cargo run -p cockpit-server -- token set atlassian-oauth
   ```

4. *Sign in.* Under "Einrichten", the Jira and GitHub steps show "Mit Atlassian anmelden" / "Mit GitHub anmelden".
   The button opens `GET /api/oauth/{atlassian|github}/start` and redirects to the service (with a one-time `state`
   valid for 10 minutes, and PKCE). After consent the service calls `GET /api/oauth/{service}/callback`; the backend
   checks `state`, exchanges the `code` for tokens and redirects to `/` with a notice of success or the error. The
   callback is a browser navigation: loopback and host checks still apply, a JSON content type is not required.
   - *Atlassian:* the backend then queries `accessible-resources`. Exactly one site is chosen automatically; with
     several, `site` must be set in the configuration (the error message lists the options). Jira then runs through
     `https://api.atlassian.com/ex/jira/<cloudId>`; the chosen site (id and name, not secret) is stored in the database.
   - *GitHub:* the user token expires (GitHub App); the backend refreshes it before expiry with the refresh token.
     Atlassian rotates the refresh token on every refresh; the backend stores that too.
5. *Disconnect.* "Trennen" in the app or `POST /api/oauth/{service}/revoke` deletes the tokens in `.env`. For GitHub
   the backend also revokes the token at the service (`DELETE /applications/{client_id}/token`). Atlassian offers no
   call for that; revoke access at id.atlassian.com/manage-profile/apps (the app points there). The tokens are always
   deleted, even if the service does not answer. The client secret stays.
6. *When it stops working.* An expired or revoked access shows "not signed in" (`Problem::Unauthorized`) at the source.
   If the service rejects the refresh, the backend deletes the tokens: the service counts as disconnected and you sign
   in again. Network errors or service outages keep the tokens.

`GET /api/health` reports under `sign_in.github` and `sign_in.jira` `connected`, `kind` (`oauth`, `token` or `none`),
`oauth_possible` (app set up and secret present) and, for Atlassian, the `site`.

**Not verified against the real services.** Token exchange, refresh, revocation and site choice are only tested
against recorded or mocked responses (`cargo test`). Open until the first real run: whether Atlassian accepts
`code_verifier` for a web app with a client secret (otherwise it is dropped there), whether the GitHub App accepts
PKCE, and GitHub's installation redirect (`installation_id`, `setup_action`): without a `state` from our start the
backend rejects it; the app is already installed, sign-in always begins in Cockpit.

### The setup assistant in detail

The page "Einrichten" (`#/setup`) is a guided assistant in five steps with a progress indicator (Container, Jira,
GitHub, Modell, Fertig). It appears by itself on first start when nothing is set up (once per browser; the browser
remembers only this hint) and is always reachable via "Einrichten" in the sidebar. Every step can be skipped; the
summary says in plain language what works (check mark), what is open and what to verify, with "Ansehen" leading to
the right step. The texts assume no prior knowledge.

- *Container.* Detects Docker or Podman; if neither exists, the step explains what a container is and points to
  Rancher Desktop (engine **"dockerd (moby)"**) or Podman. "Erneut prüfen" asks again.
- *Jira.* API token in three steps, connection test and project choice (above). The Atlassian OAuth app is folded
  away under "Erweitert", with a walkthrough of the developer console.
- *GitHub.* Instructions for a **GitHub App** with a checklist of permissions (Contents, Pull requests, Issues,
  Checks read & write; Commit statuses and Metadata read-only), callback URL to copy, client ID and secret, a hint
  to "Install App" and the repos, then "Mit GitHub anmelden".
- *Modell.* Provider, endpoint address, model, key and connection test.

**How inputs arrive.** Non-secret values (client IDs, Jira site, email, projects, repos, endpoint address and model)
go to `PUT /api/config`. The server changes `config.toml` **atomically** (neighbour file, then rename; existing
comments, order and other values stay, new files are readable only by you) and validates the result **as at start**
(`Config::parse`): if invalid, the file stays as it was and the answer names the reason in plain language. Inputs are
normalised (`acme` → `https://acme.atlassian.net`, GitHub URLs → `owner/name`, projects uppercased, `localhost:4000` →
`http://localhost:4000`). Everything takes effect **without restart** (sources, OAuth apps and the agent endpoint are
swapped in the running server); only the sync interval and the port wait for a restart. If the client ID of an app
changes, the answer advises signing in again.

**How keys arrive.** Client secrets, tokens and the endpoint key go into password fields (`autocomplete="off"`) and
are sent once to `PUT /api/secrets/{name}` over the loopback connection; the server stores them in `.env`. See
[docs/security.md](docs/security.md) for the guarantees.

**Model provider and endpoint.** `[agent] provider` picks the tool in the container: `claude-agent-sdk` runs;
`claude-code` and `codex` are known values that report "not implemented yet" in plain text (the assistant shows them
as "bald"). `[agent.endpoint]` says where model requests go: with `url` to an Anthropic-compatible endpoint
(LiteLLM, another proxy; in the container `ANTHROPIC_BASE_URL` + `ANTHROPIC_AUTH_TOKEN`), without `url` directly to
Anthropic (in the container `ANTHROPIC_API_KEY`); `model` is the name as the endpoint offers it. "Modelle laden" calls
`GET /api/agent/models`; the backend then queries `GET <url>/v1/models` (proxy: `Authorization: Bearer`; direct:
`https://api.anthropic.com/v1/models` with `x-api-key` and `anthropic-version`) without following redirects, with a
timeout and a size limit, and returns only the IDs. "Verbindung testen" calls `GET /api/agent/check` and reports
*reachable*, *key accepted* and *model present*, with a hint on errors. Without a key it does not call the endpoint.
`/api/health` reports under `endpoint` provider, address, model and whether a key exists (never the key).

**Container runtime.** The backend chooses: `COCKPIT_CONTAINER`, else `[agent] container`, else it looks for
`podman`, then `docker` in the `PATH` and checks with `<runtime> info` whether the engine runs (a process call, never
the socket; the result is cached for 10 seconds). `/api/health` reports under `container` `runtime`, `engine_running`,
`source` (`env`, `config`, `detected`, `none`) and a `hint`.

## Architecture

```
Browser (apps/cockpit, WASM)            Backend (apps/server)
┌──────────────────────────┐   HTTP    ┌────────────────────────────────┐
│ Pages → store.dispatch   │ ────────▶ │ POST /api/actions              │──▶ GitHub, Jira
│ Workspace (signal)       │ ◀──────── │ GET  /api/workspace (snapshot) │──▶ Agent (container)
│                          │ ◀── SSE ─ │ GET  /api/events (changes)     │◀── polling (webhooks: open)
└──────────────────────────┘           │ SQLite · keys in .env          │
                                        └────────────────────────────────┘
          both use  crates/cockpit-core  (model, rules, Action)
```

```
.
├── Cargo.toml                  # workspace: shared versions and members
├── apps/                       # everything you start
│   ├── cockpit/                # the main application: UI (WASM), own index.html and Trunk.toml
│   │   └── src/                # main.rs (start, routing, agent tick), route.rs, store.rs, pages/
│   ├── server/                 # backend: axum, SQLite, key storage (binary cockpit-server)
│   ├── agent/                  # container image and runner of the agent (Node/TypeScript)
│   └── playground/             # the playground: shows every building block on its own (own Trunk.toml)
├── crates/                     # libraries
│   ├── cockpit-core/           # domain logic of the cockpit: no UI, no network, with tests
│   ├── integrations/           # GitHub and Jira clients (read, write, sync), native only
│   └── ui/                     # component library (crate `feature-ui`) plus tokens.css, components.css, extras.css
├── scripts/                    # start.sh (build app, start server), launchd template
├── docs/                       # architecture.md, security.md
└── start-cockpit.command       # double-click start for macOS
```

Principles (details in [docs/architecture.md](docs/architecture.md)):

1. **`cockpit-core` is the single source of truth.** Model and rules live there, without UI and network, with
   tests. Browser and backend use the same crate.
2. **Every change is an `Action`.** The UI calls `store.dispatch(action)`; the backend applies it through
   `Workspace::apply`, additionally triggers the effect at GitHub or Jira and writes it to the log. The browser holds
   nothing of it.
3. **Two rules are non-negotiable:** comments and reviews of the agent leave only after your approval. The agent
   never merges and never writes to the main branch.
4. **Every step stays runnable.** After every change the Playground and the Cockpit build, and the Cockpit runs with
   the server, with an empty start and with `COCKPIT_SEED=demo`.

The server crates (`apps/server`, `crates/integrations`) compile natively only; CI excludes them from the Wasm
build.

## Commands and checks

Run before every PR (CI runs the same):

```bash
cargo fmt --all --check
cargo clippy --workspace --exclude cockpit-server --exclude cockpit-integrations \
  --all-targets --target wasm32-unknown-unknown -- -D warnings
cargo clippy -p cockpit-server -p cockpit-integrations --all-targets -- -D warnings
cargo test --workspace
(cd apps/playground && trunk build) && (cd apps/cockpit && trunk build)
(cd apps/agent/runner && node --disable-warning=ExperimentalWarning --test test/*.test.ts)   # Node >= 22.18
```

Also click through the feature in the browser against the server, once with an empty start
(`COCKPIT_CONFIG=<empty config.toml> COCKPIT_DB=<new file> cargo run -p cockpit-server`: all pages show empty
states), once with `COCKPIT_SEED=demo` (fresh `COCKPIT_DB`), and with the server stopped (the notice "nicht erreichbar"
with "Erneut versuchen"). When trying things out always use your own database and an empty `config.toml`, **never
against real Jira or GitHub boards**.

Tests that start real containers are marked `#[ignore]` and run with
`COCKPIT_CONTAINER=docker cargo test -- --ignored` or `=podman`; a PR names the runtime it was checked with.

Rules for every PR: one feature, one branch, one PR against `main`; keep it small and do not touch unrelated areas.
Logic goes into `cockpit-core` with unit tests; the app only wires it up. CSS uses tokens only;
`crates/ui/tokens.css` and `crates/ui/components.css` come 1:1 from the design and stay unchanged (your own
additions go into `extras.css`). New fields in persisted types need `#[serde(default)]` so older states stay
readable. What can only be verified with real credentials is tested against recorded responses and named in the PR as
"not verified against the real service".

## Security model

Full detail: [docs/security.md](docs/security.md). In short:

- **Credentials** live only in the backend, in the `.env` file in the project folder (never committed, mode `600`,
  CI fails on a tracked `.env`), never in the repo, database, logs or browser storage. The setup assistant sends a
  value once over loopback to `PUT /api/secrets/{name}` (fixed names only, the answer is only "set", the field is
  cleared at once).
- **Loopback only.** The server listens on `127.0.0.1`. Every route under `/api` checks the peer (loopback) and the
  `Host` header (`127.0.0.1:<port>` or `localhost:<port>`, against DNS rebinding); writing requests also need
  `Content-Type: application/json` and a missing or own `Origin`.
- **The agent has no GitHub token** and cannot push. It commits to a branch `claude/<ticket>-…`; the backend
  verifies it in a fresh bare repo against the repo's path rules and pushes it. **Never** the main branch, **never** a
  force-push, **never** a merge. You create the pull request yourself.
- Comments and reviews of the agent go out only after your approval; only you may merge.
- The agent writes only to `/work`; the runner's `canUseTool` blocks `.git`, forbidden paths, `git push` and network
  commands. Limits (budget per run and month, time limit, parallel runs) stop the container (`rm -f`).

## Container rules

The agent container must run with **Docker** (for example from Rancher Desktop) **and** with **Podman**. Rules for
anything that builds or starts containers (details in [docs/architecture.md](docs/architecture.md#container-rules)):

1. **Command line only.** The backend calls `docker` or `podman` as a process, never the Docker socket or a library.
   Only commands and switches both understand the same way (`build -f`, `run --rm`, `-e`, `-v`, `--network`,
   `--add-host`, `--user`, `--label`, `ps`, `rm -f`, `image inspect`).
2. **Runtime selectable:** `[agent] container` or `COCKPIT_CONTAINER`; without either, detection tries `podman`
   first, then `docker` (checked with `<cli> info`).
3. **Fully qualified images** (`docker.io/library/node:…`) and a `Containerfile` without BuildKit features
   (no `RUN --mount`, no `# syntax=`).
4. **Reach the host** with `--add-host cockpit-host:host-gateway` and `http://cockpit-host:<port>`, not
   `host.docker.internal` or `host.containers.internal`.
5. **Secrets only as `-e NAME`**, with the value in the child process environment, never on the command line.
6. **Label per run** (`cockpit.run=<key>`) so orphaned containers are removed at server start.
7. **Rancher Desktop** must use the engine "dockerd (moby)"; "containerd" mode (only `nerdctl`) is not supported.

## The Cockpit app

Pages (hash routes):

| Page | Address | What it does |
|------|---------|--------------|
| Einrichten | `#/setup` | state and next step per service: sign in to / disconnect Jira and GitHub, check the agent endpoint, detect the container runtime |
| Aufmerksamkeit | `#/`, `#/mine`, `#/team`, `#/snoozed` | inbox of tickets and pull requests, filtered by switchable rules (mention, review request, blocked PR, overdue ticket, agent done, assigned); snooze; change status, create a branch, reply, assign the agent; edit a ticket; link ticket and PR; search, source filter and project/repo/person/status/label filters; keys `↑↓`/`j k`, `e`, `r`, `s`, `u`, `/` |
| Review | `#/review/87` | read the diff; select lines and write your own comments; approve, reject or edit comments of the agent; send the review; checks with log; merge (only you) |
| Übersicht | `#/overview` | open entries and load per person; "Hängt" shows failed checks, waiting reviews, tickets without a branch and agents waiting for you |
| Benachrichtigungen | `#/notifications` | per occasion choose browser, email and Slack; quiet hours; permission request by button; test notification |
| Agent | `#/agent/BAU-142` | assignment with options; plan for approval; five-step flow to the pull request; answer questions, send hints, pause; history as a conversation |
| Grenzen & Regeln | `#/limits` | prices per million tokens, budget per run and month, time limit, parallel runs; per repo allowed/forbidden paths, required tests, instructions and plan requirement; usage today and this month |

Inbox filters: besides the Jira project the header filters by **repo**, **person**, **status** and **label**; each
selection defaults to "Alle", filters combine with AND, "Filter zurücksetzen" resets them. The choice belongs to the
page only and is not persisted. The logic is `cockpit_core::InboxFilter` (`crates/cockpit-core/src/search.rs`).

Notifications are derived purely from comparing the workspace before and after an action
(`crates/cockpit-core/src/notify.rs`). In the **browser** Cockpit never asks for permission by itself, only via the
button. Email and Slack are only prepared: the choice is stored, sending will go through the backend later.

More about agent behaviour (questions, steering, follow-up jobs, costs, limits, rules per repo) is in
[docs/architecture.md](docs/architecture.md#agent-behaviour).

## The component library (`feature-ui`)

The crate `crates/ui` mirrors the SwiftUI names of the design:

```rust
view! {
    <Button variant=ButtonStyle::GlassProminent size=ControlSize::Large>"Weiter"</Button>
    <Toggle checked=wifi />
    <TextField label="E-Mail" value=mail input_type="email" error=invalid />
}
```

| Group | Components |
|-------|------------|
| Basics | `Root`, `Text`, `Icon`, `Glass` |
| Layout | `VStack`, `HStack`, `ZStack`, `Spacer`, `Grid`, `Split`, `Container`, `PageSection` |
| Controls | `Button`, `Link`, `Badge`, `Toggle`, `Segmented`, `Slider`, `Stepper` |
| Input | `TextField`, `SearchField` |
| Content | `List`, `ListSection`, `ListRow`, `ListButtonRow`, `Card`, `CardBody`, `Media` |
| Navigation | `Navbar`, `IosNav`, `TabBar`, `Pager`, `Footer` |
| Overlay | `Sheet` |
| App | `SplitView`, `Sidebar`, `Pane`, `Status`, `Avatar`, `Chip`, `InboxRow`, `CommentBubble`, `Textarea`, `Diff`, `SuggestionCard`, `Timeline`, `Progress`, `Log`, `Steps` |

**Building block and story.** A building block lives in `crates/<name>/` (a crate that depends only on `leptos`, hence
reusable); a story lives in `apps/playground/src/stories/<name>.rs` and shows it with sample values. In another
project include it by path:

```toml
[dependencies]
feature-ui = { path = "../rust-playground/crates/ui" }
```

and include `crates/ui/tokens.css`, `components.css` and `extras.css`, in this order.

**Design tokens.** `crates/ui/tokens.css` has three layers:

| Layer | Prefix | Content | Example |
|-------|--------|---------|---------|
| Primitive | `--ref-*` | raw values | `--ref-space-16` |
| Semantic | `--sys-*` | roles, theme dependent | `--sys-color-label` |
| Component | `--comp-*` | per building block | `--comp-button-height-large` |

Your own CSS uses only `--sys-*` (if needed `--ref-*`); then light/dark, accent colour and "increase contrast" work
automatically. All values are visible in the story **Foundations**; theme, accent and contrast are switched at the
bottom left of the Playground. *Updating the design:* `tokens.css` and `components.css` are taken unchanged from the
design; replace the files on changes. Own additions go into `extras.css`.

**Add your own story.**

1. Create `apps/playground/src/stories/my_block.rs` (template: `link_badge.rs`):
   ```rust
   use feature_ui::*;
   use leptos::prelude::*;
   use super::{Story, common::Section};

   pub const STORY: Story = Story {
       id: "my-block",            // appears in the URL: #my-block
       title: "My block",
       group: "Controls",         // heading in the navigation
       description: "What does the story show?",
       api: ".ui-… · data-…",     // short reference
       render,
   };

   fn render() -> AnyView {
       view! { <Section title="Example">{/* … */}</Section> }.into_any()
   }
   ```
2. In `apps/playground/src/stories/mod.rs` add `mod my_block;` and register `my_block::STORY` in `STORIES`.

**Add a building block as a new crate.**

1. `cargo new --lib crates/my-block --name feature-my-block`
2. In its `Cargo.toml`: `leptos = { workspace = true }`. Through `members = [..., "crates/*"]` it is part of the
   workspace automatically.
3. Register it in the root `Cargo.toml` under `[workspace.dependencies]`:
   `feature-my-block = { path = "crates/my-block" }`, then use it in `apps/playground/Cargo.toml`.
4. Include your own CSS in `apps/playground/index.html` with `<link data-trunk rel="css" …>`, after the tokens.

**Font.** The design uses SF Pro (present on Apple devices) and falls back to **Inter**. `apps/playground/index.html`
loads Inter from Google Fonts. Without an external connection remove the three `<link>` lines and the system font
applies.

### Learning path

For learning Rust through this code base: each concept names where you see it and the Playground story that shows it.

| # | Concept | In the code | In the Playground |
|---|---------|-------------|-------------------|
| 1 | **Components and props**: a function with `#[component]`, parameters are props | `crates/ui/src/text.rs` (`Text`) | Foundations |
| 2 | **Enums instead of strings**: `enum`, `impl`, `#[derive(Default)]`, `#[default]`, `const ALL` | `crates/ui/src/button.rs` (`ButtonStyle`) | Button |
| 3 | **`Option` and `then_some`**: set an attribute only sometimes | `button.rs`, function `attr()` and `data-role` | Button |
| 4 | **Children**: pass content to a component | `crates/ui/src/layout.rs` (`VStack`) | Stacks & Grids |
| 5 | **Signals**: reactive state, `RwSignal`, `.get()` / `.set()` | `crates/ui/src/controls.rs` (`Toggle`) | Toggle · Picker · … |
| 6 | **`move` closures**: why signals sit in `move \|\| …` | everywhere, e.g. `data-role=role` in `button.rs` | Button |
| 7 | **Derived signals**: `Signal::derive` | `apps/playground/src/stories/button.rs` | Button (live controls) |
| 8 | **Callbacks**: report events upwards | `button.rs` (`on_click`) | Button |
| 9 | **Rendering lists**: iterators and `collect_view()` | `controls.rs` (`Segmented`) | Toggle · Picker · … |
| 10 | **Conditional rendering**: `<Show>` and `match` with `.into_any()` | `sheet.rs`, `list.rs` (`ListRow`) | iPhone example |
| 11 | **`'static` and constants**: `&'static [(&str, &str)]`, `const`, `static` | `controls.rs`, `apps/playground/src/stories/mod.rs` | all |
| 12 | **Function pointers**: `render: fn() -> AnyView` in a registry | `apps/playground/src/stories/mod.rs` (`Story`) | the navigation itself |
| 13 | **Chaining `Option` and `Result`**: `.ok().flatten().is_some_and(…)` | `apps/playground/src/main.rs` (`system_theme`) | light/dark switch |
| 14 | **Browser events**: `window_event_listener`, cleanup with `on_cleanup` | `apps/playground/src/main.rs` | navigation by URL (`#button`) |
| 15 | **Global counter**: `AtomicU32` for unique IDs | `crates/ui/src/field.rs` | TextField & Search |
| 16 | **Tests**: `#[cfg(test)]`, `include_str!` reads the CSS at compile time | `crates/ui/src/lib.rs` | `cargo test` |
| 17 | **Logic without UI**: `struct`, `impl`, `Result`, state machine, many tests | `crates/cockpit-core/src/agent.rs`, `workspace.rs` | Cockpit |
| 18 | **Shared state**: a `Copy` store in context, `Memo`, `Effect` | `apps/cockpit/src/store.rs`, `apps/cockpit/src/pages/inbox.rs` | Cockpit |
| 19 | **Workspace and crates**: shared versions, one crate per building block | `Cargo.toml`, `crates/ui/Cargo.toml` | – |

A good exercise for every step: break something on purpose (for example `ButtonStyle::Glas` instead of `Glass`) and
read the compiler message. Rust usually explains errors very precisely.

## Roadmap / Open work

Not built yet (everything below is open; nothing here has been verified against the real services either):

- **Follow-up jobs in real agent mode:** implementing review comments and fixing red checks currently run only in
  the simulation (the real-agent mode answers with a plain-language notice).
- **Pull request created by the agent automatically:** today the agent pushes a `claude/…` branch and you open the
  pull request yourself.
- **Adapters for Claude Code and Codex** in the runner (`apps/agent/runner/src/adapter/`, see
  [apps/agent/README.md](apps/agent/README.md)); then pass `COCKPIT_PROVIDER` on.
- **Webhooks / live update:** open decision between a tunnel (webhooks) and polling every 2 minutes; the default
  stays polling.
- **Error and empty states** (expired token, rate limit, offline, backend unreachable) across all pages, and a
  **mobile view** (navigation at the bottom, list and detail as separate levels).
- Verification with a **real container and a real model** (`#[ignore]` tests with `COCKPIT_CONTAINER=docker|podman`),
  and a first run of the **release workflow**.
- Smaller known gaps: restarting a check through the API may need a GitHub App instead of a token; Jira status
  changes depend on the project's workflow (only tested against recorded responses); editing tickets, follow-up
  jobs and limits are not sent outward yet, and the sync overwrites title and description with the service's state
  (priority and PR link stay local); browser notifications are only tested with a replaced notification API.
