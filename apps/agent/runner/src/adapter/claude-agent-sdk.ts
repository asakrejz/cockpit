// Adapter „claude-agent-sdk“: der Ablauf eines Laufs mit dem Claude Agent SDK. Das SDK, die Zeilen von stdin, Git und die Uhr kommen von außen
// (`Abhaengigkeiten`), damit der Ablauf ohne Netz und ohne SDK getestet werden kann.

import { type Auftrag, aufgabenText, systemText } from "../auftrag.ts";
import { type Senke, kuerzen } from "../ereignis.ts";
import { type Ausfuehren, festhalten } from "../git.ts";
import { type Richtlinie, entscheide } from "../richtlinie.ts";
import type { Adapter, Eingang } from "./schnittstelle.ts";

export type { Eingang };

/** Eine Nachricht des SDK (nur die Felder, die wir lesen). */
export interface SdkNachricht {
  type: string;
  subtype?: string;
  message?: { id?: string; usage?: Record<string, number>; content?: Array<Record<string, any>> };
  result?: string;
  is_error?: boolean;
  errors?: string[];
}

export interface QueryOptionen {
  cwd: string;
  model: string;
  systemPrompt: string;
  permissionMode: string;
  allowedTools: string[];
  settingSources: string[];
  maxTurns: number;
  env: Record<string, string | undefined>;
  abortController: AbortController;
  canUseTool: (name: string, eingabe: Record<string, unknown>) => Promise<{ behavior: string; message?: string; updatedInput?: Record<string, unknown> }>;
  mcpServers?: Record<string, unknown>;
}

export interface Sdk {
  query(p: { prompt: AsyncIterable<unknown>; options: QueryOptionen }): AsyncIterable<SdkNachricht>;
  /** Baut den MCP-Server mit dem Werkzeug „rueckfrage“. */
  rueckfrageServer(frage: (text: string, vorschlaege: string[]) => Promise<string>): unknown;
}

export interface Abhaengigkeiten {
  sdk: Sdk;
  senke: Senke;
  git: Ausfuehren;
  eingang: Eingang;
  /** Arbeitsverzeichnis im Container. */
  wurzel: string;
  jetzt: () => number;
  umgebung: Record<string, string | undefined>;
}

/** Eine Warteschlange, die ein `AsyncIterable` speist (für den Streaming-Eingang des SDK). */
export class Warteschlange<T> {
  private puffer: T[] = [];
  private wartende: Array<(w: IteratorResult<T>) => void> = [];
  private zu = false;
  push(w: T): void {
    if (this.zu) return;
    const e = this.wartende.shift();
    if (e) e({ value: w, done: false });
    else this.puffer.push(w);
  }
  schliessen(): void {
    this.zu = true;
    for (const e of this.wartende.splice(0)) e({ value: undefined as never, done: true });
  }
  [Symbol.asyncIterator](): AsyncIterator<T> {
    return {
      next: () => {
        const w = this.puffer.shift();
        if (w !== undefined) return Promise.resolve({ value: w, done: false });
        if (this.zu) return Promise.resolve({ value: undefined as never, done: true });
        return new Promise((ok) => this.wartende.push(ok));
      },
    };
  }
}

function nutzernachricht(text: string) {
  return { type: "user", message: { role: "user", content: text }, parent_tool_use_id: null, session_id: "" };
}

/** Kurzer Satz zu einem Werkzeugaufruf, für den Verlauf im Cockpit. */
export function beschreibe(name: string, eingabe: Record<string, any>): { titel: string; text: string } {
  const pfad = String(eingabe.file_path ?? eingabe.path ?? eingabe.pattern ?? "");
  switch (name) {
    case "Read": return { titel: "Datei lesen", text: pfad };
    case "Write": return { titel: "Datei schreiben", text: pfad };
    case "Edit":
    case "MultiEdit": return { titel: "Datei ändern", text: pfad };
    case "Glob":
    case "Grep": return { titel: "Code durchsuchen", text: pfad };
    case "Bash": return { titel: "Befehl ausführen", text: String(eingabe.command ?? "") };
    default: return { titel: name, text: "" };
  }
}

/** Zieht den Plan (JSON-Block) aus der Antwort des Modells; ohne Block wird die Antwort zur Zusammenfassung. */
export function planAusAntwort(text: string): { summary: string; files: string[]; steps: string[] } {
  const block = /```json\s*([\s\S]*?)```/i.exec(text)?.[1] ?? /(\{[\s\S]*\})/.exec(text)?.[1];
  if (block) {
    try {
      const j = JSON.parse(block);
      const liste = (x: unknown) => (Array.isArray(x) ? x.map(String) : []);
      return { summary: String(j.summary ?? ""), files: liste(j.files), steps: liste(j.steps) };
    } catch {
      /* fällt unten auf die Antwort zurück */
    }
  }
  return { summary: kuerzen(text.trim(), 600), files: [], steps: [] };
}

