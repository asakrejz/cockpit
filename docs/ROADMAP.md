# Cockpit: Architektur und Fahrplan

Dieses Dokument ist die gemeinsame Grundlage für alle, die am Cockpit arbeiten – Menschen
wie Agenten. Es hält die Entscheidungen fest und teilt die Arbeit in Wellen.

## Entscheidungen

| Frage | Entscheidung |
|-------|--------------|
| Betrieb | **Lokal, für eine Person.** Ein Dienst auf dem eigenen Rechner. Kein Hosting, keine Mandanten. |
| Backend | Rust-Crate `server/` (axum, tokio). Liefert auch die gebaute Web-App aus. |
| Daten | SQLite in einer Datei im Nutzerverzeichnis. |
| Schlüssel | Liegen nur im Backend (Schlüsselbund des Systems, sonst verschlüsselte Datei). **Nie im Browser, nie im Repo, nie im Log.** |
| Agent | Claude Agent SDK in einem Container pro Lauf, **mit Docker (Rancher Desktop) oder Podman**. Der Container bekommt nur das geklonte Repo und ein Token für genau dieses Repo. |
| Aktualität | Webhooks, wenn ein Tunnel eingerichtet ist; sonst fragt das Backend regelmäßig ab. |
| Demo-Modus | **Entfällt** (seit Welle 4). Die App braucht das Backend. Beispieldaten (`demo::workspace()`) bleiben als Testdaten, für die Stories im Playground und als Seed `COCKPIT_SEED=demo` im Server – nur auf ausdrückliche Anforderung und nur, wenn noch kein Stand gespeichert ist. |
| Modellzugang | Über einen **LiteLLM-Proxy**, kein Anthropic-API-Key. Adresse und Modellname stehen in `config.toml` (`[litellm]`), der Key nur im Schlüsselbund (`token setzen litellm`). |

## Container

Der Agent-Container muss mit **Docker** (z. B. aus Rancher Desktop) **und** mit **Podman** laufen.
Regeln für alles, was Container baut oder startet:

1. **Nur über die Kommandozeile.** Das Backend ruft `docker` oder `podman` als Prozess auf, nie den
   Docker-Socket oder eine Docker-Bibliothek. Benutzt werden nur Befehle und Schalter, die beide
   gleich verstehen (`build -f`, `run --rm`, `-e`, `-v`, `--network`, `--add-host`, `--user`,
   `--label`, `ps`, `rm -f`, `image inspect`).
2. **Laufzeit wählbar.** Konfiguration `[agent] container = "docker" | "podman"`, darüber
   `COCKPIT_CONTAINER`. Ohne Angabe sucht das Backend erst `podman`, dann `docker` im `PATH` und
   prüft mit `<cli> info`, ob die Engine läuft. `/api/health` meldet, welche gefunden wurde.
3. **Images voll benennen.** `docker.io/library/node:…` statt `node:…` – Podman löst Kurznamen
   sonst nicht oder nur nach Rückfrage auf. Die Bauanleitung heißt `Containerfile`; beide bauen
   sie mit `-f Containerfile`. Keine BuildKit-Sonderfunktionen (`RUN --mount`, `# syntax=`).
4. **Rechner erreichen.** Den LiteLLM-Proxy auf dem eigenen Rechner erreicht der Container über
   `--add-host cockpit-host:host-gateway` (Docker ab 20.10, Podman 5.x) und die Adresse
   `http://cockpit-host:<port>` – nicht über `host.docker.internal` oder `host.containers.internal`,
   die es nur bei je einer Laufzeit sicher gibt.
5. **Dateirechte.** Das Repo wird eingebunden (`-v <klon>:/work`), ohne SELinux-Zusätze (`:Z`).
   Der Prozess läuft als Nutzer ohne Root (`--user` im Containerfile festgelegt); Podman (rootless)
   und Docker bilden Nutzer verschieden ab, also nach dem Lauf nur lesen, was im Klon liegt.
6. **Geheimnisse nur per `-e NAME`** (Wert aus der Umgebung des Kindprozesses, nicht auf der
   Kommandozeile), damit sie nicht in `ps` oder in Logs stehen.
