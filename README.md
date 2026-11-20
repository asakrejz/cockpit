# Cockpit

Cockpit is a local dashboard that brings Jira (or the Projects tab of GitHub), GitHub and a coding agent together in one
place: an inbox of tickets and pull requests, a review page with a diff, and an agent that implements a ticket in a container. It runs on your own
computer, for one person.

## Prerequisites

| What | What for | Where |
|------|----------|-------|
| Rust (via rustup) | compiles Cockpit | [rustup.rs](https://rustup.rs) |
| Trunk | builds the UI | `cargo install trunk --locked` or `brew install trunk` |
| Git | fetches the code | `xcode-select --install` on macOS |
| Rancher Desktop (engine "dockerd (moby)") or Podman | runs the agent; optional | [rancherdesktop.io](https://rancherdesktop.io), [podman.io](https://podman.io/docs/installation) |

## Setup

```bash
git clone https://github.com/asakrejz/cockpit.git
cd cockpit
scripts/start.sh --open
```

Or double-click `start-cockpit.command` in Finder (first time: right-click › "Open"). The first build takes 5–15
minutes. Then the browser opens Cockpit at `http://127.0.0.1:8787`, and the setup assistant ("Einrichten") guides you
through Jira, GitHub and the model. Every step can be skipped. What only you can do (creating tokens and apps) is in the
[setup guide](docs/setup-guide.md).

## Run with Docker

Instead of building, you can run the ready-made image (log in to `ghcr.io` first, see the guide). One command starts
Cockpit, and the same command updates it (it pulls a newer image and replaces the container only if there is one):

```bash
docker compose up -d
```

Without compose: `scripts/cockpit-container.sh` does the same with `docker` or `podman`. Your configuration and keys
live in the `cockpit-config` volume; `docker compose down` keeps it (never add `-v`). Details:
[docs/container.md](docs/container.md).

## Keys

Keys live only in `config/.env` (mode `600`, created on first use). Set them in the assistant, or in a terminal; the
value comes from standard input:

```bash
echo "$TOKEN" | cargo run -p cockpit-server -- token set <name>
```

| Name | Key |
|------|-----|
| `jira` | Jira API token |
| `github` | GitHub token |
| `agent-key` | key of the model endpoint |
| `github-oauth` | client secret of your GitHub App |
| `atlassian-oauth` | client secret of your Atlassian OAuth app |

Never commit `config/.env`: Git ignores it, and CI fails when any `.env*` file is tracked. If a real key was ever
committed, rotate it. Everything else (`config/config.toml`, ports, the database) is in
[configuration](docs/configuration.md).

## Documentation

| Document | Contents |
|----------|----------|
| [docs/setup-guide.md](docs/setup-guide.md) | Jira, GitHub, OAuth, agent image, model, webhooks, step by step |
| [docs/container.md](docs/container.md) | run Cockpit as a container image |
| [docs/configuration.md](docs/configuration.md) | `config.toml`, environment variables, `config/.env` |
| [docs/using-cockpit.md](docs/using-cockpit.md) | pages of the app, filters, teams, agent runs |
| [docs/architecture.md](docs/architecture.md) | decisions, HTTP API, sync, agent behaviour, development |
| [docs/security.md](docs/security.md) | credentials, request checks, webhooks, agent sandbox |
| [docs/components.md](docs/components.md) | component library and design tokens |
| [docs/roadmap.md](docs/roadmap.md) | planned features |
| [apps/agent/README.md](apps/agent/README.md) | agent image, adapters, event protocol |

MIT license (see `Cargo.toml`).
