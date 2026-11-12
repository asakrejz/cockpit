# Übergabe: Cockpit weiterbauen

Stand: 12.11.2026, `main` nach #24. Diese Notiz ergänzt `docs/ROADMAP.md` um das, was bisher nur
im Gespräch stand. Eine neue Session liest zuerst diese Datei, dann `docs/ROADMAP.md` und `README.md`.

## Wo wir stehen

- Wellen 0–3 sind gemergt (#5–#24), in Welle 4 dazu OAuth (#29), Inbox-Filter (#28) und das
  Onboarding (Seite „Einrichten“, Seed, Start-Skript). Cockpit läuft **nur noch mit dem Server**
  (`scripts/start.sh`, App unter `http://127.0.0.1:8787`); den Demo-Modus im Browser gibt es nicht mehr.
- Gegen echte Dienste ist noch **nichts** geprüft: GitHub/Jira-Abgleich, Schreibaufrufe und
  Schlüsselbund sind nur gegen nachgebaute Antworten getestet.
- Welle 4 läuft: offen sind noch Webhooks/Live-Aktualisierung und der echte Agent (Claude Agent SDK im
  Container, Modell über LiteLLM, Agent-Image bauen). Die Einrichtung dafür (LiteLLM-Verbindungstest,
  Container-Erkennung) steht.

## Entscheidungen des Nutzers

| Thema | Entscheidung |
|-------|--------------|
| Betrieb | Lokal, für eine Person, auf dem eigenen Rechner (`~/Documents/Web/rust-playground`) |
| Container | **Docker (Rancher Desktop) und Podman** müssen beide gehen. Siehe „Container“ in `docs/ROADMAP.md`. |
| Modellzugang | **Kein Anthropic-API-Key, sondern ein LiteLLM-Proxy.** Der echte Agent (Claude Agent SDK) bekommt `ANTHROPIC_BASE_URL` = Proxy-Adresse und `ANTHROPIC_AUTH_TOKEN` = LiteLLM-Key; der Modellname ist eine Einstellung in Cockpit, weil er von der LiteLLM-Konfiguration abhängt. |
| Schlüssel | Nur im Schlüsselbund des Backends (`cockpit-server token setzen <github\|jira\|litellm>`), nie im Browser, Repo, in der Datenbank oder in Logs. Der LiteLLM-Key wird nur beim Start an den Agent-Container gegeben. |
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

- Adresse des LiteLLM-Proxys, welche Modelle er anbietet (am besten Claude-Modelle) und ob er
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
trunk build && (cd cockpit && trunk build)
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
