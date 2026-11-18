// Translation of Claude messages (assistant messages with tool calls and usage, final result) into the events of
// the runner. The Claude Agent SDK and the Claude Code CLI (`--output-format stream-json`) use the same message
// shapes, so both adapters use this one translator.

import type { Sink } from "../event.ts";
import { truncate } from "../event.ts";
import { tx } from "../texts.ts";

/** A message (only the fields we read). */
export interface SdkMessage {
  type: string;
  subtype?: string;
  message?: { id?: string; usage?: Record<string, number>; content?: Array<Record<string, any>> };
  result?: string;
  is_error?: boolean;
  errors?: string[];
}

/** Short sentence about a tool call, for the history in Cockpit. */
export function describe(name: string, input: Record<string, any>): { title: string; text: string } {
  const path = String(input.file_path ?? input.path ?? input.pattern ?? "");
  switch (name) {
    case "Read": return { title: tx("step_read"), text: path };
    case "Write": return { title: tx("step_write"), text: path };
    case "Edit":
    case "MultiEdit": return { title: tx("step_change"), text: path };
    case "Glob":
    case "Grep": return { title: tx("step_search"), text: path };
    case "Bash": return { title: tx("step_run"), text: String(input.command ?? "") };
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

/** Turns messages into events and remembers the final answer and a possible error. */
export class ClaudeTranslator {
  endText = "";
  error: string | null = null;
  /** A `result` message arrived. */
  finished = false;
  private readonly changed = new Set<string>();
  private readonly seen = new Set<string>();
  private lastStart: number;
  private readonly sink: Sink;
  private readonly root: string;
  private readonly now: () => number;

  constructor(sink: Sink, root: string, now: () => number) {
    this.sink = sink;
    this.root = root;
    this.now = now;
    this.lastStart = now();
  }

  /** Time waiting for the user does not count as run time. */
  restartClock(): void {
    this.lastStart = this.now();
  }

  /** Files the agent wrote so far (as seen in tool calls). */
  files(): string[] {
    return [...this.changed];
  }

  handle(m: SdkMessage): void {
    if (m.type === "assistant" && m.message) {
      const id = m.message.id ?? "";
      const { ein, aus } = id && this.seen.has(id) ? { ein: 0, aus: 0 } : usageFrom(m);
      if (id) this.seen.add(id);
      const now = this.now();
      const ms = Math.max(0, now - this.lastStart);
      this.lastStart = now;
      let first = true;
      for (const b of m.message.content ?? []) {
        if (b.type === "tool_use") {
          const { title, text } = describe(String(b.name), b.input ?? {});
          this.sink({ t: "step", title, text, tokens_in: first ? ein : 0, tokens_out: first ? aus : 0, ms: first ? ms : 0 });
          first = false;
          if (Writer.has(String(b.name))) {
            const p = String(b.input?.file_path ?? "");
            const rel = p.startsWith(`${this.root}/`) ? p.slice(this.root.length + 1) : p;
            if (rel && !this.changed.has(rel)) {
              this.changed.add(rel);
              this.sink({ t: "files", files: [...this.changed] });
            }
          }
        } else if (b.type === "text" && String(b.text ?? "").trim()) {
          this.endText = String(b.text);
        }
      }
      if (first && (ein > 0 || aus > 0)) {
        this.sink({ t: "step", title: tx("step_think"), text: "", tokens_in: ein, tokens_out: aus, ms });
      }
    } else if (m.type === "result") {
      this.finished = true;
      if (m.subtype === "success" && !m.is_error) this.endText = m.result ?? this.endText;
      else this.error = (m.errors ?? []).join("; ") || tx("error_model_ended", { reason: m.subtype ?? "error" });
    }
  }
}
