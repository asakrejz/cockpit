# Setup guide

Step-by-step instructions for connecting Cockpit to the real services. The easy way is the setup assistant: the
"Einrichten" page walks through container, Jira, GitHub, model access and live updates and writes client IDs, Jira
site, repos and endpoint into `config/config.toml` (in the project folder, created at the first start, ignored by Git) and keys into `config/.env`. The terminal route (edit `config/config.toml`, start from `config/config.example.toml`; run
`cockpit-server token set <name>`) leads to the same result; both can be mixed. Back to the [README](../README.md).
All keys end up in `config/.env`, see [configuration.md](configuration.md#credentials-the-env-file).

Without setup the workspace stays empty. As soon as **a source is configured and its token is in `.env`**, the backend
syncs; a source without configuration or token stays off.

## What only you can do

These steps cannot be automated; the assistant walks you through each one:

- **Atlassian account and API token.** You need access to your Jira site. The easy way is an **API token**: create
  it yourself at [id.atlassian.com](https://id.atlassian.com/manage-profile/security/api-tokens) ("Create API
  token", pick a name and an expiry date, copy) and enter site, email and token in the assistant. Only if you
  prefer signing in in the browser, create your own OAuth app in the
  [Atlassian developer console](https://developer.atlassian.com/console/myapps/) (the second way in the Jira step, "OAuth"). Every person
  creates their **own** app because the client secret cannot be shared.
- **GitHub App.** Create your own GitHub App in your account (or organisation) and install it on the repos.
  Again: your own app, your own secret.
- **Access to a model.** Either an Anthropic API key (direct), or the address and key of a proxy such as LiteLLM
  that your team runs. Without access Cockpit keeps working, but the real agent does not.
- **Container runtime:** install and start it (see the [README](../README.md#prerequisites)).

Keys (client secrets, tokens, the key of the model endpoint) are typed or pasted into the password fields of the
assistant. They go only to the server on your computer, which stores them in the **`config/.env` file**; never in browser storage, never in a tracked file. If you prefer the terminal, use
`cockpit-server token set <name>` (see [Credentials: the `.env` file](configuration.md#credentials-the-env-file)).

## Ready-made archive
 For every version tag (`v…`) `.github/workflows/release.yml`
builds an archive for macOS (Apple Silicon) with `cockpit-server`, `apps/cockpit/dist`, the agent image sources
and `start-cockpit.command` in one folder, no Rust or Trunk needed. Unzip, right-click `start-cockpit.command` ›
"Open". The programs are not signed, hence the macOS prompt. Intel Macs, Windows and Linux build themselves
(steps above).

## Agent: container, image, model

The agent implements a ticket in a **container** (Claude Agent SDK, Claude Code or Codex, one container per run).

The agent implements a ticket in a **container** (Claude Agent SDK, Claude Code or Codex, one container per run). One-time setup:

1. **Start a container runtime:** Rancher Desktop with the engine "dockerd (moby)" (not "containerd") *or* Podman
   (`podman machine start`). The assistant ("Einrichten" › "Container") shows whether it runs.
2. **Build the image:** click "Image bauen" there (or `POST /api/agent/image`). The build downloads packages and
   takes a few minutes; progress and errors appear below in plain text. The output is collapsed to the last five lines; the arrow
   at its top right opens the complete output (an error opens it by itself). `/api/health` reports under `agent`
   the state of the image: `image_state` is `missing`, `outdated`, `current` or `unknown` (not checkable). The image
   always has the same name, `localhost/cockpit-agent:local`; the build gives it the label `cockpit.fingerprint`, and
   Cockpit compares it with a fingerprint of the sources (see `apps/agent/README.md`). So a new Cockpit version
   detects an old image by itself and asks to rebuild; there is no tag to raise. A build by hand
   (`docker build -f apps/agent/Containerfile -t localhost/cockpit-agent:local apps/agent`, or `podman`) has no label
   and counts as outdated, so use the assistant.
3. **Provider and endpoint:** in the "Modell" step choose the tool (Claude Agent SDK, Claude Code or Codex; see
   [configuration.md](configuration.md#model-provider-and-endpoint)), the endpoint (directly at the provider, your own proxy such as LiteLLM, or
   another compatible endpoint), store the key (`config/.env`, name `agent-key`, variable `AGENT_KEY`) and pick the model from the list that
   "Modelle laden" fetches from the endpoint (free text as a fallback). "Aus dem Container testen" checks that a
   container can reach the endpoint. If the proxy is only reachable over a **VPN**, the container machine of
   Rancher Desktop or Podman may not know the VPN route or name; the test then says that your Mac reaches the
   proxy but the container does not. Connect the VPN first and restart Rancher Desktop or the Podman machine.
4. **Connect GitHub and choose the repos:** the agent clones through the backend, one repo per run. In the
   assistant, step "GitHub" › "Repos auswählen" shows the repos your access can reach (read-only
   `GET /api/github/repos`); you tick the ones you work with. There is ONE list: a ticked repo is a source of the
   pull-request comparison and a choice for the agent. Per repo you set the base branch (preset: the repo's default
   branch) and optionally Jira projects that route to it (`[[agent.repos]]`, see
   [configuration.md](configuration.md#several-repos)); one repo is the default. The folder `apps/agent/` is looked up next to `apps/cockpit/dist`
   (`[agent] dir` or `COCKPIT_AGENT_DIR` overrides).

If something is not set up, no run starts; the message says in plain language what is missing and where in the
assistant to fix it.

## Jira

**Default: API token (three entries).** In the app under "Einrichten" › "Jira" enter (1) the address of your Jira
site (`acme.atlassian.net`; a pasted address with `https://` and a path is understood), (2) the email of your
Atlassian account, (3) the token. "Token-Seite öffnen" opens `https://id.atlassian.com/manage-profile/security/api-tokens`
in a new tab: create a token, pick a name and expiry, copy and paste it into the password field. The token goes to
`PUT /api/secrets/jira` (into `config/.env`), site and email to `PUT /api/config` (into `config.toml`). "Verbindung prüfen"
(`POST /api/jira/check`, read only: `GET /rest/api/3/myself`) then tests whether "Jira antwortet und dich kennt"
and otherwise names the cause in plain language (wrong address, token rejected, no permission, unreachable). Afterwards the
assistant suggests the Jira projects to choose from (or "Alle Projekte"), like the drop-down "Jira-Projekt" in the inbox's filter dialog. The
summary shows whether the sign-in is "mit API-Token" or "mit OAuth"; "Token erneuern" returns to the step and
"Trennen" deletes the token from `config/.env` (`DELETE /api/secrets/jira`).

**Security.** The token has the rights of your Atlassian account. As long as Cockpit should only read, use an
account or role without write permissions; a short expiry limits the damage. The token lives only in `config/.env` and never
in browser storage, URL, database, `config.toml` or log.

**Choosing a project and a sprint view.** If the Jira site has several projects (boards), pick one in the inbox: the button
"Filter" opens the filter dialog, and "Jira-Projekt" is its first field. The list comes from Jira (`GET /api/jira/projects`,
behind it `GET /rest/api/3/project/search`: all projects your token may see) plus the projects of the tickets already
present. The choice is the action `SelectJiraProject` and is stored. With a project selected the inbox shows only its tickets
(pull requests stay visible, they belong to no project), and the choice takes precedence over `projects` and `jql` from
`config.toml`. "Alle Projekte" restores those.

What the sync fetches follows the choice (read queries only, never a change in Jira) and always covers **all tickets of
other people** too, not only yours (what is neither assigned to nor watched by you appears under "Team" with "Im Projekt"):

| Project | Sprint in the dialog | Query |
|---------|----------------------|-------|
| none | Aktueller Sprint (default) | as before: `sprint in openSprints()` (restricted to `projects` from `config.toml`), or your own `jql` |
| `BAU` | Aktueller Sprint | `project = BAU AND sprint in openSprints()` |
| `BAU` or none | Alle offenen | `project in (BAU) AND statusCategory != Done` |
| `BAU` or none | Backlog | … `AND sprint is EMPTY` |
| `BAU` or none | Zuletzt aktualisiert | … `AND updated >= -14d` |

Without a chosen project a sprint view other than "Aktueller Sprint" asks for the projects from `config.toml` (all projects
if there are none); an own `jql` in `config.toml` is only replaced when a project is chosen. The sprint view is saved with the
other filters (action `SetInboxFilter`); after a change of project or sprint view the server syncs immediately.

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

## GitHub

The token (`echo "$GITHUB_TOKEN" | cargo run -p cockpit-server -- token set github`) is read anew on every use, so a
new token works without restart.

| Service | Token | Rights |
|---------|-------|--------|
| GitHub | *fine-grained personal access token*, limited to the configured repos | repository permissions **Contents** (read and write: create a branch), **Pull requests** (read and write: read, create the pull request of a finished agent run, send a review, merge), **Issues** (read and write: comments), **Commit statuses** (read-only: state of the checks), **Metadata** (read-only, automatic). There is **no "Checks" permission** for a fine-grained token: GitHub offers it only to GitHub Apps. If Cockpit should only read, everything *Read-only* is enough |
| Jira | *API token* of the Atlassian account | the account needs "Browse projects", "Add comments" and "Edit issues / Transition issues" in the projects; sign-in with the email from the configuration |

**Checks with a fine-grained token.** The Checks API (`GET /repos/{repo}/commits/{sha}/check-runs`) answers `403
Resource not accessible by personal access token`, because the permission "Checks" exists only for GitHub Apps. Cockpit
then reads the combined commit status (`GET /repos/{repo}/commits/{sha}/status`, permission *Commit statuses*): `success`
is "Checks bestanden", `failure` or `error` is failed, `pending` is running. A commit without any status (a repo that
reports only check runs, e.g. GitHub Actions) shows "Checks nicht sichtbar", also when the status is denied too; the
pull request itself still loads and merging is not blocked by it. The denial is remembered per repo while the server
runs, so later syncs do not ask again; a restart or another token (a change of the sign-in method) asks anew. Check logs,
annotations and "Restart" show a plain message that this access cannot read checks. A GitHub App sign-in sees all of it.

## OAuth (alternative to tokens)

Instead of a token you sign in in the browser. Tokens and client secret live only in the backend's `config/.env`, never in
browser, repo, database or log. In the assistant the Jira step has a picker ("API-Token" / "OAuth"); only the chosen way is shown (the
connected way is preselected, otherwise the token). If both exist, OAuth wins over the token.

**Removing an OAuth app again.** In the Jira step "Client-ID entfernen" (with a confirmation) deletes the client ID
from `config.toml` and the client secret from `config/.env`; "Trennen" has the option "Auch Client-ID und Secret vergessen".
The same works through the API: `PUT /api/config` with an empty `client_id` (`{"atlassian":{"client_id":""}}` or
`{"github_oauth":{"client_id":""}}`) removes the whole section (including `site`), and
`DELETE /api/secrets/{atlassian-oauth|github-oauth}` removes the secret. `{"atlassian_site": "…"}` sets
`[atlassian] site` (empty removes it) – needed when the account approves several sites.

1. *Register the apps.* GitHub: a **GitHub App** (callback `http://127.0.0.1:8787/api/oauth/github/callback`, "Expire
   user authorization tokens" on, repository permissions as in the GitHub rights table, install the app on the repos).
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
5. *Disconnect.* "Trennen" in the app or `POST /api/oauth/{service}/revoke` deletes the tokens in `config/.env`. For GitHub
   the backend also revokes the token at the service (`DELETE /applications/{client_id}/token`). Atlassian offers no
   call for that; revoke access at id.atlassian.com/manage-profile/apps (the app points there). The tokens are always
   deleted, even if the service does not answer. The client secret stays.
6. *When it stops working.* An expired or revoked access shows "not signed in" at the source.
   If the service rejects the refresh, the backend deletes the tokens: the service counts as disconnected and you sign
   in again. Network errors or service outages keep the tokens.

`GET /api/health` reports under `sign_in.github` and `sign_in.jira` `connected`, `kind` (`oauth`, `token` or `none`),
`oauth_possible` (app set up and secret present), `expired` (the sign-in ran out and cannot be renewed) and, for
Atlassian, the `site`. **This is the one place that decides whether a service is connected** (`ServiceStatus::resolve`
in `cockpit-core`): a working sign-in goes before the token, an expired one falls back to the token, a token counts
when its key is in `config/.env`. The summary, the sidebar, inbox, review, the assistant steps and the sync state all read it.

### GitHub App and organization

A user token of a GitHub App only sees repos on accounts where the app is **installed**. If your repos belong to an
organization but the app is only installed on your personal account, the sign-in works and the lists stay empty:
GitHub answers `404` (not visible) or `403` (permission missing) for each repo, and the sync keeps that as a small
warning. Cockpit therefore asks `GET /api/github/access` (read-only: `GET /repos/{owner}/{repo}` per configured repo,
plus `GET /user/installations` when something is hidden) and shows a hint instead of "No pull requests yet" in the
review tab, the inbox, the GitHub step and the last step of the assistant. The hint lists the affected repos and
these steps:

1. Install the app on the organization that owns the repos (the page of your app on GitHub › *Install*; Cockpit
   links to it when GitHub tells the app's name).
2. If the organization is not offered, open the app's settings on GitHub. Under **Where can this GitHub App be
   installed?** choose **Any account**.
3. An owner of the organization may have to approve the installation.
4. The app needs the repository permissions **Contents** and **Pull requests** (at least read).
5. After you change permissions, accept them on the installation page; until then GitHub keeps the old ones.

**Cannot install the app on the organization?** Use a token instead: in the GitHub step choose "API-Token", save a
fine-grained personal access token (resource owner: the organization) and Cockpit uses it for the sync and the repo
list (`GET /user/repos` lists organization repos). Saving the token stores `auth = "token"` in `[github]`; an existing
app sign-in stays stored but is not used, and the step says so (disconnect it there if you do not need it). The hint
above mentions this alternative.

Then press *Check again*. Per repo the report says `visible`, `not_visible` (404), `no_permission` (403/401) or
`unreachable` (network, quota: nothing is known, so it is no "empty" either). "No pull requests" is only shown when
every configured repo is `visible`.

**Not verified against the real services.** Token exchange, refresh, revocation and site choice are only tested
against recorded or mocked responses (`cargo test`). Open until the first real run: whether Atlassian accepts
`code_verifier` for a web app with a client secret (otherwise it is dropped there), whether the GitHub App accepts
PKCE, and GitHub's installation redirect (`installation_id`, `setup_action`): without a `state` from our start the
backend rejects it; the app is already installed, sign-in always begins in Cockpit.

## The setup assistant in detail

The page "Einrichten" (`#/setup`) is a guided assistant in six steps with a progress indicator (Container, Jira,
GitHub, Modell, Live, Fertig). It appears by itself on first start when nothing is set up (once per browser; the browser
remembers only this hint) and is always reachable via "Einrichten" in the "Mehr" menu of the navigation. Every step can be skipped; the
summary says in plain language what works (check mark), what is open and what to verify, with "Ansehen" leading to
the right step. The texts assume no prior knowledge. The current step is part of the address (`#/setup/container`,
`#/setup/jira`, `#/setup/github`, `#/setup/model`, `#/setup/live`, `#/setup/done`), so a reload keeps it and the back
button walks through the steps; `#/setup` alone opens the first step that is not done yet (or the summary). Error
messages never ask you to run terminal commands: they name the step and field ("… trage es im Schritt „GitHub“ im
Feld „Client-Secret“ ein") and link to that step. The terminal commands remain for developers (see
[Configuration](configuration.md)).

- *Container.* Detects Docker or Podman; if neither exists, the step explains what a container is and points to
  Rancher Desktop (engine **"dockerd (moby)"**) or Podman. "Erneut prüfen" asks again.
- *Jira.* A picker chooses between "API-Token" (three steps, connection test and project choice, see Jira) and
  "OAuth" (walkthrough of the developer console, client ID and secret, optional site, sign-in; client ID and secret
  can be removed again). Only the chosen way is shown.
- *GitHub.* A switch "Token (empfohlen)" / "GitHub App (OAuth)". **Token:** create a fine-grained personal access
  token at `github.com/settings/personal-access-tokens/new`, select only the repos you want (start with test repos),
  repository permissions Contents, Pull requests and Issues *Read and write*, Commit statuses and Metadata *Read-only*
  (no Checks: GitHub offers that permission only to GitHub Apps; for read-only use everything *Read-only* is enough), and
  put it into the password field (stored in `config/.env`). The wizard never asks for a classic token. **GitHub App:** callback URL to copy (`127.0.0.1`, not `localhost`),
  webhook "Active" off, Setup URL empty, no private key needed – only client ID and client secret, saved together
  with one "Speichern" ("Client-ID entfernen" deletes the ID), then "Install App" and "Mit GitHub anmelden". Both
  ways end in the repo list and "Verbindung prüfen" (`POST /api/github/check`: `GET /user` and `GET /repos/{owner}/{name}`
  per saved repo; read-only; never prints the token). The list comes from `GET /user/repos` (token; owner,
  collaborator, organization member; at most 500) or from the app's installations (OAuth).
- *Sign-in method (Jira and GitHub).* The picker is a persisted choice (`auth = "token"` or `"oauth"` in
  `[jira]` / `[github]`), not only a switch of the visible form: the way you pick is the one the server uses. Saving a
  token picks "token", completing an OAuth sign-in picks "oauth", and switching the picker to a way that is already
  set up stores it at once. If "token" is chosen while an OAuth sign-in is still stored, a short note says it is kept
  but not used; "trennen" in the OAuth way removes it (nothing is revoked automatically). Without a stored choice
  the older rule applies: a working OAuth sign-in first, then the token. See
  [Configuration](configuration.md#choosing-the-sign-in-method-auth).
- *Modell.* Provider, endpoint address, model, key and connection test.
- *Live (optional).* Switch for webhooks, the public address of your tunnel (display only), a generated secret per
  source (shown once), copyable target addresses, short instructions for cloudflared/ngrok and for the webhook at
  GitHub and Jira, "Zuletzt empfangen" per source and a local test button. See "Live updates and webhooks".

Inputs are validated like at server start; an invalid value leaves `config.toml` unchanged and the answer names the reason. Changes
take effect without restart (only the sync interval and the port wait). Secret fields send the value once over loopback to
the server, which stores it in `config/.env` (guarantees: [security.md](security.md)).

## Live updates and webhooks

By default the backend polls Jira and GitHub every `interval_s` (120 s) and pushes changes to the browser through
the event stream (`GET /api/events`; the browser reconnects by itself and shows "zuletzt abgeglichen"). That needs
no setup and stays the default. **Webhooks are an optional accelerator**: when something changes, GitHub or Jira
calls your Cockpit and it syncs that source right away.

A webhook is **only a trigger**. Cockpit never takes data from the request: after a valid one it syncs the affected
source through the normal, authenticated API (merged: at most one sync per source every 5 seconds). Webhooks need
a tunnel because GitHub and Jira cannot reach `127.0.0.1`. **Cockpit installs and starts nothing**; you run the
tunnel and create the webhooks yourself, Cockpit never creates or changes a webhook at a service.

1. **Switch on.** In the assistant, step "Live" (or in `config.toml`): `[server] webhooks = true`. Without it both
   routes answer `404`.
2. **Secret.** The assistant generates a random secret per source (`GITHUB_WEBHOOK_SECRET`, `JIRA_WEBHOOK_SECRET` in
   `config/.env`) and shows it **once**, so you can paste it into GitHub or Jira. It is never shown again; "Neues Secret
   erzeugen" replaces it. You can also set the variables by hand. Without a secret nothing is accepted.
3. **Tunnel** (manual, pick one; the port is the one of `COCKPIT_PORT`, 8787 by default):

   ```bash
   cloudflared tunnel --url http://127.0.0.1:8787     # quick tunnel, new https://….trycloudflare.com address at every start
   ```

   ```bash
   ngrok http 127.0.0.1:8787                          # needs an ngrok account; free plans may change the address
   ```

   Enter the https address under "Öffentliche Adresse" (`[server] public_url`). It is only used to show you the two
   target addresses; Cockpit never calls it.
4. **GitHub.** Repo or App › Webhooks › Add webhook. Payload URL `https://<tunnel>/api/webhooks/github`, content type
   `application/json`, the secret, events: Pull requests, Pull request reviews, Pull request review comments, Check
   runs, Check suites, Issue comments. GitHub sends a `ping` right away (answered with `200`).
5. **Jira.** Administration › System › WebHooks › Create. URL `https://<tunnel>/api/webhooks/jira`, events: issue
   created/updated/deleted and comment created/updated/deleted. Jira Cloud admin webhooks with a secret sign the body
   (`X-Hub-Signature: sha256=…`); put the secret in that field. If your Jira cannot sign (older versions,
   automation rules) append the secret to the URL: `…/api/webhooks/jira?token=<secret>`. That is **weaker**: the
   secret travels in the address, may appear in logs of the tunnel or Jira, and does not protect the body. Prefer the
   signature.
6. **Check.** "Lokal testen" in the assistant sends a signed request from Cockpit to itself over loopback and proves
   secret and route without any external service. "Zuletzt empfangen" per source (time and event type only, kept in
   memory) shows whether a real delivery arrived; at GitHub use Recent Deliveries › Redeliver.

Routes, status codes and signature checks: [architecture.md](architecture.md#webhooks) and [security.md](security.md#webhooks-optional-internet-facing).

**Safety net for the poll interval.** While webhooks of a source arrive regularly (a relevant event within the last
two intervals) that source is polled only every `[sync] webhook_fallback_interval_s` (default 5 × `interval_s`, at
most 10 minutes). When they stop, it returns to `interval_s` by itself. A quiet repository therefore also returns
to the normal interval; that costs only a few requests. A manual "Jetzt abgleichen" always asks everything.

**Tunnel safety.** Through the tunnel only `/api/webhooks/github` and `/api/webhooks/jira` are meant to be reachable.
Everything else in Cockpit still insists on a loopback peer and a loopback `Host`, so it answers `403` to a foreign
name, but do not rely on one layer: configure the tunnel to forward only the path prefix `/api/webhooks/` where it
can (cloudflared ingress rules with `path`, ngrok traffic policy, or a reverse proxy in front), keep the secrets
long and secret, and stop the tunnel when you do not need it. Details and the threat model:
[docs/security.md](security.md).
