#!/bin/sh
# Entrypoint of the Cockpit image. Starts as root for one reason only: to learn which group may use the
# engine socket (if one is mounted). Then it drops to the user `cockpit` (uid 10001) and starts the server.
# The server itself never runs as root.
#
#   * No socket: drop to `cockpit` without extra groups. Cockpit runs, the agent shows "no engine".
#   * Socket /var/run/docker.sock (or the path in DOCKER_HOST=unix://...): `cockpit` gets the socket's group id as
#     a supplementary group for this process only. No group is created, /etc is not touched, so it also works
#     with --read-only.
#   * Socket owned by gid 0 (root:root 660, as Docker Desktop and Rancher Desktop for macOS present it): the
#     supplementary group is 0. What that means: the server can read and write files in the container that are
#     group-owned by root and group-accessible; the image has none that matter (config and data belong to
#     `cockpit`). The socket itself is root-equivalent on the host anyway, which is the real risk (docs/security.md).
#     Not needed, and not done, when the socket is world-accessible.
#   * Started with another user (docker run --user 10001): nothing to drop, the server is started as is.
#
# The folders of config, database and keys must be writable for uid 10001 (a volume is; a bind mount has to be).
# If one is not, this script says so before the server fails on it.
set -eu

server=/usr/local/libexec/cockpit-server
run_as=cockpit

if [ "$(id -u)" != 0 ]; then
  exec "$server" "$@"
fi

# The home of `cockpit` (it does not exist; the point is that it is not /root, which the server user may not read).
export HOME=/home/cockpit

groups=""
sock=/var/run/docker.sock
case "${DOCKER_HOST:-}" in
  unix://*) sock="${DOCKER_HOST#unix://}" ;;
esac
if [ -S "$sock" ]; then
  gid="$(stat -c %g "$sock")"
  mode="$(stat -c %a "$sock")"
  # Other users may use it already (mode ...6 or ...7 for the "other" digit): no group needed.
  case "$mode" in
    *[67]) ;;
    *) groups="$gid" ;;
  esac
fi

for f in "${COCKPIT_CONFIG:-/app/config/config.toml}" "${COCKPIT_DB:-/app/config/cockpit.db}" \
  "${COCKPIT_ENV_FILE:-/app/config/.env}"; do
  dir="$(dirname "$f")"
  [ -d "$dir" ] || continue
  if ! setpriv --reuid="$run_as" --regid="$run_as" --clear-groups test -w "$dir"; then
    echo "cockpit: the folder $dir is not writable for the user 'cockpit' (uid 10001)." >&2
    echo "cockpit: use a volume, or make that folder writable for uid 10001 (chown 10001:10001)." >&2
  fi
done

if [ -n "$groups" ]; then
  exec setpriv --reuid="$run_as" --regid="$run_as" --groups="$groups" --no-new-privs "$server" "$@"
fi
exec setpriv --reuid="$run_as" --regid="$run_as" --clear-groups --no-new-privs "$server" "$@"
