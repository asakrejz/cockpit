#!/bin/sh
# `cockpit-server` in the image. `docker exec` and the healthcheck run as the image's user (root); this makes
# sure the server binary never does: as root it drops to `cockpit` first, as any other user it just runs.
set -eu
server=/usr/local/libexec/cockpit-server
if [ "$(id -u)" = 0 ]; then
  export HOME=/home/cockpit
  exec setpriv --reuid=cockpit --regid=cockpit --clear-groups --no-new-privs "$server" "$@"
fi
exec "$server" "$@"
