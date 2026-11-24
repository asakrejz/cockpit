#!/usr/bin/env bash
# Start or update the Cockpit container without compose. Idempotent: run it as often as you like.
#
#   scripts/cockpit-container.sh [up|stop|logs|status] [--agent]
#
#   up      pull the image; start Cockpit if missing or stopped; replace the container if the image is newer
#           (the volume stays, so your data stays). Default.
#   stop    stop the container (it stays; `up` starts it again)
#   logs    follow the logs
#   status  show whether it runs and which image it uses
#   --agent (with up) opt-in: lets Cockpit start agent containers on YOUR engine. SECURITY: the mounted engine
#           socket is root-equivalent access to your host; only on a machine you trust. Same volume as without
#           the flag; the image drops root itself and picks up the group of the socket (docs/container.md,
#           "Agent containers"). Switching between modes replaces the container, the data stays.
#
# Works with docker and podman (detected, docker first; COCKPIT_CONTAINER=podman|docker overrides). Environment:
#   COCKPIT_IMAGE   image            (default ghcr.io/asakrejz/cockpit:latest)
#   COCKPIT_NAME    container name   (default cockpit)
#   COCKPIT_VOLUME  volume name      (default cockpit-config)
#   COCKPIT_PORT    host port        (default 8787, published on 127.0.0.1 only)
#   COCKPIT_PULL    0 = do not pull  (for locally built images)
#   DOCKER_SOCK     engine socket for --agent (default /var/run/docker.sock)
# The volume is never removed by this script.
set -euo pipefail

image="${COCKPIT_IMAGE:-ghcr.io/asakrejz/cockpit:latest}"
name="${COCKPIT_NAME:-cockpit}"
volume="${COCKPIT_VOLUME:-cockpit-config}"
port="${COCKPIT_PORT:-8787}"
pull="${COCKPIT_PULL:-1}"
sock="${DOCKER_SOCK:-/var/run/docker.sock}"
agent=0
cmd=up
for a in "$@"; do
  case "$a" in
    --agent) agent=1 ;;
    up|stop|logs|status|help|-h|--help) cmd="$a" ;;
    *) printf "error: unknown argument '%s'. Use: up | stop | logs | status, optionally --agent\n" "$a" >&2; exit 1 ;;
  esac
done
mode=standard; [ "$agent" = 1 ] && mode=agent

say() { printf '%s\n' "$*"; }
die() { printf 'error: %s\n' "$*" >&2; exit 1; }

engine="${COCKPIT_CONTAINER:-}"
if [ -z "$engine" ]; then
  if command -v docker >/dev/null 2>&1; then engine=docker
  elif command -v podman >/dev/null 2>&1; then engine=podman
  else die "neither docker nor podman found. Install one, or set COCKPIT_CONTAINER."; fi
fi
command -v "$engine" >/dev/null 2>&1 || die "container engine '$engine' not found."

exists() { "$engine" container inspect "$name" >/dev/null 2>&1; }
running() { [ "$("$engine" container inspect --format '{{.State.Running}}' "$name" 2>/dev/null)" = true ]; }

run_container() {
  local args=(run -d --name "$name" --restart unless-stopped --label "cockpit.mode=${mode}"
    -p "127.0.0.1:${port}:8787")
  if [ "$agent" = 1 ]; then
    [ -S "$sock" ] || die "no engine socket at ${sock}. Set DOCKER_SOCK (see docs/container.md for the path of your engine)."
    say "WARNING: --agent mounts the engine socket. That is root-equivalent access to this machine."
    args+=(-v "${sock}:/var/run/docker.sock")
  fi
  args+=(-v "${volume}:/app/config")
  # The server must accept the published port in the Host header.
  [ "$port" = 8787 ] || args+=(-e "COCKPIT_PUBLIC_PORT=${port}")
  "$engine" "${args[@]}" "$image" >/dev/null
  say "Cockpit is running: http://127.0.0.1:${port}"
}

cmd_up() {
  if [ "$pull" = 0 ]; then
    say "Skipping pull (COCKPIT_PULL=0), using the local image ${image}."
  else
    say "Pulling ${image} ..."
    "$engine" pull "$image" >/dev/null || die "pull failed. Check the network and the image name (${image}); see docs/container.md."
  fi
  local new_id old_id old_mode
  new_id="$("$engine" image inspect --format '{{.Id}}' "$image")"

  if ! exists; then
    say "No container '${name}' yet, creating it."
    run_container
    return
  fi
  old_id="$("$engine" container inspect --format '{{.Image}}' "$name")"
  old_mode="$("$engine" container inspect --format '{{index .Config.Labels "cockpit.mode"}}' "$name")"
  [ -n "$old_mode" ] || old_mode=standard
  if [ "$old_mode" != "$mode" ]; then
    say "Switching '${name}' from ${old_mode} to ${mode} mode. Replacing the container (your data is kept)."
    "$engine" stop "$name" >/dev/null
    "$engine" rm "$name" >/dev/null
    run_container
  elif [ "$old_id" != "$new_id" ]; then
    say "A newer image is available. Replacing container '${name}' (volume '${volume}' is kept)."
    "$engine" stop "$name" >/dev/null
    "$engine" rm "$name" >/dev/null
    run_container
  elif running; then
    say "Cockpit is already running and up to date (http://127.0.0.1:${port})."
  else
    say "Container '${name}' is stopped and up to date, starting it."
    "$engine" start "$name" >/dev/null
    say "Cockpit is running: http://127.0.0.1:${port}"
  fi
}

cmd_stop() {
  exists || { say "No container '${name}', nothing to stop."; return; }
  if running; then "$engine" stop "$name" >/dev/null; say "Stopped '${name}'. Data is kept in volume '${volume}'."
  else say "'${name}' is not running."; fi
}

cmd_status() {
  if ! exists; then say "No container '${name}'. Run: $0 up"; return; fi
  "$engine" container inspect --format 'Container {{.Name}}: {{.State.Status}}, image {{.Config.Image}}, started {{.State.StartedAt}}' "$name"
}

case "$cmd" in
  up) cmd_up ;;
  stop) cmd_stop ;;
  logs) exists || die "no container '${name}'."; exec "$engine" logs -f "$name" ;;
  status) cmd_status ;;
  *) sed -n '2,23p' "$0" ;;
esac
