#!/bin/bash
# Doppelklick-Start für macOS: startet Cockpit und öffnet den Browser.
#
# * Aus dem geklonten Repo: ruft scripts/start.sh auf (baut die App beim ersten Mal; das dauert
#   ein paar Minuten).
# * Aus dem fertigen Archiv (cockpit-server und cockpit/dist liegen neben dieser Datei): startet
#   direkt, ohne Rust und Trunk.
#
# Beim ersten Mal fragt macOS eventuell, ob du die Datei öffnen willst: Rechtsklick auf die Datei,
# „Öffnen“, noch einmal „Öffnen“. Zum Beenden das Fenster schließen oder Ctrl-C drücken.

cd "$(dirname "$0")" || exit 1

warten() {
  echo
  read -r -n 1 -s -p "Drücke eine Taste, um das Fenster zu schließen …"
  echo
}

if [ -x scripts/start.sh ]; then
  scripts/start.sh --open
  code=$?
elif [ -x ./cockpit-server ]; then
  # Ein heruntergeladenes Archiv trägt die Markierung „aus dem Internet“; ohne sie startet der Server.
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
warten
exit "$code"
