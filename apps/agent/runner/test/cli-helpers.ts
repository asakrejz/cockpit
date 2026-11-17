// Helpers for the CLI adapter tests: a real Git repo in a temp folder and fake `claude` / `codex` programs that
// print prepared lines. They call nothing and need no network.

import { execFileSync } from "node:child_process";
import { chmodSync, mkdirSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { gitRun } from "../src/git.ts";
import type { AdapterEnv } from "../src/adapter/interface.ts";
import { collector, input } from "./helpers.ts";

export interface Scenario {
  /** Files written into the working directory before the output: path -> content. */
  write?: Record<string, string>;
  /** Commit everything the fake wrote itself (an agent that runs `git commit`). */
  commit?: boolean;
  /** Raw lines for stdout. */
  lines?: string[];
  /** Lines for stderr. */
  stderr?: string[];
  exit?: number;
  /** Sleep before the output (ms), for the timeout. */
  sleep?: number;
  /** Tool calls that the fake (as Claude Code does) passes to the hook from the `--settings` file. */
  hook?: Array<{ tool_name: string; tool_input: Record<string, unknown> }>;
}

export interface Seen {
  args: string[];
  env: Record<string, string>;
  stdin: string;
  cwd: string;
  hook: Array<{ call: unknown; answer: string }>;
}

const FAKE = (node: string) => `#!${node}
const fs = require("node:fs");
const path = require("node:path");
const cp = require("node:child_process");
const here = path.dirname(fs.realpathSync(process.argv[1]));
const sc = JSON.parse(fs.readFileSync(path.join(here, "scenario.json"), "utf8"));
const stdin = fs.readFileSync(0, "utf8");
const seen = { args: process.argv.slice(2), env: process.env, stdin, cwd: process.cwd(), hook: [] };
const i = seen.args.indexOf("--settings");
if (i >= 0 && sc.hook) {
  const settings = JSON.parse(fs.readFileSync(seen.args[i + 1], "utf8"));
  const command = settings.hooks.PreToolUse[0].hooks[0].command;
  for (const call of sc.hook) {
    const r = cp.spawnSync("/bin/sh", ["-c", command], { input: JSON.stringify(call), encoding: "utf8" });
    seen.hook.push({ call, answer: r.stdout });
  }
}
for (const [p, c] of Object.entries(sc.write || {})) {
  fs.mkdirSync(path.dirname(path.join(process.cwd(), p)), { recursive: true });
  fs.writeFileSync(path.join(process.cwd(), p), c);
}
if (sc.commit) {
  cp.execFileSync("git", ["add", "-A"]);
  cp.execFileSync("git", ["-c", "user.name=x", "-c", "user.email=x@x.invalid", "commit", "-q", "-m", "agent"]);
}
fs.writeFileSync(path.join(here, "seen.json"), JSON.stringify(seen));
setTimeout(() => {
  for (const l of sc.stderr || []) process.stderr.write(l + "\\n");
  for (const l of sc.lines || []) process.stdout.write(l + "\\n");
  process.exit(sc.exit || 0);
}, sc.sleep || 0);
`;

/** A folder with fake programs. `install` writes one program and its scenario. */
export function fakeBin() {
  const dir = mkdtempSync(join(tmpdir(), "cockpit-fake-"));
  return {
    dir,
    install(name: "claude" | "codex", scenario: Scenario) {
      writeFileSync(join(dir, name), FAKE(process.execPath));
      chmodSync(join(dir, name), 0o755);
      writeFileSync(join(dir, "scenario.json"), JSON.stringify(scenario));
    },
    seen(): Seen {
      return JSON.parse(readFileSync(join(dir, "seen.json"), "utf8"));
    },
  };
}

/** A clone with one commit on the branch of the job. */
export function repo(branch = "claude/BAU-142-fokus-ring"): string {
  const root = mkdtempSync(join(tmpdir(), "cockpit-repo-"));
  const g = (...a: string[]) => execFileSync("git", a, { cwd: root, stdio: "pipe" });
  g("init", "-q", "-b", "main");
  mkdirSync(join(root, "src"));
  writeFileSync(join(root, "src", "a.txt"), "a\n");
  g("add", "-A");
  g("-c", "user.name=t", "-c", "user.email=t@t.invalid", "commit", "-q", "-m", "init");
  g("checkout", "-q", "-b", branch);
  return root;
}

export function commits(root: string): number {
  return Number(execFileSync("git", ["rev-list", "--count", "HEAD"], { cwd: root }).toString().trim());
}

export const TOKEN = "sk-test-geheim-0123456789";

/** The environment of the container as the backend sets it, plus the fake programs in front in PATH. */
export function adapterEnv(bin: string, root: string, env: Record<string, string | undefined>) {
  const s = collector();
  const d: AdapterEnv = {
    sink: s.sink,
    git: gitRun,
    input: input(),
    root,
    now: () => Date.now(),
    env: { PATH: `${bin}:${process.env.PATH}`, HOME: tmpdir(), ...env },
  };
  return { s, d };
}
