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
| Agent | Claude Agent SDK in einem **Podman**-Container pro Lauf. Der Container bekommt nur das geklonte Repo und ein Token für genau dieses Repo. |
| Aktualität | Webhooks, wenn ein Tunnel eingerichtet ist; sonst fragt das Backend regelmäßig ab. |
| Demo-Modus | Bleibt erhalten. Ohne Backend läuft die App mit Beispieldaten im Browser. |

## Architektur

```
Browser (cockpit/, WASM)                 Backend (server/)
┌──────────────────────────┐   HTTP    ┌────────────────────────────────┐
│ Seiten → store.dispatch  │ ────────▶ │ POST /api/actions              │──▶ GitHub, Jira
│ Workspace (Signal)       │ ◀──────── │ GET  /api/workspace (Snapshot) │──▶ Agent (Podman)
│                          │ ◀── SSE ─ │ GET  /api/events (Änderungen)  │◀── Webhooks / Abfrage
└──────────────────────────┘           │ SQLite · Schlüssel             │
                                        └────────────────────────────────┘
          beide benutzen  crates/cockpit-core  (Modell, Regeln, Action)
```

Grundsätze:

1. **`cockpit-core` ist die eine Wahrheit.** Modell und Regeln stehen dort, ohne UI und ohne
   Netzwerk, mit Tests. Browser und Backend benutzen dasselbe Crate.
2. **Jede Änderung ist eine `Action`.** Die Oberfläche ruft `store.dispatch(action)`. Im
   Demo-Modus wendet der Browser sie an, im Server-Modus das Backend – das zusätzlich die
   Wirkung bei GitHub oder Jira auslöst und sie ins Protokoll schreibt.
3. **Zwei Regeln sind unverhandelbar:** Kommentare und Reviews des Agenten gehen erst nach
   Freigabe raus. Der Agent merged nie und schreibt nie in den Hauptbranch.
4. **Jeder Schritt bleibt lauffähig.** Nach jedem PR bauen Playground und Cockpit, und der
   Demo-Modus funktioniert.

## Regeln für jeden PR

- Ein Feature, ein Branch, ein PR nach `main`. Klein halten, fremde Bereiche nicht anfassen.
- Logik nach `cockpit-core` mit Unit-Tests. Die App verdrahtet nur.
- Texte und Kommentare auf Deutsch. CSS nur mit Tokens. `styles/tokens.css` und
  `features/ui/components.css` stammen 1:1 aus dem Design und bleiben unverändert.
- Vor dem PR grün: `cargo fmt --all --check`; `cargo clippy --workspace --exclude
  cockpit-server --exclude cockpit-integrations --all-targets --target wasm32-unknown-unknown
  -- -D warnings` (die Server-Crates kompilieren nicht für Wasm); `cargo clippy -p cockpit-server
  -p cockpit-integrations --all-targets -- -D warnings` (nativ); `cargo test --workspace`;
  `trunk build` für beide Apps. Das Feature im Browser durchspielen.
- Was nur mit echten Zugangsdaten prüfbar ist, wird gegen nachgebaute Antworten getestet
  und im PR klar als „nicht gegen den echten Dienst geprüft“ benannt.

## Fahrplan

Jede Welle baut auf den gemergten PRs der vorigen auf. Innerhalb einer Welle laufen die
Aufgaben parallel; sie sind so geschnitten, dass je Bereich nur eine Aufgabe läuft.

### Welle 0 – läuft

| Bereich | Aufgabe | Branch |
|---------|---------|--------|
| Inbox | Suche, Quellen-Filter, Tastaturbedienung | `claude/cockpit-inbox-search-keyboard` |
| Review | Eigene Kommentare an Diff-Zeilen | `claude/cockpit-review-line-comments` |
| Agent | Plan vor dem Start, mit Freigabe | `claude/cockpit-agent-plan-approval` |
| Zustand | Speicherung im Browser | `claude/cockpit-persist-state` |
| Neue Seite | Team-Sicht: wer arbeitet woran, wo hängt etwas | `claude/cockpit-team-view` |

### Welle 1 – Fundament

| Bereich | Aufgabe |
|---------|---------|
| Kern | **Action-Schicht und Protokoll:** `Action`-Enum, `Workspace::apply`, `store.dispatch`; alle Seiten stellen um. Jede Aktion landet mit Zeit und Urheber im Protokoll (eigene Seite). |
| Backend | **Grundgerüst `server/`:** axum, SQLite, Schlüsselablage, Konfiguration, `GET /api/health`. Clients für GitHub und Jira als Bibliothek, getestet gegen nachgebaute Antworten, mit Abbildung auf die Typen aus `cockpit-core`. |

### Welle 2 – Funktionen in der Demo

| Bereich | Aufgabe |
|---------|---------|
| Inbox | Aufmerksamkeits-Regeln (Erwähnung, Review-Anfrage, blockierter PR, überfälliges Ticket) und Zurückstellen |
| Review | Änderungsvorschläge am Diff; Mergen; fehlgeschlagene Checks mit Log ansehen und neu starten |
| Agent | Rückfragen des Agenten und Nachsteuern während des Laufs |
| Einstellungen | Benachrichtigungen (Browser; E-Mail und Slack über das Backend vorbereitet), einstellbar je Grund |
| Backend | **Server-Modus:** `POST /api/actions`, `GET /api/workspace`, `GET /api/events`; die App schaltet zwischen Demo und Server um |

### Welle 3 – Echte Daten

| Bereich | Aufgabe |
|---------|---------|
| Inbox | Tickets bearbeiten (Titel, Beschreibung, Zuweisung, Priorität); Ticket und PR automatisch verknüpfen, Status folgt dem PR |
| Review / Agent | Folgeauftrag „Review-Kommentare umsetzen“; Agent untersucht rote Checks und schlägt einen Fix vor |
| Agent | Kosten und Grenzen (Verbrauch, Budget, Zeitlimit, parallele Läufe); Regeln pro Repo (Pfade, Pflicht-Tests, Anweisungen) |
| Backend | GitHub und Jira lesen und schreiben über die Aktionen; Abgleich per regelmäßiger Abfrage |

### Welle 4 – Anbindung vollenden

| Bereich | Aufgabe |
|---------|---------|
| Backend | Anmeldung per OAuth für GitHub und Atlassian, mit Widerruf |
| Backend | Webhooks und Live-Aktualisierung in der App |
| Backend | Echter Agent: Claude Agent SDK im Podman-Container; Plan, Rückfragen, Kosten und Protokoll kommen aus dem Lauf |
| Inbox | Mehrere Projekte und Repos; Filter nach Projekt, Repo, Person, Status, Label |

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
  Anthropic-API-Key bereitstellen, Podman installiert haben, optional einen Tunnel für Webhooks.
