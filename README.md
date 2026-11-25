# Cockpit

Cockpit is a local dashboard that brings Jira (or the Projects tab of GitHub), GitHub and a coding agent together in one
place: an inbox of tickets and pull requests, a review page with a diff, and an agent that implements a ticket in a
container. It runs on your own computer, for one person.

## Prerequisites

| What | What for | Where |
|------|----------|-------|
| Rust (via rustup) | compiles Cockpit (native start only) | [rustup.rs](https://rustup.rs) |
| Trunk | builds the UI (native start only) | `cargo install trunk --locked` or `brew install trunk` |
| Git | fetches the code | `xcode-select --install` on macOS |
| Rancher Desktop (engine "dockerd (moby)") or Podman | runs the agent and the container start; optional for the native start | [rancherdesktop.io](https://rancherdesktop.io), [podman.io](https://podman.io/docs/installation) |

## Start natively

```bash
git clone https://github.com/asakrejz/cockpit.git
cd cockpit
scripts/start.sh --open
```

Or double-click `start-cockpit.command` in Finder (first time: right-click › "Open"). The first build takes 5–15 minutes.
Then the browser opens Cockpit at `http://127.0.0.1:8787`, and the setup assistant (the page "Setup", German "Einrichten")
guides you through Jira, GitHub and the model. Every step can be skipped. What only you can do (creating tokens and apps)
is in the [setup guide](docs/setup-guide.md).

## Start as a container

Instead of building, you can run the ready-made image (pulling it needs no sign-in). In the cloned folder:

```bash
docker compose up -d
```

The same command updates Cockpit. Without compose, `scripts/cockpit-container.sh` does the same with `docker` or
`podman`. Your configuration and keys live in the `cockpit-config` volume. Update, only the image without a clone, the
agent socket and the security notes: [docs/container.md](docs/container.md).

## Keys

Keys (Jira token, GitHub token, key of the model endpoint, client secrets) live only in `config/.env` (mode `600`, created
on first use, never committed: Git ignores it and CI fails when any `.env*` file is tracked). Set them in the setup
assistant, or in a terminal with `cockpit-server token set <name>`; the names and the commands are in
[docs/configuration.md](docs/configuration.md#credentials-the-env-file).

## Documentation

| Document | Contents |
|----------|----------|
| [docs/setup-guide.md](docs/setup-guide.md) | Jira, GitHub, OAuth, agent image, model, webhooks, step by step |
| [docs/configuration.md](docs/configuration.md) | the reference: `config.toml`, environment variables, `config/.env` |
| [docs/container.md](docs/container.md) | run Cockpit as a container image |
| [docs/using-cockpit.md](docs/using-cockpit.md) | the pages of the app, filters, teams, agent runs |
| [docs/architecture.md](docs/architecture.md) | crates, HTTP API, sync, agent behaviour, development and CI |
| [docs/security.md](docs/security.md) | credentials, request checks, webhooks, agent sandbox |
| [docs/components.md](docs/components.md) | component library and design tokens |
| [apps/agent/README.md](apps/agent/README.md) | agent image, adapters, job and event protocol |

MIT license (see `Cargo.toml`).
