// Adapter "codex": runs the OpenAI Codex CLI headless (`codex exec --json`) and translates its event lines
// (`item.completed` with messages, commands and file changes, `turn.completed` with usage) into the events of
// the runner.
//
// Rules for the CLI, all defined here in one place:
//  * Sandbox `workspace-write` (plan mode: `read-only`) without network access, approval policy `never`: the tool
//    never asks and can only write inside /work.
//  * Codex has no hook for single paths, so the rules of the job (allowed and forbidden paths, `.git`) are enforced
//    by the runner after the run (cli.ts). The limit is documented in docs/security.md.
//  * The model endpoint is a provider defined per `-c` flag; the key is read by Codex from the environment variable
//    named there (`OPENAI_API_KEY`), it is never an argument.
// Endpoint: `OPENAI_BASE_URL` (proxy or compatible endpoint, `/v1` is added if missing) or directly at OpenAI,
// always `OPENAI_API_KEY` as the bearer key; the model comes in `COCKPIT_MODEL`.

import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { type Job, systemText, tasksText } from "../job.ts";
import { type Outcome, type Spec, type Translator, baseEnv, runCli } from "./cli.ts";
import type { Adapter } from "./interface.ts";
import type { Sink } from "../event.ts";

/** The central definition of the CLI call. */
export const CODEX = {
  command: "codex",
  sandbox: { plan: "read-only", implement: "workspace-write" },
  approvalPolicy: "never",
  /** Name of the provider entry in the Codex configuration and of the variable that holds its key. */
  provider: "cockpit",
  keyVariable: "OPENAI_API_KEY",
  wireApi: "responses",
  direct: "https://api.openai.com/v1",
} as const;

/** The base address Codex needs (with `/v1`). */
export function codexBase(env: Record<string, string | undefined>): string {
  const url = (env.OPENAI_BASE_URL ?? "").trim().replace(/\/+$/, "");
  if (!url) return CODEX.direct;
  return /\/v1$/.test(url) ? url : `${url}/v1`;
}

/** The arguments of `codex`. The prompt comes through stdin (`-`), the key through the environment. */
export function codexArgs(o: { model: string; plan: boolean; root: string; base: string }): string[] {
  const c = (k: string, v: string | boolean) => ["-c", `${k}=${typeof v === "boolean" ? v : JSON.stringify(v)}`];
  const p = `model_providers.${CODEX.provider}`;
  return [
    "exec",
    "--json",
    "--color", "never",
    "--sandbox", o.plan ? CODEX.sandbox.plan : CODEX.sandbox.implement,
    "-C", o.root,
    "-m", o.model,
    ...c("approval_policy", CODEX.approvalPolicy),
    ...c("sandbox_workspace_write.network_access", false),
    ...c("model_provider", CODEX.provider),
    ...c(`${p}.name`, "Cockpit endpoint"),
    ...c(`${p}.base_url`, o.base),
    ...c(`${p}.env_key`, CODEX.keyVariable),
    ...c(`${p}.wire_api`, CODEX.wireApi),
    "-",
  ];
}

/** The environment of the CLI: the key and a private state folder, nothing else from the container. */
export function codexEnv(env: Record<string, string | undefined>, home: string): Record<string, string> {
  const out = baseEnv(env);
  if (env[CODEX.keyVariable]) out[CODEX.keyVariable] = env[CODEX.keyVariable] as string;
  out.CODEX_HOME = home;
  return out;
}

/** Translates the event lines of `codex exec --json`. */
export class CodexLines implements Translator {
  private text = "";
  private error: string | null = null;
  private lastError: string | null = null;
  private finished = false;
  private tokensIn = 0;
  private tokensOut = 0;
  private lastStart: number;
  private readonly changed = new Set<string>();
  private readonly sink: Sink;
  private readonly root: string;
  private readonly now: () => number;

  constructor(sink: Sink, root: string, now: () => number) {
    this.sink = sink;
    this.root = root;
    this.now = now;
    this.lastStart = now();
  }

