# Cockpit as a container image

Instead of building Cockpit yourself you can pull a ready-made image and start it. Every merge to `main` publishes
it as `ghcr.io/<owner>/cockpit:latest` (`linux/amd64` and `linux/arm64`, so also Apple Silicon). Replace `<owner>`
with the GitHub account or organisation that owns the repository. The repository and the image are public: pulling
needs no sign-in. Security notes: [security.md](security.md).

## Start and update (one command)

Get `compose.yaml` (it is in the repository) and run, in its folder:

```bash
docker compose up -d
```

Then open <http://127.0.0.1:8787>. The setup assistant takes you from there.

**The same command updates Cockpit.** Run it again whenever you want: it pulls `latest` (`pull_policy: always`),
recreates the container only if the image changed, and otherwise leaves the running container alone. Your data
is in the volume and survives. (`podman compose up -d` works the same.)

Without compose, use the script (`docker` or `podman`, detected in this order; `COCKPIT_CONTAINER=podman` overrides):

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

### Only the image (no clone, no compose)

Everything above also works with `docker run` alone; you do not need the repository:

```bash
docker pull ghcr.io/<owner>/cockpit:latest
docker run -d --name cockpit --restart unless-stopped \
  -p 127.0.0.1:8787:8787 \
  -v cockpit-config:/app/config \
  -v /var/run/docker.sock:/var/run/docker.sock \
  ghcr.io/<owner>/cockpit:latest
```

Update: `docker pull ghcr.io/<owner>/cockpit:latest`, `docker rm -f cockpit`, then the same `docker run` again (the volume
keeps your data). Without the socket line Cockpit runs without the agent. `docker run` fails with "already exists" when the
container exists; remove it first.

## What lives where

The container is stateless. Everything that belongs to you is in **one volume** mounted at `/app/config`:

| File | Content |
| --- | --- |
| `config.toml` | configuration (created at the first start, written by the setup assistant) |
| `cockpit.db` | database |
| `.env` | **your keys** (Jira, GitHub, model endpoint), mode `600` |

Keys never enter the image. Set them in the setup assistant, or from a terminal:

```bash
docker exec -i -u cockpit cockpit cockpit-server token set jira      # the value is read from standard input
```

Back up or remove the volume with `docker volume`; a bind mount (`-v "$HOME/cockpit-config:/app/config"`) works too, if the folder
is writable for user id `10001` (the user inside the image).

## Update

Run the same command again: `docker compose up -d` (or `scripts/cockpit-container.sh`). See above.

## Agent containers (opt-in)

Cockpit starts agent containers with the `docker` command. In the image that works only if you give it the container
engine of your host: mount its socket (`-v /var/run/docker.sock:/var/run/docker.sock`, in compose the override file
`compose.agent.yaml`, with the script `--agent`). The `docker` CLI is part of the image (about 39 MB on disk).

> **A mounted socket is root-equivalent access to your host.** Whoever controls Cockpit (or the code that runs in it)
> can start any container, mount any folder and read or change anything the engine can reach. Do this only on a
> machine you trust and only if you want the agent. Without the mount Cockpit works, but the agent features show the
> container engine as "not running".

Socket paths (the left side of the `-v` line; the right side stays `/var/run/docker.sock`):

| Engine | Socket on the host |
| --- | --- |
| Docker (Linux, Docker Desktop) | `/var/run/docker.sock` |
| Rancher Desktop (macOS) | `~/.rd/docker.sock` |
| Podman (Linux, rootless) | `$XDG_RUNTIME_DIR/podman/podman.sock` |
| Podman machine (macOS) | path from `podman machine inspect --format '{{.ConnectionInfo.PodmanSocket.Path}}'` |

Compose and the script take the path from `DOCKER_SOCK`:

```bash
DOCKER_SOCK=~/.rd/docker.sock docker compose -f compose.yaml -f compose.agent.yaml up -d
scripts/cockpit-container.sh --agent
```

Why an override file and not a profile: a profile would start next to the normal `cockpit` service and both would want
the same port; an override changes the one service, so name and port stay the same. Switching modes recreates the
container; the volume stays.

