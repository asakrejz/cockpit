// Adapter "claude-agent-sdk": the course of a run with the Claude Agent SDK. The SDK, the lines from stdin, Git and the clock come from outside
// (`Dependencies`), so that the course can be tested without network and without the SDK.

import { type Job, tasksText, systemText } from "../job.ts";
import { type Sink, truncate } from "../event.ts";
import { type Run, record } from "../git.ts";
import { type Policy, decide } from "../policy.ts";
import type { Adapter, Input } from "./interface.ts";

export type { Input };

/** A message from the SDK (only the fields we read). */
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
  /** Builds the MCP server with the tool "ask_user". */
  askUserServer(question: (text: string, suggestions: string[]) => Promise<string>): unknown;
}

export interface Dependencies {
  sdk: Sdk;
  sink: Sink;
  git: Run;
  input: Input;
  /** Working directory in the container. */
  root: string;
  now: () => number;
  env: Record<string, string | undefined>;
}

/** A queue that feeds an `AsyncIterable` (for the streaming input of the SDK). */
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

/** Short sentence about a tool call, for the history in Cockpit. */
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

/** Pulls the plan (JSON block) out of the model's answer; without a block the answer becomes the summary. */
export function planFromAnswer(text: string): { summary: string; files: string[]; steps: string[] } {
  const block = /```json\s*([\s\S]*?)```/i.exec(text)?.[1] ?? /(\{[\s\S]*\})/.exec(text)?.[1];
  if (block) {
    try {
      const j = JSON.parse(block);
      const list = (x: unknown) => (Array.isArray(x) ? x.map(String) : []);
      return { summary: String(j.summary ?? ""), files: list(j.files), steps: list(j.steps) };
    } catch {
      /* falls back to the answer below */
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

/** Runs the job. Returns the exit code of the process (0 = a result was reported). */
export async function run(a: Job, d: Dependencies): Promise<number> {
  const model = d.env.ANTHROPIC_MODEL ?? "";
  if (!model) {
    d.sink({ t: "error", text: "Es ist kein Modell eingestellt (ANTHROPIC_MODEL fehlt)." });
    return 1;
  }
  const plan = a.mode === "plan";
  const input = new Queue<unknown>();
  const cancel = new AbortController();
  const deadline = setTimeout(() => cancel.abort(), a.max_seconds * 1000);
  const policy: Policy = {
    root: d.root,
    allowed: a.rules.allowed_paths,
    forbidden: a.rules.forbidden_paths,
  };
  const changed = new Set<string>();
  const seen = new Set<string>();
  let lastStart = d.now();

  const answers = new Queue<string>();
  // Hints from the user: as a further message to the running agent.
  const hintPump = (async () => {
    // Answers are fetched by the tool "ask_user" itself; only hints are passed on here.
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

  const askUser = async (text: string, suggestions: string[]): Promise<string> => {
    d.sink({ t: "question", text, suggestions });
    const it = answers[Symbol.asyncIterator]();
    const r = await it.next();
    lastStart = d.now(); // Time waiting for the user does not count as run time
    return r.done ? "(keine Antwort)" : r.value;
  };

  const options: QueryOptions = {
    cwd: d.root,
    model: model,
    systemPrompt: systemText(a),
    permissionMode: plan ? "plan" : "default",
    allowedTools: plan ? ["Read", "Glob", "Grep"] : ["Read", "Glob", "Grep", "Write", "Edit", "MultiEdit", "Bash", "mcp__cockpit__ask_user"],
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
    ...(plan ? {} : { mcpServers: { cockpit: d.sdk.askUserServer(askUser) } }),
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
              const rel = p.startsWith(`${d.root}/`) ? p.slice(d.root.length + 1) : p;
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
        else error = (m.errors ?? []).join("; ") || `Das Modell hat den Lauf mit „${m.subtype ?? "Fehler"}“ beendet.`;
        input.close();
      }
    }
  } catch (e) {
    error = cancel.signal.aborted
      ? `Das Zeitlimit von ${Math.round(a.max_seconds / 60)} Minuten im Container ist überschritten.`
      : `Das SDK meldet: ${(e as Error).message}`;
  } finally {
    clearTimeout(deadline);
    input.close();
    answers.close();
  }
  void hintPump.catch(() => {});

  if (error) {
    d.sink({ t: "error", text: error });
    return 1;
  }
  if (plan) {
    const p = planFromAnswer(endText);
    d.sink({ t: "plan", summary: p.summary, files: p.files, steps: p.steps });
    return 0;
  }
  try {
    const f = await record(d.git, d.root, a.branch, `${a.ticket.key}: ${a.ticket.title}\n\nUmgesetzt vom Cockpit-Agenten.`);
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

/** Loads the real SDK (only in the container; the tests pass a fake). */
export async function loadSdk(): Promise<Sdk> {
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

/** The adapter for `COCKPIT_PROVIDER=claude-agent-sdk`. `sdk` loads the SDK only when needed. */
export function claudeAgentSdk(sdk: () => Promise<Sdk> = loadSdk): Adapter {
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
