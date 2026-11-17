#!/usr/bin/env bash
# Starts Cockpit locally: builds the app if needed and starts the server.
#
#   scripts/start.sh            build (if needed) and start
#   scripts/start.sh --open     also open the browser as soon as the server answers
#                               (port 8787 or COCKPIT_PORT; if it is only in config.toml, pass COCKPIT_PORT)
#   scripts/start.sh --release  server as a release build (slower to build, faster in operation)
#
# Without a terminal: double-click `start-cockpit.command` in the repo folder (it calls this script with --open).
# Nothing is installed and nothing is created outside the repo: configuration and database live in
# config/ (config.toml, cockpit.db; created at the first start, ignored by Git), keys in .env.
# Details: docs/configuration.md. Environment variables such as COCKPIT_PORT,
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
      sed -n '2,14p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
      exit 0
      ;;
    *)
      echo "Unknown option: $arg (allowed: --open, --release, --help)" >&2
      exit 2
      ;;
  esac
done

# Under launchd the PATH is short; add the usual places of Homebrew and rustup.
export PATH="$PATH:/opt/homebrew/bin:/usr/local/bin:$HOME/.cargo/bin"

missing() {
  echo "\"$1\" is missing. $2" >&2
  exit 1
}
command -v cargo > /dev/null || missing cargo "Rust is not installed yet. Instructions and command: https://rustup.rs (takes about 5 minutes). Afterwards close the terminal window and start the script again."
command -v trunk > /dev/null || missing trunk "Trunk is not installed yet. With Homebrew: brew install trunk - or: cargo install trunk --locked (takes about 5 to 10 minutes)."

# The app is outdated if it is missing or any source is newer than the result.
app_outdated() {
  local target="apps/cockpit/dist/index.html"
  [ -f "$target" ] || return 0
  [ -n "$(find apps/cockpit/src apps/cockpit/index.html apps/cockpit/style.css apps/cockpit/Trunk.toml \
    crates/cockpit-core/src crates/ui Cargo.toml Cargo.lock \
    -type f -newer "$target" -print -quit 2> /dev/null)" ]
}

if app_outdated; then
  echo "Building the app into apps/cockpit/dist ..."
  (cd apps/cockpit && trunk build --release)
else
  echo "App in apps/cockpit/dist is up to date."
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

echo "Starting the server (Ctrl-C stops it). Port: COCKPIT_PORT, else config.toml, else 8787 ..."
exec cargo run ${profile[@]+"${profile[@]}"} -p cockpit-server