### Socket access without `--user root`

The image starts its entrypoint (`container/entrypoint.sh`) as root for one reason: to find out which group may use the
socket. It reads the group id of `/var/run/docker.sock` (or of the path in `DOCKER_HOST=unix://…`), then drops to the
user `cockpit` (uid 10001) with `setpriv` and gives that process the socket's group as a supplementary group. No
group is created, `/etc` is not touched. Without a socket it just drops. **The server never runs as root**: the
`cockpit-server` command in the image (healthcheck, `docker exec`) drops root the same way.

Docker Desktop and Rancher Desktop for macOS present the socket as `root:root` with mode `660` (group id 0). Then the
supplementary group is `0`. That means the server process may read and write files inside the Cockpit container that are
group-owned by root and group-accessible; the image has none that matter (config, database and keys belong to
`cockpit`). The socket is root-equivalent access to the host anyway; that is the risk you accept (see above), and the
group does not add to it. If the socket is world-accessible (mode `…6`/`…7`), no group is added at all.

`docker exec -i -u cockpit cockpit cockpit-server token set jira` is the form for commands in a running container; the
command drops root itself, but `-u cockpit` states it.

### No "paths must match": copy instead of bind mounts

The engine runs on the host, so a folder that Cockpit hands to an agent container with `-v <path>:/work` would be
resolved *on the host*, where a path of the Cockpit container does not exist. Inside the image Cockpit therefore uses
the **copy strategy** (`COCKPIT_AGENT_IO=copy`, chosen automatically when Cockpit runs in a container):

1. `docker create` the agent container (same switches as before: label `cockpit.run`, no capabilities, no token in the
   arguments, secrets only as `-e NAME` from the environment of the process), without mounts.
2. `docker cp` the prepared clone(s) to `/work` and the job file to `/job`. The files get `a+rwX` for this copy because
   `docker cp` makes root the owner and the agent runs as another user; the copy only lives in the throw-away container.
3. `docker start -a -i`: the runner talks to the backend over stdout/stdin exactly as before.
4. When it ends (also when it is stopped, halted by a limit or cancelled) the backend copies `/work` back into a fresh
   folder and swaps it in for the clone (so files the agent deleted are gone too), then `docker rm -f`. A halted run
   resumes from that clone. Plan and review runs change nothing and are not copied out.

Everything else is unchanged: the backend checks the branch pattern and the path rules and pushes, the container has no
GitHub token. The native start keeps the bind mounts (`COCKPIT_AGENT_IO=bind`, the default outside a container).
`COCKPIT_AGENT_IO=bind|copy` overrides the choice. One difference: a plan or review run no longer gets a read-only mount
(there is none); its copy is discarded unread.

The agent image itself is **not** part of this image: "Build image" in the app builds it from the sources inside the
image (`/app/apps/agent`) with the engine of your host (the build context is sent through the CLI, so no host path is
needed). The native start (`start-cockpit.command`, see the [README](../README.md)) remains the best-tested way to use
the agent.

### What the setup assistant says in a container

Inside the image the "Container" step knows it runs in a container (`COCKPIT_IN_CONTAINER=1`, also detected via
`/.dockerenv` or `/run/.containerenv`; `GET /api/health` reports `container.in_container` and `container.socket`:
`missing`, `denied` or `reachable`). Without a socket it explains how to mount it instead of "not installed"; with a
socket the user cannot use (`denied`) it says to pull the current image; with a reachable socket
it proceeds as normal: "reachable" is enough, there is nothing else to set up.

Webhooks (a tunnel into Cockpit) need the native start as well: the webhook routes accept only a loopback peer.

## Size and the `latest`-only policy

- The compressed image is far below 150 MB (Debian slim, `git`, the stripped server, the web app and the docker
  CLI). The workflow measures every platform before it pushes and **fails at 450 MB or more**.
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

The first build compiles all Rust dependencies (several minutes). They are a layer of their own that only changes with
`Cargo.toml` or `Cargo.lock`, so after a change to the source a rebuild compiles just the workspace crates. The same
layers are what the image workflow caches.
