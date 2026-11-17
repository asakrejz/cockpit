// Adapter "claude-code": runs the Claude Code CLI headless (`claude -p --output-format stream-json`) and translates
// its message lines into the events of the runner.
//
// Rules for the CLI, all defined here in one place:
//  * Tools: only the listed ones, web tools denied, never a permission mode that skips the checks.
//  * A PreToolUse hook (src/hook.ts) asks the same `decide()` as the SDK adapter before every tool call: allowed and
//    forbidden paths, `.git`, no push or network commands, no reading of the keys.
//  * Settings of the repo and the user are not loaded (`--setting-sources user`; the image has no user settings),
//    MCP servers are ignored (`--strict-mcp-config`).
//  * After the run the changed files are checked again (cli.ts).
// Endpoint: `ANTHROPIC_BASE_URL` + `ANTHROPIC_AUTH_TOKEN` (proxy) or `ANTHROPIC_API_KEY` (directly at Anthropic).

import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { type Job, systemText, tasksText } from "../job.ts";
import type { Policy } from "../policy.ts";
import { type Outcome, type Spec, type Translator, baseEnv, runCli } from "./cli.ts";
import { ClaudeTranslator } from "./claude-messages.ts";
import type { Adapter } from "./interface.ts";

/** The central definition of the CLI call. Nothing here comes from the job or the environment. */
export const CLAUDE = {
  command: "claude",
  readTools: ["Read", "Glob", "Grep"],
  writeTools: ["Write", "Edit", "MultiEdit", "Bash", "TodoWrite"],
  /** Never available, even if a setting would allow them: no network access for the agent. */
  deniedTools: ["WebFetch", "WebSearch"],
  maxTurns: { plan: 25, implement: 120 },
  permissionMode: "default",
  settingSources: "user",
} as const;

/** The arguments of `claude`. The prompt is not among them (stdin), neither is a key. */
export function claudeArgs(o: { model: string; plan: boolean; system: string; settings: string }): string[] {
  return [
    "-p",
    "--output-format", "stream-json",
    "--verbose",
    "--model", o.model,
    "--append-system-prompt", o.system,
    "--permission-mode", CLAUDE.permissionMode,
    "--allowedTools", (o.plan ? CLAUDE.readTools : [...CLAUDE.readTools, ...CLAUDE.writeTools]).join(","),
    "--disallowedTools", CLAUDE.deniedTools.join(","),
    "--max-turns", String(o.plan ? CLAUDE.maxTurns.plan : CLAUDE.maxTurns.implement),
    "--settings", o.settings,
    "--setting-sources", CLAUDE.settingSources,
    "--strict-mcp-config",
  ];
}

/** The `--settings` file: the hook that checks every tool call, and the denied tools again as a permission rule. */
export function claudeSettings(hookCommand: string): Record<string, unknown> {
  return {
    hooks: { PreToolUse: [{ matcher: "*", hooks: [{ type: "command", command: hookCommand }] }] },
    permissions: { deny: [...CLAUDE.deniedTools], defaultMode: CLAUDE.permissionMode },
  };
}

/** The environment of the CLI: the endpoint values that belong to the mode, nothing else from the container. */
export function claudeEnv(env: Record<string, string | undefined>, model: string): Record<string, string> {
  const out = baseEnv(env);
  const proxy = (env.ANTHROPIC_BASE_URL ?? "").trim();
  if (proxy) {
    out.ANTHROPIC_BASE_URL = proxy;
    if (env.ANTHROPIC_AUTH_TOKEN) out.ANTHROPIC_AUTH_TOKEN = env.ANTHROPIC_AUTH_TOKEN;
  } else if (env.ANTHROPIC_API_KEY) {
    out.ANTHROPIC_API_KEY = env.ANTHROPIC_API_KEY;
  }
  return {
    ...out,
    ANTHROPIC_MODEL: model,
    ANTHROPIC_SMALL_FAST_MODEL: model,
    ANTHROPIC_DEFAULT_HAIKU_MODEL: model,
    ANTHROPIC_DEFAULT_SONNET_MODEL: model,
    ANTHROPIC_DEFAULT_OPUS_MODEL: model,
    CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC: "1",
    DISABLE_AUTOUPDATER: "1",
    DISABLE_TELEMETRY: "1",
  };
}

/** The translator for `stream-json` lines: same shapes as the SDK messages. */
class Lines implements Translator {
  private readonly t: ClaudeTranslator;
  constructor(t: ClaudeTranslator) {
    this.t = t;
  }
  line(v: Record<string, any>): void {
    this.t.handle(v as never);
  }
  outcome(): Outcome {
    return { text: this.t.endText, error: this.t.error, finished: this.t.finished, files: this.t.files() };
  }
}

/** The adapter for `COCKPIT_PROVIDER=claude-code`. */
export function claudeCode(): Adapter {
  return {
    name: "claude-code",
    run(job: Job, d) {
      return runCli(job, d, (sink): Spec | string => {
        const model = d.env.ANTHROPIC_MODEL ?? "";
        if (!model) return "Es ist kein Modell eingestellt (ANTHROPIC_MODEL fehlt).";
        const plan = job.mode === "plan";
        const dir = mkdtempSync(join(tmpdir(), "cockpit-claude-"));
        const policy: Policy = { root: d.root, allowed: job.rules.allowed_paths, forbidden: job.rules.forbidden_paths };
        const policyFile = join(dir, "policy.json");
        const settingsFile = join(dir, "settings.json");
        writeFileSync(policyFile, JSON.stringify(policy));
        const hook = fileURLToPath(new URL("../hook.ts", import.meta.url));
        writeFileSync(
          settingsFile,
          JSON.stringify(claudeSettings(`'${process.execPath}' --disable-warning=ExperimentalWarning '${hook}' '${policyFile}'`)),
        );
        return {
          label: "Claude Code",
          command: CLAUDE.command,
          args: claudeArgs({ model, plan, system: systemText(job), settings: settingsFile }),
          env: claudeEnv(d.env, model),
          stdin: tasksText(job, { canAsk: false }),
          translator: new Lines(new ClaudeTranslator(sink, d.root, d.now)),
          cleanup: () => rmSync(dir, { recursive: true, force: true }),
        };
      });
    },
  };
}
