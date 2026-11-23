// Adapter "claude-agent-sdk": the course of a run with the Claude Agent SDK. The SDK, the lines from stdin, Git and the clock come from outside
// (`Dependencies`), so that the course can be tested without network and without the SDK.

import { type Job, tasksText, systemText, commitMessage } from "../job.ts";
import { type Sink, truncate } from "../event.ts";
import { tx } from "../texts.ts";
import { type Run, recordJob } from "../git.ts";
import { type Policy, decide, policyOf } from "../policy.ts";
import type { Adapter, Input } from "./interface.ts";
import { finishReview } from "../review.ts";
import { ClaudeTranslator, type SdkMessage, describe, planFromAnswer } from "./claude-messages.ts";

export { describe, planFromAnswer };
export type { SdkMessage };

export type { Input };

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

/** Runs the job. Returns the exit code of the process (0 = a result was reported). */
export async function run(a: Job, d: Dependencies): Promise<number> {
  const model = d.env.ANTHROPIC_MODEL ?? "";
  if (!model) {
    d.sink({ t: "error", text: "No model is set (ANTHROPIC_MODEL is missing)." });
    return 1;
  }
  const review = a.mode === "review";
  // Plan and review only read.
  const plan = a.mode === "plan" || review;
  const input = new Queue<unknown>();
  const cancel = new AbortController();
  const deadline = setTimeout(() => cancel.abort(), a.max_seconds * 1000);
  const policy: Policy = policyOf(d.root, a);
  const tr = new ClaudeTranslator(d.sink, d.root, d.now);

  const answers = new Queue<string>();
  // Hints from the user: as a further message to the running agent.
  const hintPump = (async () => {
    // Answers are fetched by the tool "ask_user" itself; only hints are passed on here.
    for (;;) {
      const z = await d.input.next();
      if (!z) return;
      if (z.t === "hint") {
        input.push(user_message(`Hint from the user: ${z.text}`));
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
    tr.restartClock(); // Time waiting for the user does not count as run time
    return r.done ? "(no answer)" : r.value;
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
      tr.handle(m);
      if (m.type === "result") input.close();
    }
    endText = tr.endText;
    error = tr.error;
  } catch (e) {
    error = cancel.signal.aborted
      ? tx("error_timeout", { minutes: Math.round(a.max_seconds / 60) })
      : tx("error_sdk", { message: (e as Error).message });
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
  if (review) {
    finishReview(d.sink, a, endText);
    return 0;
  }
  if (plan) {
    const p = planFromAnswer(endText);
    d.sink({ t: "plan", summary: p.summary, files: p.files, steps: p.steps });
    return 0;
  }
  try {
    const f = await recordJob(d.git, d.root, a);
    if (!f) {
      d.sink({ t: "error", text: tx("error_nothing_changed") });
      return 1;
    }
    d.sink({ t: "files", files: f.files });
    d.sink({
      t: "result",
      text: truncate(endText.trim() || tx("result_done", { key: a.ticket.key }), 1500),
      branch: a.branch,
      commit: f.commit,
      ...(a.repos.length > 1 ? { repos: f.repos } : {}),
    });
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
            "Asks the user a question and waits for the answer. Only for things you cannot clarify from the code.",
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
        d.sink({ t: "error", text: tx("error_sdk_load", { message: (e as Error).message }) });
        return 2;
      }
      return run(job, { ...d, sdk: loaded });
    },
  };
}