function usageVon(m: SdkNachricht): { ein: number; aus: number } {
  const u = m.message?.usage ?? {};
  const ein = (u.input_tokens ?? 0) + (u.cache_creation_input_tokens ?? 0) + (u.cache_read_input_tokens ?? 0);
  return { ein, aus: u.output_tokens ?? 0 };
}

const SCHREIBER = new Set(["Write", "Edit", "MultiEdit"]);

/** Führt den Auftrag aus. Gibt den Exit-Code des Prozesses zurück (0 = ein Ergebnis wurde gemeldet). */
export async function lauf(a: Auftrag, d: Abhaengigkeiten): Promise<number> {
  const modell = d.umgebung.ANTHROPIC_MODEL ?? "";
  if (!modell) {
    d.senke({ t: "fehler", text: "Es ist kein Modell eingestellt (ANTHROPIC_MODEL fehlt)." });
    return 1;
  }
  const planen = a.modus === "plan";
  const eingabe = new Warteschlange<unknown>();
  const abbruch = new AbortController();
  const frist = setTimeout(() => abbruch.abort(), a.max_sekunden * 1000);
  const richtlinie: Richtlinie = {
    wurzel: d.wurzel,
    erlaubt: a.regeln.erlaubte_pfade,
    verboten: a.regeln.verbotene_pfade,
  };
  const geaendert = new Set<string>();
  const gesehen = new Set<string>();
  let letzterStart = d.jetzt();

  const antworten = new Warteschlange<string>();
  // Hinweise des Nutzers: als weitere Nachricht an den laufenden Agenten.
  const hinweisPumpe = (async () => {
    // Antworten holt das Werkzeug „rueckfrage“ selbst; hier werden nur Hinweise weitergereicht.
    for (;;) {
      const z = await d.eingang.naechste();
      if (!z) return;
      if (z.t === "hinweis") {
        eingabe.push(nutzernachricht(`Hinweis des Nutzers: ${z.text}`));
        d.senke({ t: "hinweise_angekommen" });
      } else if (z.t === "antwort") {
        antworten.push(z.text);
      }
    }
  })();

  const rueckfrage = async (text: string, vorschlaege: string[]): Promise<string> => {
    d.senke({ t: "frage", text, vorschlaege });
    const it = antworten[Symbol.asyncIterator]();
    const r = await it.next();
    letzterStart = d.jetzt(); // Wartezeit auf den Nutzer zählt nicht als Laufzeit
    return r.done ? "(keine Antwort)" : r.value;
  };

  const optionen: QueryOptionen = {
    cwd: d.wurzel,
    model: modell,
    systemPrompt: systemText(a),
    permissionMode: planen ? "plan" : "default",
    allowedTools: planen ? ["Read", "Glob", "Grep"] : ["Read", "Glob", "Grep", "Write", "Edit", "MultiEdit", "Bash", "mcp__cockpit__rueckfrage"],
    settingSources: [],
    maxTurns: planen ? 25 : 120,
    env: {
      ...d.umgebung,
      CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC: "1",
      ANTHROPIC_SMALL_FAST_MODEL: modell,
      ANTHROPIC_DEFAULT_HAIKU_MODEL: modell,
      ANTHROPIC_DEFAULT_SONNET_MODEL: modell,
      ANTHROPIC_DEFAULT_OPUS_MODEL: modell,
    },
    abortController: abbruch,
    canUseTool: async (name, eing) => {
      const e = entscheide(richtlinie, name, eing);
      return e.behavior === "allow" ? { behavior: "allow", updatedInput: eing } : { behavior: "deny", message: e.message };
    },
    ...(planen ? {} : { mcpServers: { cockpit: d.sdk.rueckfrageServer(rueckfrage) } }),
  };

  eingabe.push(nutzernachricht(aufgabenText(a)));
  let endText = "";
  let fehler: string | null = null;
  try {
    for await (const m of d.sdk.query({ prompt: eingabe, options: optionen })) {
      if (m.type === "assistant" && m.message) {
        const id = m.message.id ?? "";
        const { ein, aus } = id && gesehen.has(id) ? { ein: 0, aus: 0 } : usageVon(m);
        if (id) gesehen.add(id);
        const jetzt = d.jetzt();
        const ms = Math.max(0, jetzt - letzterStart);
        letzterStart = jetzt;
        let erster = true;
        for (const b of m.message.content ?? []) {
          if (b.type === "tool_use") {
            const { titel, text } = beschreibe(String(b.name), b.input ?? {});
            d.senke({ t: "schritt", titel, text, token_ein: erster ? ein : 0, token_aus: erster ? aus : 0, ms: erster ? ms : 0 });
            erster = false;
            if (SCHREIBER.has(String(b.name))) {
              const p = String(b.input?.file_path ?? "");
              const rel = p.startsWith(`${d.wurzel}/`) ? p.slice(d.wurzel.length + 1) : p;
              if (rel && !geaendert.has(rel)) {
                geaendert.add(rel);
                d.senke({ t: "dateien", dateien: [...geaendert] });
              }
            }
          } else if (b.type === "text" && String(b.text ?? "").trim()) {
            endText = String(b.text);
          }
        }
        if (erster && (ein > 0 || aus > 0)) {
          d.senke({ t: "schritt", titel: "Überlegt", text: "", token_ein: ein, token_aus: aus, ms });
        }
      } else if (m.type === "result") {
        if (m.subtype === "success" && !m.is_error) endText = m.result ?? endText;
        else fehler = (m.errors ?? []).join("; ") || `Das Modell hat den Lauf mit „${m.subtype ?? "Fehler"}“ beendet.`;
        eingabe.schliessen();
      }
    }
  } catch (e) {
    fehler = abbruch.signal.aborted
      ? `Das Zeitlimit von ${Math.round(a.max_sekunden / 60)} Minuten im Container ist überschritten.`
      : `Das SDK meldet: ${(e as Error).message}`;
  } finally {
    clearTimeout(frist);
    eingabe.schliessen();
    antworten.schliessen();
  }
  void hinweisPumpe.catch(() => {});

  if (fehler) {
    d.senke({ t: "fehler", text: fehler });
    return 1;
  }
  if (planen) {
    const p = planAusAntwort(endText);
    d.senke({ t: "plan", summary: p.summary, files: p.files, steps: p.steps });
    return 0;
  }
  try {
    const f = await festhalten(d.git, d.wurzel, a.branch, `${a.ticket.key}: ${a.ticket.titel}\n\nUmgesetzt vom Cockpit-Agenten.`);
    if (!f) {
      d.senke({ t: "fehler", text: "Der Agent hat nichts geändert, es gibt nichts zu pushen." });
      return 1;
    }
    d.senke({ t: "dateien", dateien: f.dateien });
    d.senke({ t: "ergebnis", text: kuerzen(endText.trim() || `${a.ticket.key} umgesetzt.`, 1500), branch: a.branch, commit: f.commit });
    return 0;
  } catch (e) {
    d.senke({ t: "fehler", text: (e as Error).message });
    return 1;
  }
}

