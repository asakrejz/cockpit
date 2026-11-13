# Rust Playground

Ein Lernprojekt: **Web-Bausteine in Rust, kompiliert zu WebAssembly.** Jeder Baustein
bekommt im Playground einen eigenen Reiter, ähnlich wie bei Storybook. So lernst du Rust
an kleinen, sichtbaren Beispielen und baust nebenbei eine Bibliothek, die du in anderen
Projekten wiederverwenden kannst.

Im Repo liegen zwei Apps, die sich die Bausteine teilen:

- **Playground** (`apps/playground/`): zeigt jeden Baustein einzeln.
- **Cockpit** (`apps/cockpit/`): ein Dashboard für Jira, GitHub und Agenten, gebaut aus denselben Bausteinen. Es braucht den lokalen Server (`apps/server/`), der Daten und Schlüssel hält.

Technik:

- UI-Framework: [Leptos](https://leptos.dev) (läuft komplett im Browser)
- Build-Tool: [Trunk](https://trunkrs.dev) (Rust → WASM → fertige Seite)
- Aussehen: Design **„Baukasten"** (iOS 26 / Liquid Glass), gesteuert über Design-Tokens

## Schnellstart

Wer ohne Programmierkenntnisse einsteigt, liest zuerst „Schnellstart für Einsteiger“ unten.

```bash
# einmalig
rustup target add wasm32-unknown-unknown   # passiert dank rust-toolchain.toml auch automatisch
cargo install trunk --locked                # oder: brew install trunk

scripts/start.sh --open             # Cockpit (oder Doppelklick auf „Cockpit starten.command“): App bauen (falls nötig), Server starten, Browser öffnen
(cd apps/playground && trunk serve --open)   # Playground auf http://localhost:8080
cargo test                          # Tests laufen nativ, ohne Browser
(cd apps/playground && trunk build --release)   # fertige Playground-Seite nach apps/playground/dist
```

Beim Lernen hilft: im Ordner `apps/playground` `trunk serve` laufen lassen, eine Datei ändern, Speichern – der Browser
lädt neu, und der Compiler sagt dir sofort, was nicht passt.

## Schnellstart für Einsteiger

Für alle, die noch nie ein Terminal benutzt haben. Du brauchst etwa **eine Stunde**, davon das meiste Warten auf
Downloads. Gebraucht werden ein Mac (Apple Silicon oder Intel) und eine Internetverbindung. Vorgebaute Archive gibt es
unter „Releases“ im Repo (siehe „Fertiges Archiv statt selbst bauen“ weiter unten); diese Anleitung baut selbst.

**1. Programme installieren** (einmalig; jeweils den Anweisungen auf der Seite folgen):

| Was | Wofür | Wo | Dauer |
|-----|-------|----|-------|
| **Rust** (über „rustup“) | übersetzt Cockpit | [rustup.rs](https://rustup.rs) – dort steht ein Befehl, den du ins Terminal einfügst (Programme › Dienstprogramme › Terminal) | 5–10 Min. |
| **Trunk** | baut die Oberfläche | im Terminal `cargo install trunk --locked` (nach Rust) oder mit [Homebrew](https://brew.sh) `brew install trunk` | 5–10 Min. |
| **Rancher Desktop** *oder* **Podman** | Container für den Agenten (erst später nötig) | [rancherdesktop.io](https://rancherdesktop.io) – beim ersten Start die Engine „dockerd (moby)“ wählen; oder [podman.io](https://podman.io/docs/installation) | 10 Min. |
| **Git** | holt den Code | meldet sich beim ersten Aufruf von selbst, sonst im Terminal `xcode-select --install` | 5–10 Min. |

**2. Cockpit holen.** Im Terminal (Befehle einzeln einfügen und mit Enter bestätigen):

```bash
git clone https://github.com/asakrejz/rust-playground.git ~/Documents/Web/rust-playground
```

Das Repo ist privat: Du brauchst dafür Zugriff und musst dich bei GitHub anmelden (Git fragt danach, oder du nimmst die
Adresse von der Seite des Repos unter „Code“).

**3. Starten per Doppelklick.** Öffne im Finder den Ordner `Documents/Web/rust-playground` und doppelklicke auf
**`Cockpit starten.command`**. Beim ersten Mal fragt macOS, ob du die Datei öffnen willst (Rechtsklick › „Öffnen“ › noch
einmal „Öffnen“). Ein Terminal-Fenster geht auf, baut die App (beim ersten Mal 5–15 Minuten, danach Sekunden) und
öffnet den Browser auf `http://127.0.0.1:8787`. Schließe das Fenster, um Cockpit zu beenden.

**4. Dem Assistenten folgen.** Beim ersten Start öffnet sich die Seite **„Einrichten“** von selbst; später erreichst du sie
über „Einrichten“ in der Seitenleiste. Sie führt dich in fünf Schritten (Container, Jira, GitHub, Modellzugang,
Zusammenfassung) und erklärt jeden Klick. Jeden Schritt kannst du überspringen und später nachholen.

**Was nur du tun kannst** – das lässt sich nicht automatisieren, der Assistent führt dich aber Schritt für Schritt hindurch:

- **Atlassian-Konto und API-Token.** Du brauchst Zugriff auf eure Jira-Site. Der einfache Weg ist ein **API-Token**: Du
  erzeugst ihn selbst unter [id.atlassian.com](https://id.atlassian.com/manage-profile/security/api-tokens) („Token
  erstellen“, Namen vergeben, Ablaufdatum wählen, kopieren) und trägst Site, E-Mail und Token im Assistenten ein. Nur wenn
  du lieber eine Anmeldung im Browser willst, legst du in der
  [Atlassian-Entwicklerkonsole](https://developer.atlassian.com/console/myapps/) eine eigene OAuth-App an (unter
  „Erweitert“). Jede Person legt ihre **eigene** App an, denn das Client-Secret ist nicht teilbar.
- **GitHub App.** Du legst in deinem GitHub-Konto (oder eurer Organisation) eine eigene GitHub App an und installierst sie
  auf den Repos. Auch hier gilt: eigene App, eigenes Secret.
- **Zugang zu einem Modell.** Entweder ein API-Key von Anthropic (direkt) oder Adresse und Key eines Proxys wie LiteLLM,
  den dein Team betreibt (dann auch den Modellnamen, der sich aber im Assistenten aus einer Liste wählen lässt). Ohne
  Zugang läuft Cockpit weiter, der echte Agent aber noch nicht.
- **Container-Programm** installieren und starten (siehe Tabelle oben).

Schlüssel (Client-Secrets, Tokens, Key des Modell-Endpunkts) tippst oder fügst du in die Passwortfelder des Assistenten ein. Sie gehen
nur an den Server auf deinem Rechner, der sie in der **`.env`-Datei im Projektordner** ablegt – nicht in den Browser-Speicher, nicht in eine
Datei. Wer lieber im Terminal arbeitet, nimmt `cockpit-server token setzen <name>` (siehe „Backend starten“).

**Fertiges Archiv statt selbst bauen.** Bei jedem Versions-Tag (`v…`) baut `.github/workflows/release.yml` ein Archiv für
macOS (Apple Silicon): `cockpit-server`, `apps/cockpit/dist` und `Cockpit starten.command` in einem Ordner, ohne Rust und Trunk.
Entpacken, Rechtsklick auf `Cockpit starten.command` › „Öffnen“. Die Programme sind nicht signiert, deshalb die
Rückfrage von macOS. Intel-Macs, Windows und Linux bauen selbst (Schritte oben). Der Workflow ist noch nie gelaufen
(es gab noch kein Tag); beim ersten Tag prüfen, ob das Archiv startet.

## Den Agenten einrichten

Der Agent setzt ein Ticket in einem **Container** um (Claude Agent SDK, ein Container pro Lauf). Einrichten, einmalig:

1. **Container-Laufzeit starten:** Rancher Desktop mit der Engine „dockerd (moby)“ (nicht „containerd“) *oder* Podman
   (`podman machine start`). Der Assistent („Einrichten“ › „Container“) zeigt, ob sie läuft.
2. **Image bauen:** dort auf „Image bauen“ klicken (oder `POST /api/agent/image`). Der Bau lädt Pakete aus dem Internet und
   dauert einige Minuten; Fortschritt und Fehler stehen in Klartext darunter. `/api/health` meldet unter `agent`, ob das Image
   da ist (`image inspect`). Von Hand: `docker build -f agent/Containerfile -t localhost/cockpit-agent:1 agent` (bzw. `podman`).
3. **Agent-Anbieter und Endpunkt:** Im Schritt „Modell“ wählst du das Werkzeug (Claude Agent SDK; Claude Code und Codex
   stehen als „bald“ da), den Endpunkt (direkt bei Anthropic, eigener Proxy wie LiteLLM, anderer kompatibler Endpunkt),
   legst den Key ab (`.env`, Name `agent-key`, Variable `AGENT_KEY`) und wählst das Modell aus der Liste, die „Modelle laden“ beim
   Endpunkt abfragt (Freitext als Ausweg). „Aus dem Container testen“ prüft, ob ein Container den Endpunkt erreicht.
   Liegt der Proxy nur im **VPN**, kennt die
   Container-Maschine von Rancher Desktop bzw. Podman die VPN-Route oder den Namen womöglich nicht: Dann meldet der Test
   „Dein Mac erreicht den Proxy, der Container aber nicht“ – VPN zuerst verbinden und Rancher Desktop bzw. die
   Podman-Maschine neu starten.
4. **GitHub verbinden und das Repo festlegen:** Der Agent klont über das Backend. Ist in `[github] repos` genau ein Repo,
   gilt es; sonst `[agent] repo = "owner/name"` in `config.toml`. Der Ordner `apps/agent/` wird neben `apps/cockpit/dist` gesucht
   (`[agent] dir` bzw. `COCKPIT_AGENT_DIR` überschreibt).

Ist etwas nicht eingerichtet, startet kein Lauf; die Meldung sagt in Klartext, was fehlt und wo im Assistenten man es holt.

**Ablauf eines Laufs.** „Agent starten“ legt (auf Wunsch) erst einen Plan vor: ein kurzer Container im Lesemodus. Nach deiner
Freigabe klont das Backend das Repo frisch, legt den Branch `claude/<ticket>-…` an und startet den Container (`run --rm`,
Label `cockpit.lauf=<schlüssel>`, `--add-host cockpit-host:host-gateway`, Repo per `-v <klon>:/work`, Läufer ohne Root;
Adresse, Key und Modell nur als `-e NAME`, die Werte stehen in der Umgebung des Prozesses, nie in der Argumentliste oder im
Log). Plan, Rückfragen, Verbrauch und Protokoll kommen als JSON-Zeilen aus dem Container (`agent/README.md`) und werden als
Aktionen des Agenten angewendet; Kosten rechnet `cockpit-core` mit den Preisen aus „Grenzen & Regeln“. Antworten und
Hinweise gehen über stdin zurück, „Pausieren“ ist `pause`/`unpause`.

**Was der Agent darf – und nie darf.**

- Er hat **kein GitHub-Token** und kann nicht pushen. Er committet auf `claude/<ticket>-…`; das Backend prüft den Branch in
  einem frischen Bare-Repo (nie im Klon, den der Container beschrieben hat), gegen die Pfad-Regeln des Repos, und pusht
  ihn dann. **Nie** der Hauptbranch, **nie** ein Force-Push, **nie** ein Merge. Den Pull Request legst du selbst an.
- Kommentare und Reviews des Agenten gehen erst nach deiner Freigabe raus.
- Er schreibt nur in `/work`; `canUseTool` im Läufer sperrt `.git`, verbotene Pfade, `git push`, Netzwerkbefehle.
- Grenzen (Budget pro Lauf und Monat, Zeitlimit, parallele Läufe) gelten wie bisher; bei Erreichen wird der Container
  beendet (`rm -f`), der Lauf wartet angehalten auf dich (der Klon bleibt zum Fortsetzen).
- Abbruch, Zeitlimit, Fehler im Container und ein Neustart des Servers (verwaiste Container werden per Label entfernt, Läufe als
  abgebrochen markiert) stehen in Klartext im Protokoll und am Lauf.

**Noch offen:** Folgeaufträge (Review-Kommentare umsetzen, roten Check untersuchen) laufen nur simuliert; der Pull Request
wird nicht automatisch angelegt; das Ticket wird nur über das Repo aus der Einstellung zugeordnet; Prüfung mit echten
Containern und echtem Modell steht aus (siehe `docs/ROADMAP.md`).

## Backend starten

Das Backend (`server/`, Binary `cockpit-server`) ist ein lokaler Dienst für eine Person. Es lauscht nur
auf `127.0.0.1`, liefert die gebaute Cockpit-App aus und hält Daten (SQLite) und Schlüssel
(`.env`-Datei im Projektordner) – Schlüssel verlassen das Backend nie. **Die App läuft nur mit dem Server**: Es gibt
keinen Modus ohne Backend mehr.

```bash
scripts/start.sh                # bauen (falls nötig) und starten: http://127.0.0.1:8787
scripts/start.sh --open         # zusätzlich den Browser öffnen
curl localhost:8787/api/health  # {"status":"ok","version":"…","verbindungen":{…},"abgleich":{…},"endpunkt":{…},"container":{…}}
```

`scripts/start.sh` (bash, macOS) prüft `cargo` und `trunk`, baut `apps/cockpit` nach `apps/cockpit/dist`, wenn die Ausgabe fehlt
oder eine Quelle neuer ist, und startet `cockpit-server`. Es installiert nichts. Mit `--release` baut es den Server
als Release-Build. Ohne das Skript: `(cd apps/cockpit && trunk build) && cargo run -p cockpit-server`. **Ohne Terminal** startet ein Doppelklick auf
`Cockpit starten.command` dasselbe (`--open`) und zeigt Fehler in lesbarem Deutsch.


**Beim ersten Start ist alles leer.** Der Server beginnt mit einem leeren Workspace; die Seite „Einrichten“ zeigt je Dienst
den Stand und den nächsten Schritt. Beispieldaten gibt es nur auf ausdrücklichen Wunsch: `COCKPIT_SEED=demo` (oder
`seed = "demo"` in `config.toml`) lädt sie, **aber nur, wenn noch kein Stand gespeichert ist** – ein vorhandener Stand
(auch ein leerer, mit Aktionen veränderter) geht vor. Für einen frischen Seed eine neue Datenbank nehmen
(`COCKPIT_DB=/pfad/zur/neuen.db`).

```bash
COCKPIT_SEED=demo COCKPIT_DB=/tmp/cockpit-demo.db scripts/start.sh --open   # Beispieldaten, eigene Datenbank
```

Konfiguration (alles optional): Datei `config.toml` im Konfigurationsverzeichnis (`~/.config/cockpit/`
unter Linux, `~/Library/Application Support/cockpit/` unter macOS), darüber Umgebungsvariablen `COCKPIT_PORT` (8787),
`COCKPIT_DIST` (`apps/cockpit/dist`), `COCKPIT_DB` (Datenverzeichnis des Nutzers, `cockpit/cockpit.db`), `COCKPIT_CONFIG`
(Pfad der Datei), `COCKPIT_SEED` (`leer` oder `demo`) und `COCKPIT_CONTAINER` (`docker` oder `podman`).

```toml
port = 8787
seed = "leer"                     # oder "demo", siehe oben
[github]                          # api_url ist optional
[jira]
site = "https://firma.atlassian.net"
email = "ich@firma.example"
[agent]
anbieter = "claude-agent-sdk"        # umgesetzt; "claude-code" und "codex" sind bekannt, aber noch nicht umgesetzt
container = "podman"              # oder "docker"; ohne Angabe wird erkannt
[agent.endpunkt]                  # Modellzugang, siehe „Einrichten“ unten
url = "https://litellm.example"   # leer oder weglassen = direkt bei Anthropic
modell = "claude-sonnet-5-5"
```

Ältere Dateien mit `[litellm] url/modell` und dem Schlüssel `litellm` werden weiter gelesen (Alias für `[agent.endpunkt]`
und `agent-key`); der Assistent schreibt beim Speichern das neue Format und übernimmt dabei die alten Werte.

Schlüssel stehen nie in der Datei. Sie kommen über die Standardeingabe in die `.env`-Datei:
`echo "$TOKEN" | cargo run -p cockpit-server -- token setzen github` (oder `jira`, `github-oauth`, `atlassian-oauth`,
`agent-key`; `token loeschen <name>` entfernt sie).
`/api/health` meldet je Verbindung nur ja/nein. Die Server-Crates (`server/`, `crates/integrations`)
laufen nur nativ; für Wasm schließt die CI sie aus (siehe `docs/ROADMAP.md`).

### Zugangsdaten: die `.env`-Datei

Alle Schlüssel (Jira-Token, GitHub-Token, Key des Agent-Endpunkts, Client-Secrets, Tokens der Anmeldung) liegen in einer
`.env`-Datei im Projektordner (Entscheidung vom 13.11.2026; das Risiko einer Datei im Repo-Ordner ist bewusst gewählt).
**Sie wird nie committet**: `.gitignore` schließt `.env` und `.env.*` aus, nur `.env.example` (nur Dummy-Werte) gehört ins Repo.

- **Anlegen:** Der Assistent und `cockpit-server token setzen <name>` legen die Datei bei Bedarf an (Rechte `600`).
  Von Hand: `cp .env.example .env` und die Werte eintragen.
- **Ort:** `.env` im Arbeitsverzeichnis des Servers (der Repo-Ordner; `scripts/start.sh` und „Cockpit starten.command“ wechseln
  dorthin). `COCKPIT_ENV_FILE=/pfad/zur/datei` biegt den Ort um (Betrieb, Tests). Der Assistent zeigt den Pfad im letzten Schritt.
- **Format:** `NAME=wert`, Kommentare mit `#`, andere Zeilen bleiben beim Schreiben erhalten. Werte mit Sonderzeichen stehen in
  einfachen Anführungszeichen. Die Token-Blobs der Anmeldung sind kompaktes JSON in einer Zeile (`'{"access":"…"}'`, kein base64).
  Schreiben ist atomar (temporäre Datei, dann umbenennen).
- **Vorrang:** Eine echte Umgebungsvariable gleichen Namens gilt vor der Datei. Ein Wert, der so gesetzt ist, lässt sich im
  Assistenten nicht überschreiben oder löschen.
- **Variablen:**

  | Name im Backend | Variable in der `.env` |
  |---|---|
  | `jira` | `JIRA_TOKEN` |
  | `github` | `GITHUB_TOKEN` |
  | `agent-key` (älter: `litellm`) | `AGENT_KEY` |
  | `github-oauth` | `GITHUB_OAUTH_CLIENT_SECRET` |
  | `atlassian-oauth` | `ATLASSIAN_OAUTH_CLIENT_SECRET` |
  | Tokens der GitHub-Anmeldung (vom Backend) | `GITHUB_OAUTH_TOKENS` |
  | Tokens der Atlassian-Anmeldung (vom Backend) | `ATLASSIAN_OAUTH_TOKENS` |

- **Sicherheitsnetz:** Beim Start und in `/api/health` (`env_datei`, sichtbar im letzten Schritt des Assistenten) warnt der
  Server, wenn Git die `.env` verfolgt, `.gitignore` sie nicht abdeckt oder die Rechte lockerer als `600` sind (beim Start
  werden sie auf `600` gesetzt). Die CI schlägt fehl, wenn irgendeine `.env*`-Datei (außer `.env.example`) verfolgt wird.
  Wurde je eine echte `.env` committet: Schlüssel sofort tauschen, `git rm --cached .env`.
- **Container:** Die `.env` kommt nie ins Image oder in den Container (`apps/agent/.dockerignore`); der Agent bekommt Werte nur
  als `-e NAME` vom Backend.
- **Alte Einträge aus dem Schlüsselbund** (vor dem 13.11.2026): `cargo run -p cockpit-server -- token uebernehmen` kopiert sie
  einmalig in die `.env`. Das liest nur den Schlüsselbund, löscht dort nichts, überschreibt nichts und läuft nie von selbst.

### Entwickeln mit `trunk serve`

`apps/cockpit/Trunk.toml` leitet `/api` an `http://127.0.0.1:8787` weiter (`[[proxy]]`). Server und Entwicklungsserver zusammen:

```bash
COCKPIT_DEV_PORT=8081 COCKPIT_SEED=demo cargo run -p cockpit-server   # Terminal 1
(cd apps/cockpit && trunk serve --open)                                    # Terminal 2, http://localhost:8081
```

Trunk schickt `Host` ohne Port und lässt den `Origin` der Entwicklungsseite stehen; beides lässt der Server nur zu, wenn
`COCKPIT_DEV_PORT` gesetzt ist (nur als Umgebungsvariable, nie in der Datei). Ohne sie gilt die strenge Prüfung unten.

### Beim Login starten (launchd, macOS)

`scripts/de.cockpit.server.plist` ist eine Vorlage für einen LaunchAgent. **Es wird nichts automatisch installiert.**

1. Einmal `scripts/start.sh` von Hand ausführen, damit alles gebaut ist.
2. Vorlage nach `~/Library/LaunchAgents/de.cockpit.server.plist` kopieren und die Platzhalter `/PFAD/ZUM/REPO` und
   `/Users/BENUTZER` ersetzen (siehe Kommentar in der Datei).
3. Laden: `launchctl bootstrap gui/$(id -u) ~/Library/LaunchAgents/de.cockpit.server.plist`
   · Entladen: `launchctl bootout gui/$(id -u) ~/Library/LaunchAgents/de.cockpit.server.plist`.
   Das Log steht in `~/Library/Logs/cockpit-server.log`.

### Wie App und Server zusammenarbeiten

Das Backend hält den `Workspace` (SQLite, Migration 2) und wendet jede Aktion selbst an; der Takt des simulierten
Agenten läuft ebenfalls dort. Im Browser liegt nichts (kein `localStorage`). Die App lädt den Stand, schickt Aktionen
und hört auf Änderungen:

| Route | Zweck |
|-------|-------|
| `GET /api/workspace` | Stand als JSON (Form von `cockpit_core::persist`, dazu `revision`) |
| `POST /api/actions` | Body `{"action": <Action>}`; wendet sie als „Du“ an. Antwort `{revision, zusammenfassung, protokolliert}`; eine abgelehnte Aktion ist `422` (unbekannte Form `400`) mit `{"fehler": "…"}` |
| `POST /api/sync` | „Jetzt abgleichen“ mit Jira und GitHub. Antwort `{aktiv, revision, protokolliert}`; `aktiv: false`, wenn keine Quelle eingerichtet ist |
| `GET /api/events` | Server-Sent Events: erst die aktuelle Revision, dann je Aktion ein Ereignis mit neuer Revision und den neuen Protokolleinträgen, dazu Keep-Alive |
| `GET /api/jira/projekte` | Jira-Projekte für die Auswahl in der Inbox: `{aktiv, projekte: [{key, name}]}`; `aktiv: false` ohne Jira oder Token |

| `GET /api/agent/image` / `POST /api/agent/image` | Stand von „Image bauen“ `{laeuft, vorhanden, fehler, zeilen}` bzw. Bau im Hintergrund starten |
| `GET /api/agent/pruefen/container` (Alias `/api/litellm/pruefen/container`) | Dieselbe Prüfung aus einem Container: `{geprueft, mac_erreicht, container_erreicht, key_ok, hinweis}` |
| `GET /api/agent/pruefen` (Alias `/api/litellm/pruefen`) | Verbindungstest zum Endpunkt: `{konfiguriert, erreichbar, key_ok, modell_vorhanden, hinweis}` (siehe „Einrichten“) |
| `GET /api/agent/modelle` | Die Modell-IDs des Endpunkts für das Drop-down: `{geladen, erreichbar, key_ok, modelle, hinweis}`; nur IDs, sortiert (Claude-Modelle zuerst), höchstens 200. Der Key bleibt im Backend |
| `GET /api/einrichtung` | Was der Assistent vorbelegt: Jira-Site/E-Mail/Projekte, Client-IDs, Repos, Anbieter, Endpunkt-Adresse und Modell, die beiden Callback-Adressen und je Schlüssel nur `{name, gesetzt}` |
| `PUT /api/konfiguration` | Schreibt Client-IDs, Jira, Repos sowie Agent-Anbieter und Endpunkt in `config.toml` (siehe „Einrichten in der App“). Antwort `{gespeichert, wirkt_sofort, hinweis}`; ungültige Eingaben sind `422` mit Klartext |
| `PUT /api/schluessel/{name}` | Legt einen Schlüssel in der `.env` ab. Body `{"wert": "…"}`, nur `github-oauth`, `atlassian-oauth`, `agent-key` (ältere Schreibweise `litellm`), `jira`, `github`. Antwort `{name, gesetzt}` – nie der Wert |
| `DELETE /api/schluessel/{name}` | „Trennen“ für Tokens: löscht `jira` oder `github` aus der `.env` (andere Namen: `404`). Antwort `{name, gesetzt: false}` |
| `POST /api/jira/pruefen` | Verbindungstest für Jira per API-Token, nur lesend (`GET /rest/api/3/myself`): `{ok, ursache, meldung, name}`, `ursache` = `ok`, `angaben`, `adresse`, `token`, `berechtigung`, `erreichbar`, `sonst`. Nie der Token |

Ist der Server nicht erreichbar (nicht gestartet, anderer Port), zeigt die App oben einen Hinweis mit „Erneut versuchen“;
die Seiten bleiben leer, es geht nichts verloren.

Alle Routen unter `/api` prüfen die Gegenstelle (nur Loopback) und den `Host`-Header (`127.0.0.1:<port>` oder
`localhost:<port>`, gegen DNS-Rebinding). Schreibende Anfragen brauchen zusätzlich `Content-Type: application/json`
und einen fehlenden oder eigenen `Origin`. Das gilt auch für `PUT /api/konfiguration` und `PUT /api/schluessel/{name}`.

### Jira verbinden

**Standard: API-Token (drei Angaben).** In der App unter „Einrichten“ › „Jira“ trägst du ein: (1) die Adresse deiner
Jira-Site (`firma.atlassian.net`; auch eine kopierte Adresse mit `https://` und Pfad wird verstanden), (2) die
E-Mail-Adresse deines Atlassian-Kontos, (3) den Token. „Token-Seite öffnen“ führt in einem neuen Tab zu
`https://id.atlassian.com/manage-profile/security/api-tokens`: „Token erstellen“, Namen vergeben, Ablaufdatum wählen,
kopieren und in das Passwortfeld einfügen. Der Token geht über `PUT /api/schluessel/jira` in die `.env`, Site und
E-Mail über `PUT /api/konfiguration` in die `config.toml`. Danach prüft „Verbindung prüfen“ (`POST /api/jira/pruefen`,
nur lesend: `GET /rest/api/3/myself`), ob „Jira antwortet und dich kennt“, und nennt sonst im Klartext die Ursache
(Adresse falsch, Token abgelehnt, keine Berechtigung, nicht erreichbar). Nach dem Test schlägt der Assistent die
Jira-Projekte zur Auswahl vor (oder „Alle Projekte“), wie das Drop-down in der Inbox. In der Zusammenfassung steht, ob die
Anmeldung „mit API-Token“ oder „mit OAuth“ läuft; „Token erneuern“ führt zurück in den Schritt, „Trennen“ löscht den Token
in der `.env` (`DELETE /api/schluessel/jira`).

**Sicherheit.** Der Token hat die Rechte deines Atlassian-Kontos. Solange Cockpit nur lesen soll, nimm ein Konto oder eine
Rolle ohne Schreibrechte; ein kurzes Ablaufdatum begrenzt den Schaden. Der Token liegt nur in der `.env` und kommt nie
in Browser-Speicher, Adresse, Datenbank, `config.toml` oder Log.

**Alternative: OAuth.** Wer keinen Token will, legt in der Atlassian-Entwicklerkonsole eine eigene OAuth-App an und
meldet sich im Browser an; im Assistenten steht das eingeklappt unter „Erweitert: Mit OAuth statt Token“, die Schritte
stehen weiter unten („Anmeldung per OAuth“). Ist beides da, geht OAuth vor dem Token.

### Mit echten Diensten verbinden

**Der bequeme Weg ist der Assistent:** Die Seite „Einrichten“ führt durch Container, Jira, GitHub und den Modellzugang
(siehe „Einrichten in der App“ weiter unten). Er schreibt Client-IDs, Jira-Site, Repos sowie Agent-Anbieter und Endpunkt selbst in die
`config.toml` und legt Schlüssel in der `.env` ab – ohne dass du Datei oder Terminal anfasst. **Der Terminal-Weg bleibt
als Alternative** und ist unten beschrieben: `config.toml` von Hand bearbeiten und `cockpit-server token setzen <name>`.
Beides führt zum selben Ergebnis und lässt sich mischen.

Ohne Einrichtung bleibt der Workspace leer (oder zeigt die Beispieldaten, wenn du `COCKPIT_SEED=demo` gewählt hast).
Sobald **eine Quelle eingerichtet ist und ihr Token in der `.env` liegt**, gleicht das Backend ab: Tickets, Pull
Requests, Kommentare, Checks und Diffs kommen von den Diensten; die (unveränderten) Beispieldaten eines Seeds verschwinden
beim ersten erfolgreichen Abruf. Eine Quelle ohne Konfiguration oder ohne Token bleibt aus.

**Der Terminal-Weg (Alternative zum Assistenten).**

**1. Konfiguration** (`config.toml`, siehe oben; keine Schlüssel in der Datei):

```toml
intervall_s = 120                        # Abfrage-Intervall, Standard 2 Minuten, mindestens 30

[github]
repos = ["firma/app", "firma/web"]       # owner/name, mehrere möglich
# api_url = "https://github.firma.example/api/v3"   # nur bei GitHub Enterprise

[jira]
site = "https://firma.atlassian.net"
email = "ich@firma.example"
projects = ["BAU", "WEB"]                # optional: mir zugewiesen oder beobachtet, offen, nur in diesen Projekten
# jql = "project = BAU AND labels = cockpit ORDER BY updated DESC"   # optional: eigene Abfrage statt der Projekte
```

Ohne `projects` und `jql` holt Jira alle mir zugewiesenen oder von mir beobachteten offenen Tickets. Bei GitHub
kommen Pull Requests der angegebenen Repos, bei denen ein Review von mir angefragt ist oder die mir gehören.

**Projekt wählen.** Hat die Jira-Site mehrere Projekte (Boards), wählst du in der Inbox oben im Drop-down „Jira-Projekt“
eines aus. Die Liste kommt von Jira (`GET /api/jira/projekte`, dahinter `GET /rest/api/3/project/search`: alle
Projekte, die dein Token sehen darf) und enthält dazu die Projekte der Tickets, die schon da sind. Die Wahl ist
die Aktion `SelectJiraProject` und bleibt gespeichert. Ist ein Projekt gewählt,

- zeigt die Inbox nur dessen Tickets (Pull Requests bleiben sichtbar, sie gehören zu keinem Projekt),
- holt der Abgleich **alle offenen Tickets des Projekts** (`project = KEY AND statusCategory != Done`), nicht nur
  deine. Was weder dir zugewiesen noch von dir beobachtet ist, steht mit „Im Projekt“ unter „Team“,
- hat sie Vorrang vor `projects` und `jql` aus `config.toml`. Mit „Alle Projekte“ gelten wieder diese.

Nach einem Wechsel gleicht der Server sofort ab.

**Filter der Inbox.** Neben dem Jira-Projekt filtert die Kopfzeile der Inbox nach **Repo**, **Person** (Besitzer),
**Status** und **Label**; jede Auswahl steht auf „Alle“, bis du etwas wählst, die Filter gelten zusammen (UND), und
„Filter zurücksetzen“ stellt alles zurück. Die Wahl gehört nur der Seite (wie Suchtext und Quelle) und wird nicht
gespeichert; die Anzahl der Treffer steht für Screenreader im Status. Die Logik steckt in `cockpit_core::InboxFilter`
(`crates/cockpit-core/src/search.rs`); die Auswahlwerte liefern `Workspace::item_repos()`, `owners()` und `labels()`.

- **Repo:** Pull Requests tragen ihr Repo (`owner/name`, `Item::repo`); Tickets haben keines und fallen bei einer Repo-Wahl heraus.
- **Status:** Tickets zählen mit ihrem Status, Pull Requests mit dem, den sie nahelegen (gemergt → Fertig, Review angefragt → Review, sonst In Arbeit).
- **Label:** `Item::labels` kommt vom Dienst (Jira `fields.labels`, GitHub-Labels des Pull Requests) und wird bei jedem Abgleich ersetzt; Groß-/Kleinschreibung zählt beim Filtern nicht.
- Gleiche Pull-Request-Nummern aus zwei Repos sind in der Inbox getrennte Einträge (die ID enthält das Repo). Die Reviews und die Zuordnung `PrLink` hängen weiter an der Nummer – zwei Repos mit derselben Nummer teilen sich dort einen Platz.

**2. Token setzen** (von der Standardeingabe in die `.env`-Datei, nie in Datei, Datenbank, Log oder Browser):

```bash
echo "$GITHUB_TOKEN" | cargo run -p cockpit-server -- token setzen github
echo "$JIRA_TOKEN"   | cargo run -p cockpit-server -- token setzen jira
```

Das Backend liest den Token bei jedem Gebrauch neu; ein neu gesetzter Token wirkt ohne Neustart.

**3. Welche Rechte die Tokens brauchen**

| Dienst | Token | Rechte |
|--------|-------|--------|
| GitHub | *fine-grained personal access token*, auf die konfigurierten Repos beschränkt | Repository-Rechte **Contents** (Read and write: Branch anlegen), **Pull requests** (Read and write: lesen, Review senden, mergen), **Checks** (Read and write: Check-Status lesen, Check neu anfordern), **Issues** (Read and write: Kommentare), **Metadata** (Read, automatisch) |
| Jira | *API-Token* des Atlassian-Kontos (id.atlassian.com › Sicherheit › API-Token) | Das Konto braucht in den Projekten „Projekte durchsuchen“, „Kommentare hinzufügen“ und „Vorgänge bearbeiten/Transitionen ausführen“. Anmeldung mit der E-Mail aus der Konfiguration |

**Anmeldung per OAuth (Alternative zum Token).** Statt eines Tokens meldest du dich im Browser an; Tokens und
Client-Secret liegen nur im `.env`-Datei des Backends, nie im Browser, Repo, in der Datenbank oder im Log.

1. *Apps registrieren.* GitHub: eine **GitHub App** (Callback `http://127.0.0.1:8787/api/oauth/github/callback`,
   „Expire user authorization tokens“ an, Repository-Rechte wie in der Tabelle unten, App auf den Repos installieren).
   Atlassian: eine **OAuth-2.0-(3LO)-App** (Callback `http://localhost:8787/api/oauth/atlassian/callback`, Jira-Rechte
   `read:jira-work`, `write:jira-work`, `read:jira-user`). Atlassian-Apps sind *resource-level*: Der Zugriff gilt nur für
   die Site, die du bei der Zustimmung wählst.
2. *Client-ID in die `config.toml`* (ist nicht geheim; mit `[atlassian]` braucht `[jira]` weder `site` noch `email`):

   ```toml
   [github]
   repos = ["firma/app"]            # die Repos braucht der Abgleich weiterhin

   [github_oauth]
   client_id = "Iv23…"

   [atlassian]
   client_id = "7dNimh…"
   # site = "firma"                 # nur nötig, wenn du mehrere Sites freigibst (Name, Adresse oder Id)
   ```

3. *Client-Secret in die `.env`* (Standardeingabe, wie bei den Token):

   ```bash
   echo "$GITHUB_CLIENT_SECRET"    | cargo run -p cockpit-server -- token setzen github-oauth
   echo "$ATLASSIAN_CLIENT_SECRET" | cargo run -p cockpit-server -- token setzen atlassian-oauth
   ```

4. *Anmelden.* In der App unter „Einrichten“ erscheint beim Jira- und beim GitHub-Schritt „Mit Atlassian
   anmelden“ bzw. „Mit GitHub anmelden“. Der Knopf öffnet `GET /api/oauth/{atlassian|github}/start` und leitet zum
   Dienst weiter (mit `state`, einmalig und 10 Minuten gültig, und PKCE). Nach der Zustimmung ruft der Dienst
   `GET /api/oauth/{dienst}/callback` auf; das Backend prüft `state`, tauscht den `code` gegen die Tokens und leitet nach
   `/` um, wo ein Hinweis Erfolg oder den Fehler nennt. Der Callback ist eine Navigation des Browsers: Loopback- und
   Host-Prüfung gelten weiter, einen JSON-Content-Type verlangt er nicht.
   - *Atlassian:* danach fragt das Backend `accessible-resources` ab. Genau eine Site wird automatisch gewählt, bei
     mehreren muss `site` in der Konfiguration stehen (sonst nennt die Fehlermeldung die Möglichkeiten). Jira läuft dann über
     `https://api.atlassian.com/ex/jira/<cloudId>`; die gewählte Site (Id und Name, nicht geheim) steht in der Datenbank.
   - *GitHub:* das Nutzer-Token läuft ab (GitHub App); das Backend frischt es vor dem Ablauf mit dem Refresh-Token auf.
     Atlassian dreht das Refresh-Token bei jeder Auffrischung; auch das speichert das Backend.
   - Ist ein OAuth-Zugang da, geht er vor einem API-Token (`token setzen github|jira`); die Token-Wege bleiben
     unverändert. Nach dem Trennen gilt wieder das Token.
5. *Trennen.* „Trennen“ in der App oder `POST /api/oauth/{dienst}/widerrufen` löscht die Tokens in der `.env`. Bei
   GitHub widerruft das Backend das Token zusätzlich beim Dienst (`DELETE /applications/{client_id}/token`). Atlassian
   bietet dafür keinen Aufruf; den Zugriff entziehst du unter id.atlassian.com/manage-profile/apps (die App weist
   darauf hin). Gelöscht wird immer, auch wenn der Dienst nicht antwortet. Das Client-Secret bleibt.
6. *Wenn es nicht mehr geht.* Ein abgelaufener oder widerrufener Zugriff zeigt „nicht angemeldet“ (`Problem::Unauthorized`)
   in der Quelle. Lehnt der Dienst das Auffrischen ab, löscht das Backend die Tokens: Der Dienst gilt als getrennt und
   du meldest dich neu an. Netzwerkfehler oder Störungen des Dienstes lassen die Tokens stehen.

`GET /api/health` meldet unter `anmeldung.github` und `anmeldung.jira` je `verbunden`, `art` (`oauth`, `token` oder
`none`), `oauth_moeglich` (App eingerichtet und Secret da) und bei Atlassian die `site`. `cockpit-core::Connections`
trägt dieselbe Art (`github_auth`, `jira_auth`); sie wird nicht im Browser gespeichert.

**Nicht gegen die echten Dienste geprüft.** Austausch, Auffrischen, Widerruf und die Site-Wahl sind nur gegen
nachgebaute Antworten getestet (`cargo test`). Offen bis zum ersten echten Durchlauf: ob Atlassian `code_verifier`
bei einer Web-App mit Client-Secret akzeptiert (sonst entfällt er dort), ob die GitHub App PKCE annimmt, und die
Installations-Weiterleitung von GitHub (`installation_id`, `setup_action`): Ohne `state` aus unserem Start lehnt das
Backend sie ab; die App ist schon installiert, die Anmeldung beginnt immer in Cockpit.

**Einrichten in der App.** Die Seite „Einrichten“ (`#/einrichten`) ist ein geführter Assistent in fünf Schritten mit
Fortschrittsanzeige (Container, Jira, GitHub, Modell, Fertig). Sie erscheint beim ersten Start von selbst, wenn noch nichts
eingerichtet ist (einmal je Browser; der Browser merkt sich nur diesen Hinweis), und ist jederzeit über „Einrichten“ in der
Seitenleiste erreichbar. Jeder Schritt lässt sich überspringen; die Zusammenfassung nennt in Klartext, was läuft (Haken),
was offen ist und was zu prüfen ist, mit „Ansehen“ zum passenden Schritt. Die Texte setzen kein Vorwissen voraus.

- *Container.* Erkennt Docker oder Podman; fehlt beides, erklärt der Schritt, was ein Container ist, und verweist auf
  Rancher Desktop (Engine **„dockerd (moby)“**) bzw. Podman. „Erneut prüfen“ fragt neu.
- *Jira.* Standard ist der **API-Token** in drei Schritten (Site, E-Mail, Token erstellen und einfügen), dann
  Verbindungstest und Projektwahl (siehe „Jira verbinden“). Die **Atlassian-App** (OAuth) bleibt als Alternative unter
  „Erweitert: Mit OAuth statt Token“ eingeklappt, mit Anleitung durch die Entwicklerkonsole (App anlegen,
  Resource-level, Jira API hinzufügen, Classic scopes `read:jira-work`, `write:jira-work`, `read:jira-user`, Callback-URL mit
  „Kopieren“-Knopf, Client-ID und Secret eintragen, dann „Mit Atlassian anmelden“).
- *GitHub.* Anleitung für eine **GitHub App** mit Abhak-Liste der Berechtigungen (Contents, Pull requests, Issues, Checks
  Read & write; Commit statuses und Metadata Read-only), Callback-URL zum Kopieren, Client-ID und Secret, Hinweis „Install
  App“ und die Repos, dann „Mit GitHub anmelden“.
- *Modellzugang.* Adresse, Modellname, Key und der Verbindungstest (siehe unten).

**Wie Eingaben ankommen.** Nicht geheime Werte (Client-IDs, Jira-Site, E-Mail, Projekte, Repos, Endpunkt-Adresse und
-Modell) schickt die Seite an `PUT /api/konfiguration`. Der Server ändert die `config.toml` **atomar** (Nachbardatei,
dann umbenennen; bestehende Kommentare, Reihenfolge und andere Werte bleiben, neue Dateien sind nur für dich lesbar) und
prüft das Ergebnis **wie beim Start** (`Config::aus`): Ist es ungültig, bleibt die Datei, wie sie war, und die Antwort
nennt den Grund in Klartext. Eingaben werden vereinheitlicht (`firma` → `https://firma.atlassian.net`, GitHub-Adressen →
`owner/name`, Projekte in Großbuchstaben, `localhost:4000` → `http://localhost:4000`). Alles gilt **ohne Neustart**
(Quellen, OAuth-Apps und der Agent-Endpunkt werden im laufenden Server ausgetauscht); nur der Takt des Abgleichs und der Port
bleiben bis zum Neustart. Ändert sich die Client-ID einer App, rät die Antwort, sich neu anzumelden.

**Wie Schlüssel ankommen.** Client-Secrets, Tokens und der Key des Endpunkts kommen in Passwortfelder
(`autocomplete="off"`) und gehen einmal an `PUT /api/schluessel/{name}` auf der Loopback-Verbindung. Der Server legt sie im
`.env`-Datei ab. Der Wert steht nur im Feld und in dieser einen Anfrage: nicht in `localStorage`, nicht in der Adresse,
nicht im Log, nicht in Fehlermeldungen und nie in einer Antwort (die sagt nur `gesetzt`); das Feld wird sofort nach dem
Absenden geleert. Erlaubt sind nur die Namen `github-oauth`, `atlassian-oauth`, `agent-key`, `litellm` (älterer Name), `jira` und `github`. Die beiden
neuen `PUT`-Routen unterliegen derselben Loopback-, Host-, Origin- und Content-Type-Prüfung wie alle Routen unter `/api`.

Die Abschnitte „Modellzugang“ und „Container“ im Einzelnen:

- *Agent-Anbieter und Endpunkt.* `[agent] anbieter` wählt das Werkzeug im Container: `claude-agent-sdk` läuft; `claude-code`
  und `codex` sind bekannte Werte, die in Klartext „noch nicht umgesetzt“ melden (der Assistent zeigt sie als „bald“).
  `[agent.endpunkt]` sagt, wohin die Modellanfragen gehen: mit `url` an einen Anthropic-kompatiblen Endpunkt (LiteLLM,
  anderer Proxy; im Container `ANTHROPIC_BASE_URL` + `ANTHROPIC_AUTH_TOKEN`), ohne `url` direkt zu Anthropic (im Container
  `ANTHROPIC_API_KEY`). `modell` ist der Name, wie der Endpunkt ihn anbietet. Der Key liegt nur in der `.env`
  (`agent-key`): `echo "$KEY" | cargo run -p cockpit-server -- token setzen agent-key`; die Werte gehen nur per `-e NAME` an
  den Container. `/api/health` meldet unter `endpunkt` Anbieter, Adresse, Modell und ob ein Key liegt (nie den Key).
  „Modelle laden“ ruft `GET /api/agent/modelle`; das Backend fragt dann `GET <url>/v1/models` (Proxy: `Authorization: Bearer`;
  direkt: `https://api.anthropic.com/v1/models` mit `x-api-key` und `anthropic-version`), ohne Weiterleitungen, mit
  Zeitlimit und Größengrenze, und liefert nur die IDs. „Verbindung testen“ ruft `GET /api/agent/pruefen` und meldet
  *erreichbar*, *Key angenommen* und *Modell vorhanden*, bei Fehlern mit Hinweis. Ohne Key ruft es den Endpunkt nicht auf.
- *Container-Laufzeit.* Der Agent läuft später in einem Container, mit Docker (Rancher Desktop, Engine „dockerd (moby)“)
  oder Podman. Das Backend wählt: `COCKPIT_CONTAINER`, sonst `[agent] container`, sonst sucht es erst `podman`, dann
  `docker` im `PATH` und prüft mit `<laufzeit> info`, ob die Engine läuft (nur als Prozessaufruf, nie über den Socket; das Ergebnis
  wird 10 Sekunden gemerkt). `/api/health` meldet unter `container` `laufzeit`, `engine_laeuft`, `quelle`
  (`umgebung`, `konfiguration`, `erkannt`, `keine`) und einen `hinweis`. Das Agent-Image baust du im Schritt
  „Container“ mit „Image bauen“ (siehe „Den Agenten einrichten“).

**Was abgeglichen wird.** Das Backend fragt im Takt (und bei `POST /api/sync`, in der App unter „Einrichten“ ›
Übersicht › Abgleich › „Jetzt abgleichen“) beide Dienste ab und führt das Ergebnis über die Aktion `SyncRemote`
(Urheber „System“) mit dem Workspace zusammen – als reine Funktion in `cockpit_core::sync::merge`:

- Neue Einträge kommen dazu, entfernte verschwinden. Lokaler Zustand bleibt: gelesen/erledigt, Zurückstellungen,
  Agent-Läufe und -Hinweise, Vorschläge und Entscheidungen im Review, eigene noch nicht gesendete Zeilenkommentare
  (sie wandern mit ihrer Zeile, wenn sich der Diff verschiebt), Urteil und „gesendet“.
- Neue Kommentare, Status-, Check- und Review-Änderungen von außen stehen mit Urheber „System“ im Protokoll und
  lösen die Benachrichtigungen aus. `@dein-Login` in Kommentaren zählt als Erwähnung.
- Pro Quelle gibt es einen Zustand (`/api/health` › `abgleich`, in der App unter „Abgleich“): `ok`,
  `nicht-angemeldet` (401, Token abgelaufen), `keine-berechtigung` (403), `kontingent` (Rate-Limit: die Quelle
  pausiert bis zum Zeitpunkt aus `Retry-After`/`X-RateLimit-Reset`), `offline`, `fehler`. Bei einem Fehler bleiben
  die letzten Daten der Quelle stehen.
- Die Nummer eines Pull Requests ist im Modell der Schlüssel eines Reviews. Haben zwei Repos dieselbe Nummer,
  steht nur der erste im Cockpit (Hinweis beim Zustand der Quelle).

**Was nach außen geht.** Im Server-Modus gehen diese Aktionen erst an den Dienst; **erst wenn er bestätigt, wird die
Aktion lokal angewendet**. Schlägt der Aufruf fehl, kommt eine deutsche Meldung (Status 502) und nichts ändert sich.
Die Regeln des Workspace (auch „nur Du darfst mergen“, „Review erst nach Entscheidung über jeden Vorschlag“)
gelten vor jedem Aufruf.

| Aktion | Aufruf |
|--------|--------|
| `Reply` | Jira: Kommentar (`POST /rest/api/3/issue/{key}/comment`, ADF) · GitHub: `POST /repos/{repo}/issues/{n}/comments` |
| `SetStatus` | Jira: passende Transition zum Ziel-Status suchen (`GET …/transitions`), dann `POST …/transitions` |
| `CreateBranch` | GitHub: Ref vom Standard-Branch des **ersten** konfigurierten Repos (`POST /repos/{repo}/git/refs`) |
| `SendReview` | `POST /repos/{repo}/pulls/{n}/reviews` mit Urteil, Text, freigegebenen Vorschlägen und eigenen Zeilenkommentaren als Positionskommentare (`suggestion`-Blöcke inklusive), gebunden an den gesehenen Commit |
| `MergePr` | `PUT /repos/{repo}/pulls/{n}/merge` mit Methode und gesehenem Commit (neuer Push → GitHub lehnt ab) |
| `RestartCheck` | `POST /repos/{repo}/check-runs/{id}/rerequest` |

Alle anderen Aktionen – und alle Einträge, die aus den Beispieldaten stammen – bleiben lokal. Die Aktion
`SyncRemote` darf nur das Backend auslösen; `POST /api/actions` lehnt sie ab.

## Lernpfad

Die Konzepte bauen aufeinander auf. Jeder Schritt nennt die Stelle im Code, an der du es
siehst, und die Story im Playground, in der es sichtbar wird.

| # | Konzept | Im Code | Im Playground |
|---|---------|---------|---------------|
| 1 | **Komponenten und Props**: eine Funktion mit `#[component]`, Parameter sind Props | `crates/ui/src/text.rs` (`Text`) | Foundations |
| 2 | **Enums statt Strings**: `enum`, `impl`, `#[derive(Default)]`, `#[default]`, `const ALL` | `crates/ui/src/button.rs` (`ButtonStyle`) | Button |
| 3 | **`Option` und `then_some`**: ein Attribut nur manchmal setzen | `button.rs`, Funktion `attr()` und `data-role` | Button |
| 4 | **Children**: Inhalt an eine Komponente übergeben | `crates/ui/src/layout.rs` (`VStack`) | Stacks & Grids |
| 5 | **Signals**: reaktiver Zustand, `RwSignal`, `.get()` / `.set()` | `crates/ui/src/controls.rs` (`Toggle`) | Toggle · Picker · … |
| 6 | **Closures mit `move`**: warum Signals in `move \|\| …` stehen | überall, z. B. `data-role=role` in `button.rs` | Button |
| 7 | **Abgeleitete Signals**: `Signal::derive` | `apps/playground/src/stories/button.rs` | Button (Live-Controls) |
| 8 | **Callbacks**: Ereignisse nach oben melden | `button.rs` (`on_click`) | Button |
| 9 | **Listen rendern**: Iteratoren und `collect_view()` | `controls.rs` (`Segmented`) | Toggle · Picker · … |
| 10 | **Bedingt rendern**: `<Show>` und `match` mit `.into_any()` | `sheet.rs`, `list.rs` (`ListRow`) | iPhone-Beispiel |
| 11 | **`'static` und Konstanten**: `&'static [(&str, &str)]`, `const`, `static` | `controls.rs`, `apps/playground/src/stories/mod.rs` | alle |
| 12 | **Funktionszeiger**: `render: fn() -> AnyView` in einer Registry | `apps/playground/src/stories/mod.rs` (`Story`) | die Navigation selbst |
| 13 | **`Option` und `Result` verketten**: `.ok().flatten().is_some_and(…)` | `apps/playground/src/main.rs` (`system_theme`) | Hell/Dunkel-Schalter |
| 14 | **Ereignisse aus dem Browser**: `window_event_listener`, Aufräumen mit `on_cleanup` | `apps/playground/src/main.rs` | Navigation per URL (`#button`) |
| 15 | **Globaler Zähler**: `AtomicU32` für eindeutige IDs | `crates/ui/src/field.rs` | TextField & Search |
| 16 | **Tests**: `#[cfg(test)]`, `include_str!` liest das CSS zur Compile-Zeit | `crates/ui/src/lib.rs` | `cargo test` |
| 17 | **Logik ohne UI**: `struct`, `impl`, `Result`, Zustandsmaschine, viele Tests | `crates/cockpit-core/src/agent.rs`, `workspace.rs` | Cockpit |
| 18 | **Gemeinsamer Zustand**: ein `Copy`-Store im Kontext, `Memo`, `Effect` | `apps/cockpit/src/store.rs`, `apps/cockpit/src/pages/inbox.rs` | Cockpit |
| 19 | **Workspace und Crates**: gemeinsame Versionen, ein Crate pro Baustein | `Cargo.toml`, `crates/ui/Cargo.toml` | – |

Gute Übung für jeden Schritt: Ändere etwas absichtlich falsch (z. B. `ButtonStyle::Glas`
statt `Glass`) und lies die Compiler-Meldung. Rust erklärt Fehler meist sehr genau.

## Projektstruktur

```
.
├── Cargo.toml                  # Workspace: gemeinsame Versionen & Mitglieder
├── apps/                       # alles, was man startet
│   ├── cockpit/                # die Hauptanwendung: Oberfläche (WASM), eigenes index.html und Trunk.toml
│   │   └── src/
│   │       ├── main.rs         # Start, Routing, Takt der Agenten
│   │       ├── route.rs        # Seiten und ihre Adressen (#/review/87)
│   │       ├── store.rs        # gemeinsamer Zustand
│   │       └── pages/          # Einrichten, Inbox, Review, Agent
│   ├── server/                 # Backend des Cockpits: axum, SQLite, Schlüsselablage
│   ├── agent/                  # Container-Image und Läufer des Agenten (Node/TypeScript)
│   └── playground/             # die Spielwiese: zeigt jeden Baustein einzeln (eigenes Trunk.toml)
│       ├── index.html          # bindet WASM und CSS ein
│       ├── style.css           # Layout der Hülle (nur Tokens, keine festen Farben)
│       └── src/
│           ├── main.rs         # Navigation, Theme-Schalter, aktive Story
│           └── stories/        # eine Datei pro Story + Registry in mod.rs
├── crates/                     # Bibliotheken
│   ├── cockpit-core/           # Fachlogik des Cockpits – ohne UI, mit Tests
│   ├── integrations/           # GitHub- und Jira-Clients (lesen, schreiben, abgleichen), nativ
│   └── ui/                     # Komponentenbibliothek (Crate `feature-ui`), von Cockpit und Playground genutzt
│       ├── tokens.css          # Design-Tokens, für das ganze Projekt
│       ├── components.css      # Komponenten-CSS, 1:1 aus dem Design
│       ├── extras.css          # Ergänzungen und Korrekturen zum Design
│       └── src/                # Leptos-Komponenten, eine Datei pro Gruppe
├── scripts/                    # start.sh (App bauen, Server starten), launchd-Vorlage
└── docs/                       # Übergabe und Fahrplan
```

`apps/` enthält die Programme, `crates/` die Bibliotheken, die sie teilen. Das Cockpit ist die
Hauptanwendung; Playground und `crates/ui` sind die Spielwiese für die Komponenten.

### Baustein und Story

| Begriff      | Wo?                         | Kennt den Playground? |
|--------------|-----------------------------|-----------------------|
| **Baustein** | `crates/<name>/` (Crate)   | nein, daher wiederverwendbar |
| **Story**    | `apps/playground/src/stories/<name>.rs` | ja, zeigt den Baustein mit Beispielwerten |

Ein Baustein-Crate hängt nur von `leptos` ab. In einem anderen Projekt bindest du ihn
per Pfad ein:

```toml
[dependencies]
feature-ui = { path = "../rust-playground/crates/ui" }
```

Dazu `crates/ui/tokens.css`, `crates/ui/components.css` und `crates/ui/extras.css`
einbinden, in dieser Reihenfolge.

## Design-Tokens

`crates/ui/tokens.css` hat drei Ebenen:

| Ebene      | Präfix    | Inhalt                          | Beispiel |
|------------|-----------|---------------------------------|----------|
| Primitive  | `--ref-*` | Rohwerte                        | `--ref-space-16` |
| Semantisch | `--sys-*` | Rollen, abhängig vom Theme      | `--sys-color-label` |
| Komponente | `--comp-*`| pro Baustein                    | `--comp-button-height-large` |

Eigenes CSS benutzt nur `--sys-*` (bei Bedarf `--ref-*`). Dann funktionieren Hell/Dunkel,
Akzentfarbe und „Kontrast erhöhen" automatisch. Alle Werte siehst du in der Story
**Foundations**, und links unten im Playground stellst du Theme, Akzentfarbe und Kontrast um.

> **Design aktualisieren:** `tokens.css` und `components.css` sind unverändert aus dem
> Design übernommen. Bei Änderungen ersetzt du einfach die Dateien. Eigene Ergänzungen
> gehören in `extras.css`.

## Bausteine (`feature-ui`)

Die API spiegelt die SwiftUI-Namen des Designs:

```rust
view! {
    <Button variant=ButtonStyle::GlassProminent size=ControlSize::Large>"Weiter"</Button>
    <Toggle checked=wifi />
    <TextField label="E-Mail" value=mail input_type="email" error=invalid />
}
```

| Gruppe     | Komponenten |
|------------|-------------|
| Grundlagen | `Root`, `Text`, `Icon`, `Glass` |
| Layout     | `VStack`, `HStack`, `ZStack`, `Spacer`, `Grid`, `Split`, `Container`, `PageSection` |
| Controls   | `Button`, `Link`, `Badge`, `Toggle`, `Segmented`, `Slider`, `Stepper` |
| Eingabe    | `TextField`, `SearchField` |
| Inhalt     | `List`, `ListSection`, `ListRow`, `ListButtonRow`, `Card`, `CardBody`, `Media` |
| Navigation | `Navbar`, `IosNav`, `TabBar`, `Pager`, `Footer` |
| Overlay    | `Sheet` |
| App        | `SplitView`, `Sidebar`, `Pane`, `Status`, `Avatar`, `Chip`, `InboxRow`, `CommentBubble`, `Textarea`, `Diff`, `SuggestionCard`, `Timeline`, `Progress`, `Log`, `Steps` |

## Cockpit

Ein Dashboard, das Jira, GitHub und Agenten an einem Ort zusammenführt. Diese Seiten gibt es:

| Seite | Adresse | Was sie kann |
|-------|---------|--------------|
| Einrichten | `#/einrichten` | Je Dienst Stand und nächster Schritt: Jira und GitHub anmelden/trennen, Agent-Endpunkt prüfen, Container-Laufzeit erkennen |
| Aufmerksamkeit | `#/`, `#/meine`, `#/team`, `#/zurueckgestellt` | Inbox aus Tickets und Pull Requests, gefiltert nach einschaltbaren Regeln (Erwähnung, Review-Anfrage, blockierter PR, überfälliges Ticket, Agent fertig, zugewiesen); Zurückstellen (1 Stunde, morgen früh, nächste Woche, bis sich etwas ändert); Status ändern, Branch erstellen, antworten, Agent beauftragen; Ticket bearbeiten (Titel, Beschreibung, Zuweisung, Priorität); Ticket und Pull Request verknüpfen (automatisch über den Schlüssel im Branch oder Titel, `BAU-142`, sonst von Hand oder lösen); „Status folgt dem Pull Request“ (Entwurf/offen → In Arbeit, Review angefragt → Review, gemergt → Fertig, abschaltbar in der Seitenleiste, Urheber im Protokoll: System); Suche und Quellen-Filter; Tasten `↑↓`/`j k`, `e`, `r`, `s`, `u`, `/` |
| Review | `#/review/87` | Diff lesen; Zeilen auswählen und eigene Kommentare schreiben; Kommentare des Agenten freigeben, ablehnen oder bearbeiten; Review senden |
| Übersicht | `#/uebersicht` | Pro Person offene Einträge und Auslastung; „Hängt“ zeigt fehlgeschlagene Checks, wartende Reviews, Tickets ohne Branch und Agenten, die auf dich warten |
| Benachrichtigungen | `#/benachrichtigungen` | Je Anlass (Review-Anfrage, Erwähnung, Agent wartet, Agent fertig, Agent angehalten, Checks rot, Ticket zugewiesen) Browser, E-Mail und Slack wählen; Ruhezeiten; Erlaubnis des Browsers per Knopf anfragen; Test-Benachrichtigung |
| Agent | `#/agent/BAU-142` | Auftrag mit Optionen; Plan zur Freigabe (abschaltbar); Ablauf in fünf Schritten bis zum Pull Request; Rückfragen beantworten, Hinweise schicken, pausieren; Verlauf als Gespräch |
| Grenzen & Regeln | `#/grenzen` | Preise pro Million Token, Budget pro Lauf und pro Monat, Zeitlimit pro Lauf, höchstens so viele Läufe gleichzeitig; pro Repo erlaubte/verbotene Pfade, Pflicht-Tests, Anweisungen an den Agenten und Plan-Pflicht; Verbrauch heute und diesen Monat |

**Agent: Rückfragen und Nachsteuern.** Der Agent kann an einem Schritt anhalten und eine Frage stellen
(Status „Wartet auf Antwort“, mit vorgeschlagenen Antworten zum Anklicken oder eigenem Text). Die Frage
zählt als Aufmerksamkeit: Sie steht in der Übersicht unter „Hängt“ (Knopf „Frage beantworten“) und in der
Sidebar bei „Agenten“. Bei den Beispieldaten fragt der Lauf für **BAU-142** nach dem zweiten Schritt, ob der alte Stil
entfernt oder als deprecated markiert werden soll. Läuft der Agent, schickst du ihm unter „Nachsteuern“ einen
Hinweis; er bestätigt ihn beim nächsten Schritt („Hinweis erhalten: …“). „Pausieren“ hält den Lauf an,
„Fortsetzen“ geht weiter. Alles Gesagte steht im Verlauf neben der Timeline. Die Aktionen heißen
`AnswerAgent`, `SteerAgent`, `PauseAgent` und `ResumeAgent`.

**Agent: Folgeaufträge auf einem Pull Request.** Auf der Review-Seite (rechts) gibt es zwei Aufträge:

- **Review-Kommentare umsetzen.** Unter „Review-Kommentare umsetzen“ stehen die offenen Zeilenkommentare (eigene und die von Reviewern, auch mit Änderungsvorschlag), standardmäßig alle ausgewählt. „Agent: n Kommentare umsetzen“ startet den Lauf `review-<Nr.>` (`RunKind::AddressReview`) mit eigenen Schritten (Kommentare lesen, Branch auschecken, Änderungen, Tests, Push). Er pusht auf **denselben Branch**, es entsteht **kein neuer Pull Request**. Plan-Freigabe, Rückfragen, Nachsteuern und Pausieren gelten wie beim Ticket-Lauf. Am Ende sind die Kommentare „Erledigt“ (mit Verweis auf den Lauf), Änderungsvorschläge stehen im Diff, der Eintrag meldet den neuen Commit und steht wieder auf „Review angefragt“. Fremde Kommentare gehen nie mit deinem Review hinaus und lassen sich nicht bearbeiten.
- **Roten Check untersuchen.** Unter jedem roten Check mit Log: „Agent untersuchen lassen“ (Lauf `check-<Nr.>-<Check>`, `RunKind::Diagnose`). Der Agent liest das Log und legt eine **Diagnose** vor: Ursache in einem Satz, Datei und Zeile, Vorschlag als Diff. Bis du entscheidest, bleibt der Branch unberührt. „Fix übernehmen“ pusht den Vorschlag auf den Branch und startet den Check neu, „Verwerfen“ beendet den Lauf. Die Demo erkennt ungenutzte Variablen/Imports (Lint in #85, Fix: Zeile entfernen) und Zeitüberschreitungen (wacklig, nur Neustart).

Die Aktionen heißen `AddressReview`, `InvestigateCheck`, `ApplyFix` und `DiscardFix`. Eine wartende Diagnose zählt als Aufmerksamkeit (Übersicht „Hängt“: „Diagnose wartet“, Sidebar „wartet“) und löst „Agent wartet“ bzw. „Agent fertig“ aus. Der Agent merged auch hier nie.
**Agent: Kosten und Grenzen.** Jeder Schritt hat einen Verbrauch (Eingabe-/Ausgabe-Token, Kosten in ganzen Euro-Cent, Laufzeit); er steht an der Timeline, die Summe des Laufs sowie „heute“ und „diesen Monat“ darunter. In der Demo sind die Werte pro Ticket und Schritt fest (`demo::step_tokens`), die Preise sind eine Einstellung; der echte Agent trägt später gemeldete Werte über `AgentRun::record_usage` ein. Erreicht ein Lauf eine Grenze (Budget pro Lauf, Monatsbudget, Zeitlimit), hält er an (`RunState::Stopped(StopReason)`): Die Agent-Seite bietet „Um X erhöhen und fortsetzen“ (`ResumeStopped`) oder „Abbrechen“, der Lauf steht in der Übersicht unter „Hängt“, und es gibt eine Benachrichtigung „Agent angehalten“. Ein neuer Lauf über der Grenze paralleler Läufe **startet nicht** (`ActionError::ParallelLimit`, deutsche Meldung) – es gibt bewusst keine Warteschlange. Tag und Monat zählen nach UTC; abgebrochene Läufe bleiben im Kassenbuch.

**Agent: Regeln pro Repo.** Pro Repo (Schlüssel: Name, heute nur das Demo-Repo) gelten erlaubte und verbotene Pfade (Glob-Muster, `cockpit_core::rules::glob_match`), Pflicht-Tests, Anweisungen und Plan-Pflicht. Plan und Schritte zeigen die geltenden Regeln. Ein Plan, der verbotene Pfade berührt, lässt sich nicht freigeben (`ActionError::PlanViolatesRules`, mit Begründung); so ein Lauf wartet immer erst mit einem Plan. Pflicht-Tests lassen sich nicht abschalten. Geänderte Regeln gelten für neue Läufe und wartende Pläne. Die Aktionen heißen `SetAgentLimits` und `SetRepoRules`.

Zwei Regeln sind in der Logik verankert und getestet: Ein Review wird erst gesendet, wenn über
jeden Vorschlag entschieden ist (eigene Zeilenkommentare brauchen keine Freigabe). Der Agent legt vor dem Start einen Plan vor und tut nichts, bis du ihn freigibst (ablehnen oder mit Anmerkung neu anfordern ist möglich). Und der Agent merged nie – sein Ergebnis ist ein Pull Request,
der auf dein Review wartet.

### Benachrichtigungen

Welche Ereignisse melden, leitet `crates/cockpit-core/src/notify.rs` rein aus dem Vergleich des Workspace vor und nach einer Aktion ab (mit Tests). Die Einstellungen liegen im Workspace und ändern sich über Aktionen (`SetNotifyChannel`, `SetQuietHours`). Im **Browser** fragt Cockpit nie von selbst nach der Erlaubnis, nur über den Knopf unter „Benachrichtigungen“. Ist der Tab im Vordergrund, erscheint ein Hinweis in der App, sonst eine Systemmeldung; ein Klick darauf öffnet das Ziel. In den Ruhezeiten bleibt es still. **E-Mail** und **Slack** sind nur vorbereitet: Die Auswahl wird gespeichert, versendet wird später über das Backend. Adressen und Webhook-URLs liegen nie im Browser.

### Stand: echter Agent im Container, simulierter nur für den Seed

Die Oberfläche und die Abläufe sind vollständig. Der Agent im Server ist ein **echter Lauf im Container** (siehe „Den
Agenten einrichten“). Nur mit dem Seed `COCKPIT_SEED=demo` (und in Tests) läuft weiterhin der simulierte Agent: ein
Timer, der die fünf Schritte durchläuft. Beispieldaten
(`crates/cockpit-core/src/demo.rs`) gibt es nur als Seed (`COCKPIT_SEED=demo`) sowie als Testdaten und für die Stories im
Playground. Im Einzelnen:

- Einrichten zeigt den Stand der Anbindungen (siehe „Einrichten in der App“); Schlüssel gibst du dort nie ein.
- Alles, was die App zeigt, liegt im Backend (SQLite) als JSON mit Versionsnummer, siehe
  `crates/cockpit-core/src/persist.rs`. Passt die Version nicht oder sind die Daten unlesbar, startet der Server mit dem
  Seed (leer oder Beispieldaten). Neue Felder in den Typen brauchen `#[serde(default)]`, damit ältere Stände lesbar bleiben.
- Review: Beim Zeilenkommentar lässt sich mit „Änderung vorschlagen“ Ersatz-Code für die Zeile angeben (Vorher/Nachher-Diff,
  gesendet als `suggestion`-Block). Rechts stehen die Checks des Pull Requests mit Log; rote lassen sich neu starten
  (bei den Beispieldaten fest hinterlegtes Ergebnis nach ein paar Takten, #85: „Lint“ bleibt rot, „Tests (Browser)“ wird grün).
  „Mergen“ (Merge/Squash/Rebase) geht nur mit grünen Checks, gesendetem Review und ohne „Änderungen“ – und nur für
  dich, nie für den Agenten. Bei Beispieldaten wird nur simuliert, nichts geht an GitHub.
- Die Berechtigungen des Agenten (Branches, Pull Requests, Tests) stehen mit dem echten Agenten im Backend; bis dahin gelten
  die Standardwerte (Branches, Pull Requests und Tests erlaubt; Kommentare und Reviews nur nach deiner Freigabe, der Agent merged nie).

## Eigene Story hinzufügen

1. Datei `apps/playground/src/stories/mein_baustein.rs` anlegen, Vorlage ist `link_badge.rs`:
   ```rust
   use feature_ui::*;
   use leptos::prelude::*;
   use super::{Story, common::Section};

   pub const STORY: Story = Story {
       id: "mein-baustein",       // erscheint in der URL: #mein-baustein
       title: "Mein Baustein",
       group: "Controls",         // Überschrift in der Navigation
       description: "Was zeigt die Story?",
       api: ".ui-… · data-…",     // Kurzreferenz
       render,
   };

   fn render() -> AnyView {
       view! { <Section title="Beispiel">{/* … */}</Section> }.into_any()
   }
   ```
2. In `apps/playground/src/stories/mod.rs` `mod mein_baustein;` ergänzen und `mein_baustein::STORY`
   in `STORIES` eintragen.

## Eigenen Baustein als neues Crate

1. `cargo new --lib crates/mein-baustein --name feature-mein-baustein`
2. In dessen `Cargo.toml`: `leptos = { workspace = true }`. Durch `members = [..., "crates/*"]`
   gehört es automatisch zum Workspace.
3. Im Root-`Cargo.toml` unter `[workspace.dependencies]` eintragen:
   `feature-mein-baustein = { path = "crates/mein-baustein" }`, dann in `apps/playground/Cargo.toml` verwenden.
4. Eigenes CSS in `apps/playground/index.html` per `<link data-trunk rel="css" …>` einbinden, nach den Tokens.

## Schrift

Das Design nutzt SF Pro (auf Apple-Geräten vorhanden) und fällt sonst auf **Inter** zurück.
`apps/playground/index.html` lädt Inter von Google Fonts. Ohne externe Verbindung entfernst du die drei
`<link>`-Zeilen, dann greift der System-Font.
