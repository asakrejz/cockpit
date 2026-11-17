# Cockpit

Cockpit is a local dashboard that brings **Jira, GitHub and coding agents** together in one place: an inbox of
tickets and pull requests that need your attention, a review page with a diff, and an agent that implements a ticket
in a container and hands you a branch to review. It runs on your own computer, for one person. Credentials never
leave your machine.

The repository also contains the **Playground**, a Storybook-like page that shows every UI building block on its own.

- UI: [Leptos](https://leptos.dev) (Rust compiled to WebAssembly), built with [Trunk](https://trunkrs.dev)
- Backend: axum + SQLite (`apps/server`, binary `cockpit-server`)
- Visible UI texts are German; code, naming and documentation are English.

## Prerequisites

| What | What for | Where |
|------|----------|-------|
| **Rust** (via rustup) | compiles Cockpit | [rustup.rs](https://rustup.rs) |
| **Trunk** | builds the UI | `cargo install trunk --locked` or `brew install trunk` |
| **Git** | fetches the code | `xcode-select --install` on macOS |
| **Rancher Desktop** (engine "dockerd (moby)") *or* **Podman** | container runtime, only for the agent | [rancherdesktop.io](https://rancherdesktop.io), [podman.io](https://podman.io/docs/installation) |
| **Node ≥ 22.18** | only to run the agent runner tests | [nodejs.org](https://nodejs.org) |

## Quick start

```bash
git clone https://github.com/asakrejz/rust-playground.git
cd rust-playground
scripts/start.sh --open
```

`scripts/start.sh` (bash, macOS) checks that `cargo` and `trunk` exist, builds `apps/cockpit` into `apps/cockpit/dist`
when needed and starts `cockpit-server` on `http://127.0.0.1:8787`. It installs nothing. `--release` builds the
server as a release build. Without the script:

```bash
(cd apps/cockpit && trunk build) && cargo run -p cockpit-server
```

The server begins with an empty workspace. On first start the page **"Einrichten"** (setup assistant) opens by itself
and guides you through container, Jira, GitHub, model access and live updates. Every step can be skipped.

### Quick start for beginners

No terminal experience needed; allow about one hour, mostly waiting for downloads. Install the programs from
[Prerequisites](#prerequisites), clone the repository as above (it is private: you need access and sign in to GitHub),
then double-click **`start-cockpit.command`** in Finder (first time: right-click › "Open" › "Open"). It builds the app
(5–15 minutes the first time), opens the browser and shows errors in plain language. Close the window to quit Cockpit.
Then follow the assistant. Every step, including what only you can do (Atlassian token, GitHub App, model access),
is in the [setup guide](docs/setup-guide.md).

For each version tag, `.github/workflows/release.yml` builds a macOS (Apple Silicon) archive with everything
pre-built; unzip and right-click `start-cockpit.command` › "Open". The programs are not signed, hence the macOS
prompt. Documentation links in this README that point into `docs/` are only valid in the repository.

## Configuration in brief

Everything is optional. `config.toml` and the database `cockpit.db` live in the folder `config/` of the project (the
server creates it at the first start; Git ignores it, only `config/config.example.toml` is committed). Nothing is
written outside the project. Environment variables such as `COCKPIT_PORT`, `COCKPIT_DB`, `COCKPIT_CONFIG` and
`COCKPIT_CONTAINER` override it. **Keys live only in the `.env` file in the project folder**, never in `config.toml`,
the database or the browser:

```bash
echo "$TOKEN" | cargo run -p cockpit-server -- token set jira   # also: github, agent-key, github-oauth, atlassian-oauth
```

**Never commit `.env`.** `.gitignore` excludes it and CI fails when any `.env*` file is tracked. The server sets mode `600` and warns about loose permissions. If a real `.env` was ever
committed, rotate the keys immediately.

The full reference (`config.toml`, all variables, several repos, model provider and endpoint, container runtime,
`trunk serve`, start at login) is in [docs/configuration.md](docs/configuration.md). Connecting Jira, GitHub, OAuth,
the agent image and webhooks: [docs/setup-guide.md](docs/setup-guide.md).

Check the server with `curl localhost:8787/api/health`.

## Security in brief

- The server listens on `127.0.0.1` only and checks peer and `Host` on every `/api` route (DNS rebinding); writing
  requests also need `Content-Type: application/json` and a missing or own `Origin`.
- Credentials stay in the backend (`.env`, mode `600`). Give tokens only the rights listed in the
  [setup guide](docs/setup-guide.md#github); while Cockpit should only read, use read-only rights.
- Webhooks are optional and off by default. Through a tunnel only `/api/webhooks/*` may be reachable; they only
  trigger a sync and accept a correct HMAC signature only.
- The agent has no GitHub token and cannot push. It commits to a branch `claude/<ticket>-…`; the backend verifies it
  against the path rules and pushes it. Never the main branch, never a force-push, never a merge. Comments and
  reviews of the agent go out only after your approval; only you merge.
- The agent writes only to `/work`; limits (budget, time, parallel runs) stop the container.

Details and threat model: [docs/security.md](docs/security.md).

## Development

```
apps/
  cockpit/      main application: UI (WASM), own index.html and Trunk.toml
  server/       backend: axum, SQLite, key storage (binary cockpit-server)
  agent/        container image and runner of the agent (Node/TypeScript)
  playground/   shows every UI building block on its own
crates/
  cockpit-core/ domain logic: model, rules, Action; no UI, no network, with tests
  integrations/ GitHub and Jira clients, native only
  ui/           component library (crate `feature-ui`), tokens.css, components.css, extras.css
scripts/        start.sh, launchd template
docs/           architecture, security, configuration, setup guide, components
```

`cockpit-core` is the single source of truth; every change is an `Action` applied by the backend. The server crates
compile natively only. Details: [docs/architecture.md](docs/architecture.md).

Run before every PR (CI runs the same):

```bash
cargo fmt --all --check
cargo clippy --workspace --exclude cockpit-server --exclude cockpit-integrations \
  --all-targets --target wasm32-unknown-unknown -- -D warnings
cargo clippy -p cockpit-server -p cockpit-integrations --all-targets -- -D warnings
cargo test --workspace
(cd apps/playground && trunk build) && (cd apps/cockpit && trunk build)
(cd apps/agent/runner && node --disable-warning=ExperimentalWarning --test test/*.test.ts)
```

Work on the UI with `(cd apps/playground && trunk serve --open)` (port 8080); for the app against a server see
[docs/configuration.md](docs/configuration.md#developing-with-trunk-serve). Tests that start real containers are
`#[ignore]`d: `COCKPIT_CONTAINER=docker cargo test -- --ignored` (or `podman`).

When trying things out, use your own database and an empty `config.toml` (`COCKPIT_CONFIG`, `COCKPIT_DB`), once with
an empty start and once with `COCKPIT_SEED=demo`; **never against real Jira or GitHub boards**.

Rules for every PR: one feature, one branch, one PR against `main`; logic goes into `cockpit-core` with unit tests;
CSS uses tokens only, `tokens.css` and `components.css` stay unchanged (own additions go into `extras.css`); new fields
in persisted types need `#[serde(default)]`; what only real credentials can verify is tested against recorded responses
and named in the PR as "not verified against the real service".

## Known limitations

Everything that needs real services has only been tested against recorded responses and local stand-ins: sync and writes
at GitHub and Jira, OAuth exchange and refresh, webhooks through a real tunnel, the Claude Code and Codex adapters with
the real CLIs, a real container with a real model, and the release workflow. Restarting a check may need a GitHub App
instead of a token; ticket and limit edits are not sent outward yet; email and Slack notifications are only prepared.

## Documentation

| Document | Contents |
|----------|----------|
| [docs/setup-guide.md](docs/setup-guide.md) | Jira, GitHub, OAuth, agent image, model endpoint, webhooks, setup assistant |
| [docs/configuration.md](docs/configuration.md) | `config.toml`, environment variables, `.env`, start at login |
| [docs/using-cockpit.md](docs/using-cockpit.md) | pages of the app, inbox filters, notifications |
| [docs/architecture.md](docs/architecture.md) | decisions, HTTP API, sync, container rules, agent behaviour |
| [docs/security.md](docs/security.md) | credentials, request checks, webhooks, agent sandbox |
| [docs/components.md](docs/components.md) | component library `feature-ui`, design tokens, learning path |
| [apps/agent/README.md](apps/agent/README.md) | agent image, adapters, event protocol |

## License

MIT (see `license` in `Cargo.toml`).