  private ms(): number {
    const now = this.now();
    const ms = Math.max(0, now - this.lastStart);
    this.lastStart = now;
    return ms;
  }

  private relative(p: string): string {
    return p.startsWith(`${this.root}/`) ? p.slice(this.root.length + 1) : p;
  }

  line(v: Record<string, any>): void {
    switch (v.type) {
      case "item.completed": {
        const item = v.item ?? {};
        switch (item.type) {
          case "agent_message":
            if (String(item.text ?? "").trim()) this.text = String(item.text);
            break;
          case "command_execution":
            this.sink({ t: "step", title: tx("step_run"), text: String(item.command ?? ""), tokens_in: 0, tokens_out: 0, ms: this.ms() });
            break;
          case "file_change": {
            const paths = (Array.isArray(item.changes) ? item.changes : []).map((c: any) => this.relative(String(c?.path ?? ""))).filter(Boolean);
            this.sink({ t: "step", title: tx("step_change"), text: paths.join(", "), tokens_in: 0, tokens_out: 0, ms: this.ms() });
            const before = this.changed.size;
            for (const p of paths) this.changed.add(p);
            if (this.changed.size > before) this.sink({ t: "files", files: [...this.changed] });
            break;
          }
          case "mcp_tool_call":
            this.sink({ t: "step", title: `${String(item.server ?? tx("step_tool"))}: ${String(item.tool ?? "")}`, text: "", tokens_in: 0, tokens_out: 0, ms: this.ms() });
            break;
          case "web_search":
            this.sink({ t: "step", title: tx("step_web"), text: String(item.query ?? ""), tokens_in: 0, tokens_out: 0, ms: this.ms() });
            break;
          default:
            break; // reasoning, todo lists and other items carry nothing for the history
        }
        break;
      }
      case "turn.completed": {
        const u = v.usage ?? {};
        const ein = Number(u.input_tokens ?? 0) || 0;
        const aus = Number(u.output_tokens ?? 0) || 0;
        this.tokensIn += ein;
        this.tokensOut += aus;
        this.finished = true;
        if (ein > 0 || aus > 0) this.sink({ t: "step", title: tx("step_think"), text: "", tokens_in: ein, tokens_out: aus, ms: this.ms() });
        break;
      }
      case "turn.failed":
        this.error = String(v.error?.message ?? "") || tx("error_model_failed");
        this.finished = true;
        break;
      case "error":
        // Also reported for retries; only decisive if the run never completes.
        this.lastError = String(v.message ?? "") || null;
        break;
      default:
        break;
    }
  }

  outcome(): Outcome {
    return { text: this.text, error: this.error ?? (this.finished ? null : this.lastError), finished: this.finished, files: [...this.changed] };
  }
}

/** The adapter for `COCKPIT_PROVIDER=codex`. */
export function codex(): Adapter {
  return {
    name: "codex",
    run(job: Job, d) {
      return runCli(job, d, (sink): Spec | string => {
        const model = (d.env.COCKPIT_MODEL ?? "").trim();
        if (!model) return "Es ist kein Modell eingestellt (COCKPIT_MODEL fehlt).";
        if (!d.env[CODEX.keyVariable]) return "Es ist kein Key eingestellt (OPENAI_API_KEY fehlt).";
        const home = mkdtempSync(join(tmpdir(), "cockpit-codex-"));
        return {
          label: "Codex",
          command: CODEX.command,
          args: codexArgs({ model, plan: job.mode === "plan" || job.mode === "review", root: d.root, base: codexBase(d.env) }),
          env: codexEnv(d.env, home),
          // Codex has no system prompt flag: the fixed rules go in front of the task.
          stdin: `${systemText(job)}\n\n${tasksText(job, { canAsk: false })}`,
          translator: new CodexLines(sink, d.root, d.now),
          cleanup: () => rmSync(home, { recursive: true, force: true }),
        };
      });
    },
  };
}
