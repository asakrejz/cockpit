# Cockpit as a container image

Instead of building Cockpit yourself you can pull a ready-made image and start it. Every merge to `main` publishes
it as `ghcr.io/<owner>/cockpit:latest` (`linux/amd64` and `linux/arm64`, so also Apple Silicon). Replace `<owner>`
with the GitHub account or organisation that owns the repository. Security notes: [security.md](security.md).

## Sign in to the registry (private repo)

The repository is private, so the image is private too. Create a classic personal access token with the scope
`read:packages` (GitHub › Settings › Developer settings › Personal access tokens) and sign in once:

```bash
echo "<token>" | docker login ghcr.io -u <github-user> --password-stdin
```

(`podman login ghcr.io` works the same.) The token only reads packages; do not give it more.

## Start and update (one command)

Get `compose.yaml` (it is in the repository and in the release archive) and run, in its folder:

```bash
docker compose up -d
```

Then open <http://127.0.0.1:8787>. The setup assistant takes you from there.

**The same command updates Cockpit.** Run it again whenever you want: it pulls `latest` (`pull_policy: always`),
recreates the container only if the image changed, and otherwise leaves the running container alone. Your data
is in the volume and survives. (`podman compose up -d` works the same.)

Without compose, use the script (`docker` or `podman`, detected; `COCKPIT_CONTAINER=podman` overrides):

```bash
scripts/cockpit-container.sh          # up (default): pull, start or replace, or report "up to date"
scripts/cockpit-container.sh status   # also: stop, logs
```

- **Stop:** `docker compose down` removes the container and keeps the volume. **Never add `-v`**: it deletes the
  volume, i.e. your config, database and keys.
