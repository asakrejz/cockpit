#!/bin/bash
# Double-click start for macOS: starts Cockpit and opens the browser.
#
# * From the cloned repo: calls scripts/start.sh (builds the app the first time; that takes a
#   few minutes).
# * From the ready-made archive (cockpit-server and apps/cockpit/dist next to this file): starts
#   directly, without Rust and Trunk.
#
# The first time, macOS may ask whether you want to open the file: right-click the file,
# "Open", "Open" again. To quit, close the window or press Ctrl-C.

cd "$(dirname "$0")" || exit 1

wait_for_key() {
  echo
  read -r -n 1 -s -p "Press any key to close the window ..."
  echo
}

if [ -x scripts/start.sh ]; then
  scripts/start.sh --open
  code=$?
elif [ -x ./cockpit-server ]; then
  # A downloaded archive carries the "from the internet" mark; without removing it the server does not start.
  xattr -dr com.apple.quarantine . 2> /dev/null
  port="${COCKPIT_PORT:-8787}"
  (
    for _ in $(seq 1 60); do
      if curl -fsS "http://127.0.0.1:$port/api/health" > /dev/null 2>&1; then
        open "http://127.0.0.1:$port"
        exit 0
      fi
      sleep 1
    done
  ) &
  echo "Cockpit is running. To quit: close this window or press Ctrl-C."
  ./cockpit-server
  code=$?
else
  echo "scripts/start.sh or cockpit-server is missing here."
  echo "Is this file in the folder of the Cockpit repo (or of the unpacked archive)?"
  code=1
fi

echo
if [ "$code" -ne 0 ]; then
  echo "Cockpit exited with an error (code $code). Read the messages above or"
  echo "the \"Quick start for beginners\" guide in the README."
else
  echo "Cockpit has exited."
fi
wait_for_key
exit "$code"
