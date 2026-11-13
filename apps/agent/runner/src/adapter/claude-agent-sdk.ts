// Adapter „claude-agent-sdk“: der Ablauf eines Laufs mit dem Claude Agent SDK. Das SDK, die Zeilen von stdin, Git und die Uhr kommen von außen
// (`Abhaengigkeiten`), damit der Ablauf ohne Netz und ohne SDK getestet werden kann.

import { type Job, tasksText, systemText } from "../auftrag.ts";
import { type Sink, truncate } from "../ereignis.ts";
import { type Run, record } from "../git.ts";
import { type Policy, decide } from "../richtlinie.ts";
import type { Adapter, Input } from "./schnittstelle.ts";

export type { Input };

/** Eine Nachricht des SDK (nur die Felder, die wir lesen). */
export interface SdkMessage {
  type: string;
  subtype?: string;
  message?: { id?: string; usage?: Record<string, number>; content?: Array<Record<string, any>> };
  result?: string;
  is_error?: boolean;
  errors?: string[];
}

export interface QueryOptions {
  cwd: string;
  model: string;
  systemPrompt: string;
  permissionMode: string;
  allowedTools: string[];
  settingSources: string[];
  maxTurns: number;
  env: Record<string, string | undefined>;
  abortController: AbortController;
  canUseTool: (name: string, input: Record<string, unknown>) => Promise<{ behavior: string; message?: string; updatedInput?: Record<string, unknown> }>;
  mcpServers?: Record<string, unknown>;
}

export interface Sdk {
  query(p: { prompt: AsyncIterable<unknown>; options: QueryOptions }): AsyncIterable<SdkMessage>;
  /** Baut den MCP-Server mit dem Werkzeug „rueckfrage“. */
  askUserServer(question: (text: string, suggestions: string[]) => Promise<string>): unknown;
}

export interface Dependencies {
  sdk: Sdk;
  sink: Sink;
  git: Run;
  input: Input;
  /** Arbeitsverzeichnis im Container. */
  wurzel: string;
  now: () => number;
  env: Record<string, string | undefined>;
}

/** Eine Warteschlange, die ein `AsyncIterable` speist (für den Streaming-Eingang des SDK). */
export class Queue<T> {
  private buffer: T[] = [];
  private waiting: Array<(w: IteratorResult<T>) => void> = [];
  private zu = false;
  push(w: T): void {
    if (this.zu) return;
    const e = this.waiting.shift();
    if (e) e({ value: w, done: false });
    else this.buffer.push(w);
  }
  close(): void {
    this.zu = true;
    for (const e of this.waiting.splice(0)) e({ value: undefined as never, done: true });
  }
  [Symbol.asyncIterator](): AsyncIterator<T> {
    return {
      next: () => {
        const w = this.buffer.shift();
        if (w !== undefined) return Promise.resolve({ value: w, done: false });
        if (this.zu) return Promise.resolve({ value: undefined as never, done: true });
        return new Promise((ok) => this.waiting.push(ok));
      },
    };
  }
}

function user_message(text: string) {
  return { type: "user", message: { role: "user", content: text }, parent_tool_use_id: null, session_id: "" };
}

/** Kurzer Satz zu einem Werkzeugaufruf, für den Verlauf im Cockpit. */
export function describe(name: string, input: Record<string, any>): { title: string; text: string } {
  const path = String(input.file_path ?? input.path ?? input.pattern ?? "");
  switch (name) {
    case "Read": return { title: "Datei lesen", text: path };
    case "Write": return { title: "Datei schreiben", text: path };
    case "Edit":
    case "MultiEdit": return { title: "Datei ändern", text: path };
    case "Glob":
    case "Grep": return { title: "Code durchsuchen", text: path };
    case "Bash": return { title: "Befehl ausführen", text: String(input.command ?? "") };
    default: return { title: name, text: "" };
  }
}

