# Übergabe: Cockpit weiterbauen

Stand: 12.11.2026, `main` nach #24. Diese Notiz ergänzt `docs/ROADMAP.md` um das, was bisher nur
im Gespräch stand. Eine neue Session liest zuerst diese Datei, dann `docs/ROADMAP.md` und `README.md`.

## Wo wir stehen

- Wellen 0–3 sind gemergt (#5–#24). Cockpit läuft als Demo im Browser und im Server-Modus
  (`cargo run -p cockpit-server`, App unter `http://127.0.0.1:8787`).
- Gegen echte Dienste ist noch **nichts** geprüft: GitHub/Jira-Abgleich, Schreibaufrufe und
  Schlüsselbund sind nur gegen nachgebaute Antworten getestet.
- Als Nächstes kommt Welle 4. Sie ist noch nicht gestartet.

## Entscheidungen des Nutzers

| Thema | Entscheidung |
|-------|--------------|
| Betrieb | Lokal, für eine Person, auf dem eigenen Rechner (`~/Documents/Web/rust-playground`) |
| Container | **Podman** statt Docker |
| Modellzugang | **Kein Anthropic-API-Key, sondern ein LiteLLM-Proxy.** Der echte Agent (Claude Agent SDK) bekommt `ANTHROPIC_BASE_URL` = Proxy-Adresse und `ANTHROPIC_AUTH_TOKEN` = LiteLLM-Key; der Modellname ist eine Einstellung in Cockpit, weil er von der LiteLLM-Konfiguration abhängt. |
| Schlüssel | Nur im Schlüsselbund des Backends (`cockpit-server token setzen <github\|jira\|litellm>`), nie im Browser, Repo, in der Datenbank oder in Logs. Der LiteLLM-Key wird nur beim Start an den Podman-Container gegeben. |
| Repo | Bleibt privat (kein GitHub Pages). |
| Sub-Agenten | Pro Aufgabe ein Agent mit eigenem PR, Modell **Sonnet** (`claude-sonnet-5-5`), nicht Fable. Höchstens **zwei gleichzeitig**: Sie teilen sich das 5-Stunden-Limit des Nutzers, vier parallel sind in Welle 3 daran abgebrochen. |

## Für Welle 4 noch offen (vom Nutzer)

- Adresse des LiteLLM-Proxys, welche Modelle er anbietet (am besten Claude-Modelle) und ob er
  vom Rechner bzw. aus dem Podman-Container erreichbar ist.
- Ein GitHub-Token (fine-grained: Contents, Pull requests, Checks, Issues) und ein Jira-API-Token
  zum Ausprobieren – direkt per `cockpit-server token setzen` eingeben, nicht in den Chat.
- GitHub-OAuth-App und Atlassian-OAuth-App registrieren; optional ein Tunnel für Webhooks.
- In `docs/ROADMAP.md` steht unter „Was nur du tun kannst“ noch „Anthropic-API-Key“ – beim Start
  von Welle 4 durch LiteLLM ersetzen.

## Bekannte Lücken aus den PRs (Abschnitte „Offen“)

- Check-Neustart über die API braucht bei GitHub womöglich eine GitHub App statt eines Tokens.
- Jira-Statuswechsel hängen am Workflow des Projekts; nur gegen Fixtures geprüft.
- Tickets bearbeiten/verknüpfen, Agent-Folgeaufträge und Grenzen gehen noch nicht nach außen;
  der Abgleich überschreibt Titel und Beschreibung mit dem Stand des Dienstes (Priorität und
  PR-Verknüpfung bleiben lokal).
- Gleiche PR-Nummern aus zwei Repos werden nicht unterschieden (mehrere Repos sind Welle 4).
- Im Server-Modus zeigt die Marke oben noch „Demo“.
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
trunk build && (cd cockpit && trunk build)
```

Dazu ein Browser-Durchlauf im Demo-Modus (`localStorage['cockpit.connections']='111111'`) und im
Server-Modus (`localStorage['cockpit.mode']='server'`), ohne Konsolenfehler.

**Mergen einer Welle:**

1. Alle PR-Branches lokal nacheinander in einen Integrations-Branch mergen (`git config
   rerere.enabled true`), Konflikte lösen und die Prüfung oben laufen lassen.
2. Auf Lücken *zwischen* den PRs achten. Bisher gefunden: Takt-Aufgaben müssen im Browser
   (`cockpit/src/main.rs`) **und** im Server (`server/src/live.rs`, `takt`) laufen; Läufe über
   `run.key()` suchen, nicht über `ticket_key` (Folgeaufträge haben eigene Schlüssel); neue
   lokale Felder dürfen vom Abgleich (`sync.rs`, `merge_item`) nicht überschrieben werden.
3. Je PR den Branch auf den geprüften Zwischenstand bringen (`git merge -s ours origin/main`,
   dann `git read-tree -u --reset <Zwischenstand>`, committen, pushen) und nacheinander
   squash-mergen. Kein Force-Push.
4. Am Ende prüfen, dass `main` dem geprüften Endstand entspricht, Sessions archivieren,
   `docs/ROADMAP.md` aktualisieren.

**Testmodule mit Konflikten:** Beide Seiten hängen neue Tests ans Ende – Git verschachtelt das.
Leichter ist es, Funktion für Funktion zusammenzuführen (Basis plus neue Tests beider Seiten).