- **Publish on `127.0.0.1` only** (the compose file does). A bare `-p 8787:8787` would expose the server to your
  network. See [security.md](security.md#running-in-a-container).
- Another host port: `COCKPIT_PORT=18787 docker compose up -d` (the script reads the same variable). The server
  then accepts that port in the `Host` header (`COCKPIT_PUBLIC_PORT`, set for you). For the OAuth sign-in the callback
  address of your OAuth app must use that port too; the simplest setup is the same port on both sides.
- `docker ps` shows the state `healthy` once the server answers (`cockpit-server health` is the healthcheck).
- **If you started Cockpit earlier with `docker run --name cockpit`:** remove that container once
  (`docker rm -f cockpit`; the volume `cockpit-config` stays and is reused), then use the commands above.
  Compose may warn that the volume "was not created by Docker Compose"; that is harmless.

### Manual `docker run` (reference)

Only for reference; it fails with "already exists" when run twice. Remove an existing container first
(`docker rm -f cockpit`), and pull yourself to update:

```bash
docker pull ghcr.io/<owner>/cockpit:latest
docker run -d --name cockpit --restart unless-stopped \
  -p 127.0.0.1:8787:8787 \
  -v cockpit-config:/app/config \
  ghcr.io/<owner>/cockpit:latest
```

## What lives where

The container is stateless. Everything that belongs to you is in **one volume** mounted at `/app/config`:

| File | Content |
| --- | --- |
| `config.toml` | configuration (created at the first start, written by the setup assistant) |
| `cockpit.db` | database |
| `.env` | **your keys** (Jira, GitHub, model endpoint), mode `600` |

Keys never enter the image. Set them in the setup assistant, or from a terminal:

```bash
docker exec -i cockpit cockpit-server token set jira      # the value is read from standard input
```

Back up or remove the volume with `docker volume`; a bind mount (`-v "$HOME/cockpit-config:/app/config"`) works too, if the folder
is writable for user id `10001` (the user inside the image).

## Update

Run the same command again: `docker compose up -d` (or `scripts/cockpit-container.sh`). See above.

## Agent containers (opt-in)

Cockpit starts agent containers with the `docker` command. In the image that works only if you give it the container
engine of your host: mount its socket. The `docker` CLI is part of the image (about 39 MB on disk). In `compose.yaml` this is a commented block.

> **A mounted socket is root-equivalent access to your host.** Whoever controls Cockpit (or the code that runs in it)
> can start any container, mount any folder and read or change anything the engine can reach. Do this only on a
> machine you trust and only if you want the agent. Without the mount Cockpit works, but the agent features show the
> container engine as "not running".

Socket paths:

| Engine | Socket on the host |
| --- | --- |
| Docker (Linux, Docker Desktop) | `/var/run/docker.sock` |
| Rancher Desktop (macOS) | `~/.rd/docker.sock` |
| Podman (Linux, rootless) | `$XDG_RUNTIME_DIR/podman/podman.sock` |
| Podman machine (macOS) | path from `podman machine inspect --format '{{.ConnectionInfo.PodmanSocket.Path}}'` |

The socket belongs to root, the image runs as a normal user, so the process needs access to it: add
`--group-add "$(stat -c %g /var/run/docker.sock)"` on Linux, or run with `--user root` (on macOS VMs usually needed).

**Paths must match.** The engine runs on the host, so the folders Cockpit gives to an agent container
(`<data folder>/…`) are resolved *on the host*. A named volume does not work for that. Use a host folder, mounted at
the **same absolute path** inside, and point Cockpit at it. Compose and the script do this for you:

```bash
mkdir -p "$HOME/cockpit-config"
docker compose -f compose.yaml -f compose.agent.yaml up -d     # compose; override file, see why below
scripts/cockpit-container.sh --agent                           # without compose
```

Both read `COCKPIT_DATA` (host folder, default `$HOME/cockpit-config`) and `DOCKER_SOCK` (socket on the host,
default `/var/run/docker.sock`; Rancher Desktop on macOS: `~/.rd/docker.sock`), run as `user: root`, mount the socket
and set `COCKPIT_CONFIG`, `COCKPIT_DB` and `COCKPIT_ENV_FILE` into that folder. Why an override file and not a
profile: a profile would start next to the normal `cockpit` service and both would want the same port; an override
changes the one service, so name and port stay the same. Switching modes recreates the container; your data stays
in the volume or the folder.

Plain `docker run` form (remove an existing container first, `docker rm -f cockpit`):

```bash
docker run -d --name cockpit --restart unless-stopped --user root \
  -p 127.0.0.1:8787:8787 \
  -v /var/run/docker.sock:/var/run/docker.sock \
  -v "$HOME/cockpit-config:$HOME/cockpit-config" \
  -e COCKPIT_CONFIG="$HOME/cockpit-config/config.toml" \
  -e COCKPIT_DB="$HOME/cockpit-config/cockpit.db" \
  -e COCKPIT_ENV_FILE="$HOME/cockpit-config/.env" \
  ghcr.io/<owner>/cockpit:latest
```

(Replace the socket path for your engine.) The agent image itself is **not** part of this release: "Build image" in
the app builds it from the sources inside the image (`/app/apps/agent`) with the engine of your host. This opt-in
setup has not been run against a real agent end to end; if it gives you trouble, use the native start
(`start-cockpit.command`, see the [README](../README.md)), which is the tested way to use the agent.

### Move the data to a host folder

If you have been running with the named volume, copy your files once into the host folder (stop first so the database
is not written meanwhile).:

```bash
docker compose down                          # keeps the volume (never `-v`); or: scripts/cockpit-container.sh stop
mkdir -p "$HOME/cockpit-config"
docker run --rm -v cockpit-config:/from:ro -v "$HOME/cockpit-config:/to" --user root \
  --entrypoint sh ghcr.io/<owner>/cockpit:latest -c 'cp -a /from/. /to/'
docker compose -f compose.yaml -f compose.agent.yaml up -d
```

That copies `config.toml`, `cockpit.db` and `.env` (mode `600` is kept). The old volume stays as a backup.

### What the setup assistant says in a container

Inside the image the "Container" step knows it runs in a container (`COCKPIT_IN_CONTAINER=1`, also detected via
`/.dockerenv` or `/run/.containerenv`; `GET /api/health` reports `container.in_container` and `container.socket`:
`missing`, `denied` or `reachable`). Without a socket it explains the two options above instead of "not installed";
with a socket the user may not use (`denied`) it points to `--group-add`/`user: root`; with a working socket it
proceeds as normal.

Webhooks (a tunnel into Cockpit) need the native start as well: the webhook routes accept only a loopback peer.

## Size and the `latest`-only policy

- The compressed image is far below 150 MB (Debian slim, `git`, the stripped server, the web app and the docker
  CLI). The workflow measures every platform before it pushes and **fails at 450 MB or more**; the registry limit
  of the private repo is 500 MB.
- There is only ever **one image: `latest`**. After each push the workflow deletes every other version of the
  package, including untagged ones. Only the pushed index and the platform manifests it references stay (they are
  separate versions; deleting them would break `docker pull`). So storage does not grow, and there is no older
  version to go back to: pin by digest yourself (`docker inspect`) if you need a fixed one.
- Details of the build: [`Containerfile`](../Containerfile), [`.github/workflows/image.yml`](../.github/workflows/image.yml),
  [`scripts/prune-image-versions.sh`](../scripts/prune-image-versions.sh).

## Build it yourself

```bash
docker build -f Containerfile -t cockpit:local .
docker run --rm -p 127.0.0.1:8787:8787 -v cockpit-config:/app/config cockpit:local
```