/** Zieht den Plan (JSON-Block) aus der Antwort des Modells; ohne Block wird die Antwort zur Zusammenfassung. */
export function planAusAnswer(text: string): { summary: string; files: string[]; steps: string[] } {
  const block = /```json\s*([\s\S]*?)```/i.exec(text)?.[1] ?? /(\{[\s\S]*\})/.exec(text)?.[1];
  if (block) {
    try {
      const j = JSON.parse(block);
      const list = (x: unknown) => (Array.isArray(x) ? x.map(String) : []);
      return { summary: String(j.summary ?? ""), files: list(j.files), steps: list(j.steps) };
    } catch {
      /* fällt unten auf die Antwort zurück */
    }
  }
  return { summary: truncate(text.trim(), 600), files: [], steps: [] };
}

function usageFrom(m: SdkMessage): { ein: number; aus: number } {
  const u = m.message?.usage ?? {};
  const ein = (u.input_tokens ?? 0) + (u.cache_creation_input_tokens ?? 0) + (u.cache_read_input_tokens ?? 0);
  return { ein, aus: u.output_tokens ?? 0 };
}

const Writer = new Set(["Write", "Edit", "MultiEdit"]);

/** Führt den Auftrag aus. Gibt den Exit-Code des Prozesses zurück (0 = ein Ergebnis wurde gemeldet). */
export async function run(a: Job, d: Dependencies): Promise<number> {
  const model = d.env.ANTHROPIC_MODEL ?? "";
  if (!model) {
    d.sink({ t: "error", text: "Es ist kein Modell eingestellt (ANTHROPIC_MODEL fehlt)." });
    return 1;
  }
  const plan = a.mode === "plan";
  const input = new Queue<unknown>();
  const cancel = new AbortController();
  const frist = setTimeout(() => cancel.abort(), a.max_seconds * 1000);
  const policy: Policy = {
    wurzel: d.wurzel,
    allowed: a.rules.allowed_paths,
    forbidden: a.rules.forbidden_paths,
  };
  const changed = new Set<string>();
  const seen = new Set<string>();
  let lastStart = d.now();

  const answers = new Queue<string>();
  // Hinweise des Nutzers: als weitere Nachricht an den laufenden Agenten.
  const hintPump = (async () => {
    // Antworten holt das Werkzeug „rueckfrage“ selbst; hier werden nur Hinweise weitergereicht.
    for (;;) {
      const z = await d.input.next();
      if (!z) return;
      if (z.t === "hint") {
        input.push(user_message(`Hinweis des Nutzers: ${z.text}`));
        d.sink({ t: "hints_arrived" });
      } else if (z.t === "answer") {
        answers.push(z.text);
      }
    }
  })();

  const ask_user = async (text: string, suggestions: string[]): Promise<string> => {
    d.sink({ t: "question", text, suggestions });
    const it = answers[Symbol.asyncIterator]();
    const r = await it.next();
    lastStart = d.now(); // Wartezeit auf den Nutzer zählt nicht als Laufzeit
    return r.done ? "(keine Antwort)" : r.value;
  };

  const options: QueryOptions = {
    cwd: d.wurzel,
    model: model,
    systemPrompt: systemText(a),
    permissionMode: plan ? "plan" : "default",
    allowedTools: plan ? ["Read", "Glob", "Grep"] : ["Read", "Glob", "Grep", "Write", "Edit", "MultiEdit", "Bash", "mcp__cockpit__rueckfrage"],
    settingSources: [],
    maxTurns: plan ? 25 : 120,
    env: {
      ...d.env,
      CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC: "1",
      ANTHROPIC_SMALL_FAST_MODEL: model,
      ANTHROPIC_DEFAULT_HAIKU_MODEL: model,
      ANTHROPIC_DEFAULT_SONNET_MODEL: model,
      ANTHROPIC_DEFAULT_OPUS_MODEL: model,
    },
    abortController: cancel,
    canUseTool: async (name, input) => {
      const e = decide(policy, name, input);
      return e.behavior === "allow" ? { behavior: "allow", updatedInput: input } : { behavior: "deny", message: e.message };
    },
    ...(plan ? {} : { mcpServers: { cockpit: d.sdk.askUserServer(ask_user) } }),
  };

  input.push(user_message(tasksText(a)));
  let endText = "";
  let error: string | null = null;
  try {
    for await (const m of d.sdk.query({ prompt: input, options: options })) {
      if (m.type === "assistant" && m.message) {
        const id = m.message.id ?? "";
        const { ein, aus } = id && seen.has(id) ? { ein: 0, aus: 0 } : usageFrom(m);
        if (id) seen.add(id);
        const now = d.now();
        const ms = Math.max(0, now - lastStart);
        lastStart = now;
        let first = true;
        for (const b of m.message.content ?? []) {
          if (b.type === "tool_use") {
            const { title, text } = describe(String(b.name), b.input ?? {});
            d.sink({ t: "step", title, text, tokens_in: first ? ein : 0, tokens_out: first ? aus : 0, ms: first ? ms : 0 });
            first = false;
            if (Writer.has(String(b.name))) {
              const p = String(b.input?.file_path ?? "");
              const rel = p.startsWith(`${d.wurzel}/`) ? p.slice(d.wurzel.length + 1) : p;
              if (rel && !changed.has(rel)) {
                changed.add(rel);
                d.sink({ t: "files", files: [...changed] });
              }
            }
          } else if (b.type === "text" && String(b.text ?? "").trim()) {
            endText = String(b.text);
          }
        }
        if (first && (ein > 0 || aus > 0)) {
          d.sink({ t: "step", title: "Überlegt", text: "", tokens_in: ein, tokens_out: aus, ms });
        }
      } else if (m.type === "result") {
        if (m.subtype === "success" && !m.is_error) endText = m.result ?? endText;
        else error = (m.errors ?? []).join("; ") || `Das Modell hat den Lauf mit „${m.subtype ?? "Error"}“ beendet.`;
        input.close();
      }
    }
  } catch (e) {
    error = cancel.signal.aborted
      ? `Das Zeitlimit von ${Math.round(a.max_seconds / 60)} Minuten im Container ist überschritten.`
      : `Das SDK meldet: ${(e as Error).message}`;
  } finally {
    clearTimeout(frist);
    input.close();
    answers.close();
  }
  void hintPump.catch(() => {});

  if (error) {
    d.sink({ t: "error", text: error });
    return 1;
  }
  if (plan) {
    const p = planAusAnswer(endText);
    d.sink({ t: "plan", summary: p.summary, files: p.files, steps: p.steps });
    return 0;
  }
  try {
    const f = await record(d.git, d.wurzel, a.branch, `${a.ticket.key}: ${a.ticket.title}\n\nUmgesetzt vom Cockpit-Agenten.`);
    if (!f) {
      d.sink({ t: "error", text: "Der Agent hat nichts geändert, es gibt nichts zu pushen." });
      return 1;
    }
    d.sink({ t: "files", files: f.files });
    d.sink({ t: "result", text: truncate(endText.trim() || `${a.ticket.key} umgesetzt.`, 1500), branch: a.branch, commit: f.commit });
    return 0;
  } catch (e) {
    d.sink({ t: "error", text: (e as Error).message });
    return 1;
  }
}

/** Lädt das echte SDK (erst im Container; die Tests geben eine Attrappe mit). */
export async function ladeSdk(): Promise<Sdk> {
  const m: any = await import("@anthropic-ai/claude-agent-sdk");
  const { z } = await import("zod");
  return {
    query: (p) => m.query(p),
    askUserServer: (question) =>
      m.createSdkMcpServer({
        name: "cockpit",
        version: "1.0.0",
        tools: [
          m.tool(
            "ask_user",
            "Stellt dem Nutzer eine Frage und wartet auf seine Antwort. Nur für Dinge, die du aus dem Code nicht klären kannst.",
            { question: z.string(), suggestions: z.array(z.string()).optional() },
            async (a: { question: string; suggestions?: string[] }) => ({
              content: [{ type: "text", text: await question(a.question, a.suggestions ?? []) }],
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
    async run(job, d) {
      let loaded: Sdk;
      try {
        loaded = await sdk();
      } catch (e) {
        d.sink({ t: "error", text: `Das Agent SDK lässt sich nicht laden (${(e as Error).message}). Image neu bauen?` });
        return 2;
      }
      return run(job, { ...d, sdk: loaded });
    },
  };
}
