# Agent-Image

Der Agent des Cockpits läuft pro Lauf in einem eigenen Container (Docker oder Podman). Dieses
Verzeichnis enthält das Image:

- `Containerfile` – Node 24, Git, der Läufer; läuft als Nutzer `node` (nicht root).
- `runner/` – der Läufer (TypeScript, ohne Build-Schritt: Node führt `.ts` direkt aus) mit dem
  Claude Agent SDK. Er liest den Auftrag aus `/auftrag/auftrag.json` und spricht über
  `ANTHROPIC_BASE_URL` / `ANTHROPIC_AUTH_TOKEN` / `ANTHROPIC_MODEL` mit dem LiteLLM-Proxy.

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
cd agent/runner && npm install && npm test   # Node ≥ 22.18, ohne Netz, das SDK ist eine Attrappe
```
