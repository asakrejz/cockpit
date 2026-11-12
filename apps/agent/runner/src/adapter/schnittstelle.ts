// Die gemeinsame Schnittstelle der Adapter. Ein Adapter führt einen Auftrag mit einem
// Agenten-Werkzeug aus (Claude Agent SDK, später Claude Code und Codex) und meldet das Ergebnis
// als JSON-Zeilen-Ereignisse (siehe ereignis.ts und apps/agent/README.md). Alles, was ein Werkzeug
// anders macht, bleibt im Adapter; das Backend sieht immer dieselben Ereignisformen.

import type { Auftrag } from "../auftrag.ts";
import type { Senke } from "../ereignis.ts";
import type { Ausfuehren } from "../git.ts";

/** Zeilen von stdin: Antworten und Hinweise des Nutzers. */
export interface Eingang {
  naechste(): Promise<{ t: string; text: string } | null>;
}

/** Was jeder Adapter von außen bekommt. Nichts davon ist an ein Werkzeug gebunden. */
export interface AdapterUmgebung {
  /** Hierhin gehören alle Ereignisse. */
  senke: Senke;
  /** Git im Klon (zum Festhalten der Änderungen auf dem Branch). */
  git: Ausfuehren;
  /** Antworten und Hinweise des Nutzers von stdin. */
  eingang: Eingang;
  /** Arbeitsverzeichnis im Container. */
  wurzel: string;
  jetzt: () => number;
  /** Die Umgebungsvariablen des Containers (Modell, Endpunkt, Key). Nie ausgeben. */
  umgebung: Record<string, string | undefined>;
}

export interface Adapter {
  /** Die Kennung aus `[agent] anbieter`, z. B. `claude-agent-sdk`. */
  readonly name: string;
  /** Führt den Auftrag aus. Rückgabe: Exit-Code (0 = fertig, 2 = Abbruch mit `fehler`-Ereignis). */
  ausfuehren(auftrag: Auftrag, d: AdapterUmgebung): Promise<number>;
}
