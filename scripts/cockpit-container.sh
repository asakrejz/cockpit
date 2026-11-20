#!/usr/bin/env bash
# Start or update the Cockpit container without compose. Idempotent: run it as often as you like.
#
#   scripts/cockpit-container.sh [up|stop|logs|status]
#
#   up      pull the image; start Cockpit if missing or stopped; replace the container if the image is newer
#           (the volume stays, so your data stays). Default.
#   stop    stop the container (it stays; `up` starts it again)
#   logs    follow the logs
#   status  show whether it runs and which image it uses
#
# Works with docker and podman (detected; COCKPIT_CONTAINER=podman|docker overrides). Environment:
#   COCKPIT_IMAGE   image            (default ghcr.io/asakrejz/cockpit:latest)
#   COCKPIT_NAME    container name   (default cockpit)
#   COCKPIT_VOLUME  volume name      (default cockpit-config)
#   COCKPIT_PORT    host port        (default 8787, published on 127.0.0.1 only)
#   COCKPIT_PULL    0 = do not pull  (for locally built images)
# The volume is never removed by this script.
set -euo pipefail

image="${COCKPIT_IMAGE:-ghcr.io/asakrejz/cockpit:latest}"
name="${COCKPIT_NAME:-cockpit}"
volume="${COCKPIT_VOLUME:-cockpit-config}"
port="${COCKPIT_PORT:-8787}"
pull="${COCKPIT_PULL:-1}"

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
  local args=(run -d --name "$name" --restart unless-stopped
    -p "127.0.0.1:${port}:8787" -v "${volume}:/app/config")
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
    "$engine" pull "$image" >/dev/null || die "pull failed. For a private image log in first: ${engine} login ghcr.io (see docs/container.md)."
  fi
  local new_id old_id
  new_id="$("$engine" image inspect --format '{{.Id}}' "$image")"

  if ! exists; then
    say "No container '${name}' yet, creating it."
    run_container
    return
  fi
  old_id="$("$engine" container inspect --format '{{.Image}}' "$name")"
  if [ "$old_id" != "$new_id" ]; then
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

case "${1:-up}" in
  up) cmd_up ;;
  stop) cmd_stop ;;
  logs) exists || die "no container '${name}'."; exec "$engine" logs -f "$name" ;;
  status) cmd_status ;;
  -h|--help|help) sed -n '2,17p' "$0" ;;
  *) die "unknown command '$1'. Use: up | stop | logs | status" ;;
esac