7. **Rancher Desktop** muss mit der Engine „dockerd (moby)“ laufen; im Modus „containerd“ gibt es
   nur `nerdctl`, das unterstützen wir nicht.
8. **Prüfen mit beiden.** Tests, die echte Container starten, sind `#[ignore]` und laufen per
   `COCKPIT_CONTAINER=docker cargo test -- --ignored` bzw. `=podman`. Der PR nennt, mit welcher
   Laufzeit geprüft wurde.

## Architektur

```
Browser (cockpit/, WASM)                 Backend (server/)
┌──────────────────────────┐   HTTP    ┌────────────────────────────────┐
│ Seiten → store.dispatch  │ ────────▶ │ POST /api/actions              │──▶ GitHub, Jira
│ Workspace (Signal)       │ ◀──────── │ GET  /api/workspace (Snapshot) │──▶ Agent (Container)
│                          │ ◀── SSE ─ │ GET  /api/events (Änderungen)  │◀── Webhooks / Abfrage
└──────────────────────────┘           │ SQLite · Schlüssel             │
                                        └────────────────────────────────┘
          beide benutzen  crates/cockpit-core  (Modell, Regeln, Action)
```

Grundsätze:

1. **`cockpit-core` ist die eine Wahrheit.** Modell und Regeln stehen dort, ohne UI und ohne
   Netzwerk, mit Tests. Browser und Backend benutzen dasselbe Crate.
2. **Jede Änderung ist eine `Action`.** Die Oberfläche ruft `store.dispatch(action)`; das
   Backend wendet sie an, löst zusätzlich die Wirkung bei GitHub oder Jira aus und schreibt sie
   ins Protokoll. Der Browser hält nichts davon.
3. **Zwei Regeln sind unverhandelbar:** Kommentare und Reviews des Agenten gehen erst nach
   Freigabe raus. Der Agent merged nie und schreibt nie in den Hauptbranch.
4. **Jeder Schritt bleibt lauffähig.** Nach jedem PR bauen Playground und Cockpit, und das
   Cockpit läuft mit dem Server – mit leerem Start und mit `COCKPIT_SEED=demo`.

## Regeln für jeden PR

- Ein Feature, ein Branch, ein PR nach `main`. Klein halten, fremde Bereiche nicht anfassen.
- Logik nach `cockpit-core` mit Unit-Tests. Die App verdrahtet nur.
- Texte und Kommentare auf Deutsch. CSS nur mit Tokens. `styles/tokens.css` und
  `features/ui/components.css` stammen 1:1 aus dem Design und bleiben unverändert.
- Vor dem PR grün: `cargo fmt --all --check`; `cargo clippy --workspace --exclude
  cockpit-server --exclude cockpit-integrations --all-targets --target wasm32-unknown-unknown
  -- -D warnings` (die Server-Crates kompilieren nicht für Wasm); `cargo clippy -p cockpit-server
  -p cockpit-integrations --all-targets -- -D warnings` (nativ); `cargo test --workspace`;
  `trunk build` für beide Apps. Das Feature im Browser durchspielen, gegen den Server mit leerem Start und mit `COCKPIT_SEED=demo`.
- Was nur mit echten Zugangsdaten prüfbar ist, wird gegen nachgebaute Antworten getestet
  und im PR klar als „nicht gegen den echten Dienst geprüft“ benannt.

## Fahrplan

Jede Welle baut auf den gemergten PRs der vorigen auf. Innerhalb einer Welle laufen die
Aufgaben parallel; sie sind so geschnitten, dass je Bereich nur eine Aufgabe läuft.

### Welle 0 – erledigt (#6–#10)

| Bereich | Aufgabe | Branch |
|---------|---------|--------|
| Inbox | Suche, Quellen-Filter, Tastaturbedienung | `claude/cockpit-inbox-search-keyboard` |
| Review | Eigene Kommentare an Diff-Zeilen | `claude/cockpit-review-line-comments` |
| Agent | Plan vor dem Start, mit Freigabe | `claude/cockpit-agent-plan-approval` |
| Zustand | Speicherung im Browser | `claude/cockpit-persist-state` |
| Neue Seite | Team-Sicht: wer arbeitet woran, wo hängt etwas | `claude/cockpit-team-view` |

