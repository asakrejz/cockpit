# Übergabe: Cockpit weiterbauen

Stand: 12.11.2026, `main` nach #24. Diese Notiz ergänzt `docs/ROADMAP.md` um das, was bisher nur
im Gespräch stand. Eine neue Session liest zuerst diese Datei, dann `docs/ROADMAP.md` und `README.md`.

## Wo wir stehen

- Wellen 0–3 sind gemergt (#5–#24), in Welle 4 dazu OAuth (#29), Inbox-Filter (#28) und das
  Onboarding (Seite „Einrichten“, Seed, Start-Skript). Cockpit läuft **nur noch mit dem Server**
  (`scripts/start.sh`, App unter `http://127.0.0.1:8787`); den Demo-Modus im Browser gibt es nicht mehr.
- Gegen echte Dienste ist noch **nichts** geprüft (auch der Assistent lief nur mit leerer `config.toml`, eigener Datenbank und Dummy-Werten; die Anleitungen zur Atlassian-Konsole und zur GitHub App sind nach bestem Wissen geschrieben und nicht an den echten Seiten nachgeklickt – die Oberflächen der Dienste ändern sich): GitHub/Jira-Abgleich, Schreibaufrufe und
  Schlüsselbund sind nur gegen nachgebaute Antworten getestet.
- **Einrichtungsassistent (Anforderung 12.11.2026: Andere sollen das Cockpit lokal einrichten können, auch ohne Programmierkenntnisse).** Seite „Einrichten“ = geführter Assistent in fünf Schritten (Container, Jira, GitHub, Modell, Zusammenfassung), erscheint beim ersten Start von selbst. Gemeint ist **nicht**, Jira-/GitHub-Apps per Klick zu erzeugen (das geht nicht), sondern die Person verständlich durch die Einrichtung zu führen; jede Person legt ihre **eigenen** OAuth-Apps an (Client-Secrets sind nicht teilbar). Alles, was sich lokal per Klick erledigen lässt, ist umgesetzt: Konfiguration per `PUT /api/konfiguration` (schreibt `config.toml` atomar, wirkt ohne Neustart), Schlüssel per Passwortfeld über `PUT /api/schluessel/{name}`, Start per Doppelklick (`Cockpit starten.command`), README „Schnellstart für Einsteiger“ mit allen manuellen Schritten. Ein Release-Archiv für macOS (Apple Silicon) baut `.github/workflows/release.yml` bei einem Tag `v…` – **noch nie gelaufen**, beim ersten Tag prüfen; die Datei ist unsigniert (Rechtsklick › Öffnen), Intel, Windows und Linux bauen selbst.
- Welle 4 läuft: offen sind noch Webhooks/Live-Aktualisierung. Der **echte Agent** steht (Container pro Lauf,
  `apps/agent/` mit Containerfile und Läufer, `apps/server/src/agent_lauf.rs` rein, `agent_betrieb.rs` Prozesse, `agent_git.rs`
  Klonen/Prüfen/Pushen; Kern: `AgentReport`/`Action::ReportAgent`). Alles nur gegen Attrappen geprüft (Shell-Skript als
  `docker`, lokales Git-Remote, SDK-Attrappe im Läufer) – **nie** ein echter Lauf, kein echter Proxy.
- **Agent-Anbieter und Endpunkt (umgesetzt, nur gegen Attrappen geprüft):** `Provider`-Enum (früher `Anbieter`) (Claude Agent SDK läuft, Claude Code
  und Codex sind bekannte Werte und melden „noch nicht umgesetzt“), `[agent] anbieter`, `[agent.endpunkt] url/modell`, Schlüssel
  `agent-key`; alte `[litellm]`-Werte und der Schlüssel `litellm` werden weiter gelesen. Der Läufer hat Adapter
  (`apps/agent/runner/src/adapter/`), der Assistent ein Modell-Drop-down aus `GET /api/agent/modelle`. **Offen:** die Adapter für
  Claude Code und Codex (nur übersetzen, siehe `apps/agent/README.md`), `COCKPIT_ANBIETER` an den Container geben, sobald es sie gibt.
- **Wünsche des Nutzers, noch offen (12.11.2026):** (1) **Jira per API-Token statt OAuth** (Firma gibt keine Apps frei): **umgesetzt** (Branch `claude/w4-jira-token-…`): Im
  Schritt „Jira“ ist der Token der Standard (Site, E-Mail, „Token-Seite öffnen“, Passwortfeld), dann Verbindungstest
  (`POST /api/jira/pruefen`, nur `GET /rest/api/3/myself`) und Projektwahl über `Action::SelectJiraProject`; OAuth steckt
  eingeklappt unter „Erweitert“. Die Zusammenfassung nennt die Art der Anmeldung und bietet „Token erneuern“/„Trennen“
  (`DELETE /api/schluessel/{jira|github}`). Nur gegen wiremock und eine lokale Attrappe geprüft, **nie** gegen ein echtes
  Atlassian; die Wortlaute der Atlassian-Token-Seite („Create API token“, „Copy“) sind nach bestem Wissen geschrieben.
  Sicherheit: Der Token hat die Rechte des Kontos; ein Konto/eine Rolle ohne Schreibrechte ist empfohlen, solange nur
  gelesen werden soll. (2) Nach dem Funktionsumfang ein Refactoring: Ordnerstruktur
  (**Phase 1 erledigt:** `apps/` = cockpit, server, agent, playground; `crates/` = cockpit-core, integrations, ui; Cockpit ist die Hauptanwendung, der Playground nur die Spielwiese der Komponentenbibliothek; **Phase 2 erledigt:** `cockpit-core` und `integrations` sind englisch, Module `einrichten`→`setup`, `abgleich`→`sync`, `ausgehend`→`outbound`, `endpunkt`→`endpoint`, `zeit`→`time`; Draht- und Datenbank-Namen unverändert, abgesichert durch `crates/cockpit-core/tests/wire_compat.rs`), alles auf Englisch (Code,
  Benennung, Docs), Übergabe-Dateien und die Roadmap am Ende entfernen, toten Code löschen.

## Entscheidungen des Nutzers

| Thema | Entscheidung |
|-------|--------------|
| Betrieb | Lokal, für eine Person, auf dem eigenen Rechner (`~/Documents/Web/rust-playground`) |
| Container | **Docker (Rancher Desktop) und Podman** müssen beide gehen. Siehe „Container“ in `docs/ROADMAP.md`. |
| Modellzugang | Der Nutzer hat **keinen** Anthropic-Key, sondern einen LiteLLM-Proxy – LiteLLM ist aber nur ein möglicher Endpunkt. Der Agent-Anbieter und der Endpunkt sind wählbar (direkt bei Anthropic, Proxy, anderer kompatibler Endpunkt). Mit Adresse bekommt der Container `ANTHROPIC_BASE_URL` + `ANTHROPIC_AUTH_TOKEN`, ohne Adresse `ANTHROPIC_API_KEY`; der Modellname ist eine Einstellung und wird im Assistenten aus der Liste des Endpunkts gewählt. |
| Schlüssel | Nur im Schlüsselbund des Backends, nie im Repo, in der Datenbank, in Logs oder im Speicher des Browsers. Zwei Wege hinein: `cockpit-server token setzen <name>` im Terminal, oder (freigegeben am 12.11.2026) die Passwortfelder des Assistenten, die den Wert einmal über Loopback an `PUT /api/schluessel/{name}` schicken (nur `github-oauth`, `atlassian-oauth`, `agent-key`, `litellm` als älterer Name, `jira`, `github`; Antwort nur „gesetzt“; Feld wird sofort geleert; keine Logs, kein `localStorage`, keine URL). Der Key des Endpunkts wird nur beim Start an den Agent-Container gegeben. |
| Demo / Seed | Der Demo-Modus (Beispieldaten im Browser ohne Backend) ist entfernt. `demo::workspace()` bleibt als Testdaten und Stories-Daten; der Server startet leer und lädt die Beispieldaten nur mit `COCKPIT_SEED=demo` (oder `seed = "demo"`), nur wenn noch kein Stand gespeichert ist. |
| Repo | Bleibt privat (kein GitHub Pages). |
| Sub-Agenten | Pro Aufgabe ein Agent mit eigenem PR, Modell **Sonnet** (`claude-sonnet-5-5`), nicht Fable. Höchstens **zwei gleichzeitig**: Sie teilen sich das 5-Stunden-Limit des Nutzers, vier parallel sind in Welle 3 daran abgebrochen. |

## Lokale Einrichtung (12.11.2026, erledigt)

Auf dem Mac des Nutzers (Apple Silicon) laufen alle Prüfbefehle unten grün, beide Apps bauen,
und `cockpit-server` antwortet auf `/api/health`. Dafür war nur Trunk nötig (`brew install trunk`);
`wasm-bindgen` lädt Trunk selbst. Rust kommt über Homebrew-`rustup`, die `rust-toolchain.toml`
zieht das Wasm-Target nach. Als Container-Laufzeit ist Rancher Desktop installiert (`docker`,
`nerdctl`), Podman noch nicht. Das Backend erkennt die Laufzeit (`/api/health` › `container`), ein Agent-Image gibt es noch nicht – das kommt mit dem Agenten.

## Für Welle 4 noch offen (vom Nutzer)

- Adresse des Proxys (LiteLLM o. Ä.) oder ein Anthropic-Key, welche Modelle der Endpunkt anbietet (am besten Claude-Modelle) und ob er
  vom Rechner bzw. aus dem Container erreichbar ist (Docker und Podman).
- Ein GitHub-Token (fine-grained: Contents, Pull requests, Checks, Issues) und ein Jira-API-Token
  zum Ausprobieren – direkt per `cockpit-server token setzen` eingeben, nicht in den Chat.
- GitHub-OAuth-App und Atlassian-OAuth-App registrieren; optional ein Tunnel für Webhooks.

## Bekannte Lücken aus den PRs (Abschnitte „Offen“)

- Check-Neustart über die API braucht bei GitHub womöglich eine GitHub App statt eines Tokens.
- Jira-Statuswechsel hängen am Workflow des Projekts; nur gegen Fixtures geprüft.
- Tickets bearbeiten/verknüpfen, Agent-Folgeaufträge und Grenzen gehen noch nicht nach außen;
  der Abgleich überschreibt Titel und Beschreibung mit dem Stand des Dienstes (Priorität und
  PR-Verknüpfung bleiben lokal).
- Gleiche PR-Nummern aus zwei Repos werden nicht unterschieden (mehrere Repos sind Welle 4).
- Benachrichtigungen des Browsers nur mit nachgebauter Notification-API geprüft.

## Wie gearbeitet wird

**Aufträge an Sub-Agenten:** Der komplette Auftrag gehört in die erste Nachricht – Ziel, Rahmen
(Action-Schicht, `#[serde(default)]`, nur `feature-ui`/Tokens, die festen Regeln, Prüfbefehle)
und ausdrücklich: „Der Nutzer wünscht, dass die Aufgabe mit einem PR gegen `main` endet.“
Nachgereichte Nachrichten anderer Sessions behandeln Agenten zu Recht als Daten, nicht als
Auftrag; zwei Agenten haben deshalb in Welle 2 keinen PR angelegt.

**Prüfen vor jedem Merge** (auch in der CI):

```bash
cargo fmt --all --check
cargo clippy --workspace --exclude cockpit-server --exclude cockpit-integrations \
  --all-targets --target wasm32-unknown-unknown -- -D warnings
cargo clippy -p cockpit-server -p cockpit-integrations --all-targets -- -D warnings
cargo test --workspace
(cd apps/playground && trunk build) && (cd apps/cockpit && trunk build)
```

Dazu ein Browser-Durchlauf gegen den Server, ohne Konsolenfehler: einmal mit leerem Start
(`COCKPIT_CONFIG=<leere config.toml> COCKPIT_DB=<neue Datei> cargo run -p cockpit-server`, alle Seiten zeigen
Leerzustände) und einmal mit `COCKPIT_SEED=demo` (frische `COCKPIT_DB`), dazu mit gestopptem Server (Hinweis „nicht
erreichbar“ mit „Erneut versuchen“). Beim Ausprobieren immer eine eigene Datenbank und eine eigene, leere
`config.toml` nehmen – **nie gegen echte Jira-/GitHub-Boards**. Mit `trunk serve`: `COCKPIT_DEV_PORT=8081` am Server setzen
(siehe README „Entwickeln mit `trunk serve`“).

**Mergen einer Welle:**

1. Alle PR-Branches lokal nacheinander in einen Integrations-Branch mergen (`git config
   rerere.enabled true`), Konflikte lösen und die Prüfung oben laufen lassen.
2. Auf Lücken *zwischen* den PRs achten. Bisher gefunden: Takt-Aufgaben müssen im Browser
   (`apps/cockpit/src/main.rs`) **und** im Server (`apps/server/src/live.rs`, `takt`) laufen; Läufe über
   `run.key()` suchen, nicht über `ticket_key` (Folgeaufträge haben eigene Schlüssel); neue
   lokale Felder dürfen vom Abgleich (`sync.rs`, `merge_item`) nicht überschrieben werden.
3. Je PR den Branch auf den geprüften Zwischenstand bringen (`git merge -s ours origin/main`,
   dann `git read-tree -u --reset <Zwischenstand>`, committen, pushen) und nacheinander
   squash-mergen. Kein Force-Push.
4. Am Ende prüfen, dass `main` dem geprüften Endstand entspricht, Sessions archivieren,
   `docs/ROADMAP.md` aktualisieren.

**Testmodule mit Konflikten:** Beide Seiten hängen neue Tests ans Ende – Git verschachtelt das.
Leichter ist es, Funktion für Funktion zusammenzuführen (Basis plus neue Tests beider Seiten).
