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
  read -r -n 1 -s -p "Drücke eine Taste, um das Fenster zu schließen …"
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
  echo "Cockpit läuft. Zum Beenden: dieses Fenster schließen oder Ctrl-C."
  ./cockpit-server
  code=$?
else
  echo "Hier fehlt scripts/start.sh oder cockpit-server."
  echo "Liegt diese Datei im Ordner des Cockpit-Repos (oder des entpackten Archivs)?"
  code=1
fi

echo
if [ "$code" -ne 0 ]; then
  echo "Cockpit wurde mit einem Fehler beendet (Code $code). Lies die Meldungen oben oder"
  echo "die Anleitung „Schnellstart für Einsteiger“ in der README."
else
  echo "Cockpit wurde beendet."
fi
wait_for_key
exit "$code"