### Welle 1 – erledigt (#11, #12)

| Bereich | Aufgabe |
|---------|---------|
| Kern | **Action-Schicht und Protokoll:** `Action`-Enum, `Workspace::apply`, `store.dispatch`; alle Seiten stellen um. Jede Aktion landet mit Zeit und Urheber im Protokoll (eigene Seite). |
| Backend | **Grundgerüst `server/`:** axum, SQLite, Schlüsselablage, Konfiguration, `GET /api/health`. Clients für GitHub und Jira als Bibliothek, getestet gegen nachgebaute Antworten, mit Abbildung auf die Typen aus `cockpit-core`. |

### Welle 2 – erledigt (#14–#18)

Aufmerksamkeits-Regeln und Zurückstellen, Review (Änderungsvorschläge, Mergen nur durch dich, Checks mit Log), Agent-Rückfragen und Nachsteuern, Benachrichtigungen, Server-Modus.

### Welle 3 – erledigt (#20–#23)

Tickets bearbeiten und mit PRs verknüpfen (Status folgt dem PR), Agent-Folgeaufträge (Review-Kommentare umsetzen, rote Checks untersuchen), Kosten, Grenzen und Regeln pro Repo, GitHub und Jira lesen und schreiben im Server-Modus. Gegen echte Dienste ist noch nichts geprüft – das braucht deine Tokens.

### Welle 4 – Anbindung vollenden

| Bereich | Aufgabe |
|---------|---------|
| Backend | Anmeldung per OAuth für GitHub und Atlassian, mit Widerruf – **erledigt** (nur gegen nachgebaute Antworten geprüft; siehe README „Mit echten Diensten verbinden“) |
| Backend | ✅ Onboarding im Server-Modus: Seite „Einrichten“ mit Stand und nächstem Schritt je Dienst (Jira, GitHub, LiteLLM, Container-Laufzeit), Verbindungstest `GET /api/litellm/pruefen`, Erkennung von Docker/Podman (`/api/health`), Seed per `COCKPIT_SEED`, Start-Skript `scripts/start.sh` und launchd-Vorlage. Der Demo-Modus der App ist entfernt. |
| Backend | Webhooks und Live-Aktualisierung in der App |
| Backend | Echter Agent: Claude Agent SDK im Container (Docker oder Podman, siehe „Container“); Modell über den LiteLLM-Proxy; Plan, Rückfragen, Kosten und Protokoll kommen aus dem Lauf |
| Inbox | ✅ Mehrere Projekte und Repos; Filter nach Projekt, Repo, Person, Status, Label (siehe README „Filter der Inbox“). Offen: `Review` und `PrLink` unterscheiden Pull Requests noch nach Nummer, nicht nach Repo |

### Welle 5 – Abschluss

| Bereich | Aufgabe |
|---------|---------|
| Überall | Fehler- und Leerzustände: abgelaufenes Token, Rate-Limit, offline, Backend nicht erreichbar |
| Überall | Mobile Ansicht für den Daumen: Navigation unten, Liste und Detail als eigene Ebenen |

## Was nur du tun kannst

Diese Schritte brauchen deine Konten. Die Agenten bauen und testen dagegen mit nachgebauten
Antworten; echt geprüft ist es erst, wenn du es mit deinen Zugängen ausprobierst.

- PRs prüfen und mergen – jede Welle startet erst danach.
- Ab Welle 3: ein GitHub-Token und ein Jira-API-Token zum Ausprobieren.
- Ab Welle 4: eine GitHub-OAuth-App und eine Atlassian-OAuth-App registrieren, einen
  Adresse und Key des LiteLLM-Proxys bereitstellen (kein Anthropic-API-Key), Rancher Desktop
  (Engine „dockerd (moby)“) oder Podman installiert haben, optional einen Tunnel für Webhooks.