/** Lädt das echte SDK (erst im Container; die Tests geben eine Attrappe mit). */
export async function ladeSdk(): Promise<Sdk> {
  const m: any = await import("@anthropic-ai/claude-agent-sdk");
  const { z } = await import("zod");
  return {
    query: (p) => m.query(p),
    rueckfrageServer: (frage) =>
      m.createSdkMcpServer({
        name: "cockpit",
        version: "1.0.0",
        tools: [
          m.tool(
            "rueckfrage",
            "Stellt dem Nutzer eine Frage und wartet auf seine Antwort. Nur für Dinge, die du aus dem Code nicht klären kannst.",
            { frage: z.string(), vorschlaege: z.array(z.string()).optional() },
            async (a: { frage: string; vorschlaege?: string[] }) => ({
              content: [{ type: "text", text: await frage(a.frage, a.vorschlaege ?? []) }],
            }),
          ),
        ],
      }),
  };
}

/** Der Adapter für `COCKPIT_ANBIETER=claude-agent-sdk`. `sdk` lädt das SDK erst bei Bedarf. */
export function claudeAgentSdk(sdk: () => Promise<Sdk> = ladeSdk): Adapter {
  return {
    name: "claude-agent-sdk",
    async ausfuehren(auftrag, d) {
      let geladen: Sdk;
      try {
        geladen = await sdk();
      } catch (e) {
        d.senke({ t: "fehler", text: `Das Agent SDK lässt sich nicht laden (${(e as Error).message}). Image neu bauen?` });
        return 2;
      }
      return lauf(auftrag, { ...d, sdk: geladen });
    },
  };
}
