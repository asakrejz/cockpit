#!/usr/bin/env bash
# Startet Cockpit lokal: baut die App bei Bedarf und startet den Server.
#
#   scripts/start.sh            bauen (falls nötig) und starten
#   scripts/start.sh --open     zusätzlich den Browser öffnen, sobald der Server antwortet
#                               (Port 8787 oder COCKPIT_PORT; steht er nur in config.toml, COCKPIT_PORT mitgeben)
#   scripts/start.sh --release  Server als Release-Build (langsamer zu bauen, schneller im Betrieb)
#
# Ohne Terminal: Doppelklick auf „Cockpit starten.command“ im Repo-Ordner (ruft dieses Skript mit --open auf).
# Es wird nichts installiert und nichts außerhalb des Repos angelegt. Konfiguration und
# Schlüssel: siehe README („Backend starten“). Umgebungsvariablen wie COCKPIT_PORT,
# COCKPIT_DB oder COCKPIT_SEED=demo gibt das Skript unverändert an den Server weiter.
set -euo pipefail

wurzel="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$wurzel"

oeffnen=0
profil=()
for arg in "$@"; do
  case "$arg" in
    --open) oeffnen=1 ;;
    --release) profil=(--release) ;;
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

# Unter launchd ist der PATH knapp; die üblichen Orte von Homebrew und rustup ergänzen.
export PATH="$PATH:/opt/homebrew/bin:/usr/local/bin:$HOME/.cargo/bin"

fehlt() {
  echo "„$1“ fehlt. $2" >&2
  exit 1
}
command -v cargo > /dev/null || fehlt cargo "Rust ist noch nicht installiert. Anleitung und Befehl: https://rustup.rs (dauert etwa 5 Minuten). Danach das Terminal-Fenster schließen und das Skript neu starten."
command -v trunk > /dev/null || fehlt trunk "Trunk ist noch nicht installiert. Mit Homebrew: brew install trunk – oder: cargo install trunk --locked (dauert etwa 5 bis 10 Minuten)."

# Die App ist veraltet, wenn sie fehlt oder irgendeine Quelle neuer ist als das Ergebnis.
app_veraltet() {
  local ziel="apps/cockpit/dist/index.html"
  [ -f "$ziel" ] || return 0
  [ -n "$(find apps/cockpit/src apps/cockpit/index.html apps/cockpit/style.css apps/cockpit/Trunk.toml \
    crates/cockpit-core/src crates/ui Cargo.toml Cargo.lock \
    -type f -newer "$ziel" -print -quit 2> /dev/null)" ]
}

if app_veraltet; then
  echo "Baue die App nach apps/cockpit/dist …"
  (cd apps/cockpit && trunk build --release)
else
  echo "App in apps/cockpit/dist ist aktuell."
fi

port="${COCKPIT_PORT:-8787}"
if [ "$oeffnen" = 1 ]; then
  # Im Hintergrund warten, bis der Server antwortet (das Bauen kann dauern), dann öffnen.
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
exec cargo run ${profil[@]+"${profil[@]}"} -p cockpit-server
