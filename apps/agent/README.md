# Agent-Image

Der Agent des Cockpits läuft pro Lauf in einem eigenen Container (Docker oder Podman). Dieses
Verzeichnis enthält das Image:

- `Containerfile` – Node 24, Git, der Läufer; läuft als Nutzer `node` (nicht root).
- `runner/` – der Läufer (TypeScript, ohne Build-Schritt: Node führt `.ts` direkt aus). Er liest den
  Auftrag aus `/auftrag/auftrag.json` und führt ihn mit einem **Adapter** aus (siehe unten).

## Adapter

Jedes Agenten-Werkzeug ist ein Adapter in `runner/src/adapter/` mit der gemeinsamen Schnittstelle aus
`schnittstelle.ts`: Auftrag rein (plus Senke, Git, stdin-Eingang, Umgebung), JSON-Zeilen-Ereignisse raus
(Tabelle unten), Exit-Code als Ergebnis. `COCKPIT_PROVIDER` (Standard `claude-agent-sdk`; im Image gesetzt) wählt den
Adapter; unbekannte und noch nicht umgesetzte Namen ergeben ein `fehler`-Ereignis in Klartext.

| Adapter | Stand | Werkzeug |
|---------|-------|----------|
| `claude-agent-sdk` | umgesetzt (`adapter/claude-agent-sdk.ts`) | Claude Agent SDK; Modell über `ANTHROPIC_MODEL` |
| `claude-code` | **offen** | `claude -p --output-format stream-json`: dessen Zeilen (Assistant-Nachrichten, Tool-Aufrufe, Ergebnis mit Verbrauch) nur in die Ereignisse unten übersetzen |
| `codex` | **offen** | `codex exec --json`: dessen Ereignisse (Nachrichten, Befehle, Dateiänderungen, Verbrauch) in dieselben Ereignisse übersetzen |

Ein neuer Adapter muss nichts am Backend ändern: gleiche Ereignisformen, gleiche Regeln (kein Push, nur in `/work`
schreiben, `git` zum Festhalten über `git.ts`).

## Endpunkt in der Umgebung

Das Backend gibt die Werte nur per `-e NAME` an den Container (nie in der Argumentliste). Mit Endpunkt-Adresse
(Proxy wie LiteLLM, anderer kompatibler Endpunkt): `ANTHROPIC_BASE_URL` und `ANTHROPIC_AUTH_TOKEN` (Bearer). Ohne Adresse
(direkt bei Anthropic): `ANTHROPIC_API_KEY` (`x-api-key`). Dazu immer `ANTHROPIC_MODEL`. Nicht benutzte Namen entfernt das
Backend aus der geerbten Umgebung, damit kein Key aus der Shell der Person im Container landet.

## Ereignisse (stdout, eine JSON-Zeile je Ereignis)

| `t` | Felder | Bedeutung |
|-----|--------|-----------|
| `plan` | `summary`, `files`, `steps` | Plan (Modus `plan`) |
| `schritt` | `titel`, `text`, `token_ein`, `token_aus`, `ms` | ein Arbeitsschritt mit Verbrauch |
| `dateien` | `dateien` | bisher geänderte Dateien |
| `frage` | `text`, `vorschlaege` | Rückfrage; der Läufer wartet auf `antwort` auf stdin |
| `hinweise_angekommen` | – | ein Hinweis des Nutzers ist beim Agenten |
| `ergebnis` | `text`, `branch`, `commit` | fertig; der Commit liegt auf dem Branch im Klon |
| `pruefung` | `erreichbar`, `key_ok`, `hinweis` | Ergebnis des Prüfmodus |
| `fehler` | `text` | Abbruch mit Grund |

Auf stdin kommen Zeilen `{"t":"antwort","text":…}` und `{"t":"hinweis","text":…}`.

## Regeln im Container

Der Container hat **kein GitHub-Token**: Er kann nicht pushen. Er committet auf einen Branch
`claude/<ticket>-…` im eingebundenen Klon; geprüft und gepusht wird danach vom Backend (Pfad-Regeln
des Repos, nie der Hauptbranch, nie ein Merge). `canUseTool` im Läufer sperrt zusätzlich Schreiben
außerhalb von `/work`, `.git`, verbotene Pfade und Befehle wie `git push`, `curl`.

## Tests

```bash
cd apps/agent/runner && npm install && npm test   # Node ≥ 22.18, ohne Netz, das SDK ist eine Attrappe
```
