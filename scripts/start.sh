#!/usr/bin/env bash
# Starts Cockpit locally: builds the app if needed and starts the server.
#
#   scripts/start.sh            build (if needed) and start
#   scripts/start.sh --open     also open the browser as soon as the server answers
#                               (port 8787 or COCKPIT_PORT; if it is only in config.toml, pass COCKPIT_PORT)
#   scripts/start.sh --release  server as a release build (slower to build, faster in operation)
#
# Without a terminal: double-click `start-cockpit.command` in the repo folder (it calls this script with --open).
# Nothing is installed and nothing is created outside the repo. Configuration and keys:
# see the README ("Backend starten"). Environment variables such as COCKPIT_PORT,
# COCKPIT_DB or COCKPIT_SEED=demo are passed to the server unchanged.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

open_browser=0
profile=()
for arg in "$@"; do
  case "$arg" in
    --open) open_browser=1 ;;
    --release) profile=(--release) ;;
    -h | --help)
      sed -n '2,12p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
      exit 0
      ;;
    *)
      echo "Unbekannte Option: $arg (erlaubt: --open, --release, --help)" >&2
      exit 2
      ;;
  esac
done

# Under launchd the PATH is short; add the usual places of Homebrew and rustup.
export PATH="$PATH:/opt/homebrew/bin:/usr/local/bin:$HOME/.cargo/bin"

missing() {
  echo "„$1“ fehlt. $2" >&2
  exit 1
}
command -v cargo > /dev/null || missing cargo "Rust ist noch nicht installiert. Anleitung und Befehl: https://rustup.rs (dauert etwa 5 Minuten). Danach das Terminal-Fenster schließen und das Skript neu starten."
command -v trunk > /dev/null || missing trunk "Trunk ist noch nicht installiert. Mit Homebrew: brew install trunk – oder: cargo install trunk --locked (dauert etwa 5 bis 10 Minuten)."

# The app is outdated if it is missing or any source is newer than the result.
app_outdated() {
  local target="apps/cockpit/dist/index.html"
  [ -f "$target" ] || return 0
  [ -n "$(find apps/cockpit/src apps/cockpit/index.html apps/cockpit/style.css apps/cockpit/Trunk.toml \
    crates/cockpit-core/src crates/ui Cargo.toml Cargo.lock \
    -type f -newer "$target" -print -quit 2> /dev/null)" ]
}

if app_outdated; then
  echo "Baue die App nach apps/cockpit/dist …"
  (cd apps/cockpit && trunk build --release)
else
  echo "App in apps/cockpit/dist ist aktuell."
fi

port="${COCKPIT_PORT:-8787}"
if [ "$open_browser" = 1 ]; then
  # Wait in the background until the server answers (building can take a while), then open.
  (
    for _ in $(seq 1 240); do
      if curl -fsS "http://127.0.0.1:$port/api/health" > /dev/null 2>&1; then
        open "http://127.0.0.1:$port"
        exit 0
      fi
      sleep 1
    done
  ) &
fi

echo "Starte den Server (Ctrl-C beendet ihn). Port: COCKPIT_PORT, sonst config.toml, sonst 8787 …"
exec cargo run ${profile[@]+"${profile[@]}"} -p cockpit-server
