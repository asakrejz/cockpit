// PreToolUse hook for the Claude Code CLI (adapter `claude-code`). Claude Code starts it before every tool call,
// passes the call as JSON on stdin and reads the decision from stdout. The decision is the same `decide()` as for
// the Claude Agent SDK, with the rules of the job from a policy file (first argument, written by the adapter
// outside of /work).
//
// Fails closed: whatever goes wrong (unreadable policy, broken input), the tool call is denied. Exit code 0 and no
// output would mean "no objection", so every error path prints a deny.

import { readFileSync } from "node:fs";
import { pathToFileURL } from "node:url";
import { type Policy, decide } from "./policy.ts";

function deny(reason: string): string {
  return JSON.stringify({
    hookSpecificOutput: { hookEventName: "PreToolUse", permissionDecision: "deny", permissionDecisionReason: reason },
  });
}

/** The output for one hook call: `""` = no objection (the normal permission rules of the CLI still apply). */
export function hookAnswer(policy: Policy, raw: string): string {
  let call: { tool_name?: unknown; tool_input?: unknown };
  try {
    call = JSON.parse(raw);
  } catch {
    return deny("Der Aufruf des Werkzeugs ist nicht lesbar.");
  }
  if (typeof call.tool_name !== "string") return deny("Der Aufruf nennt kein Werkzeug.");
  const input = typeof call.tool_input === "object" && call.tool_input !== null ? (call.tool_input as Record<string, unknown>) : {};
  const d = decide(policy, call.tool_name, input);
  return d.behavior === "allow" ? "" : deny(d.message ?? "Nicht erlaubt.");
}

async function main(): Promise<void> {
  let out: string;
  try {
    const policy = JSON.parse(readFileSync(process.argv[2] ?? "", "utf8")) as Policy;
    if (typeof policy.root !== "string" || !Array.isArray(policy.allowed) || !Array.isArray(policy.forbidden)) throw new Error("policy");
    const chunks: Buffer[] = [];
    for await (const c of process.stdin) chunks.push(c as Buffer);
    out = hookAnswer(policy, Buffer.concat(chunks).toString("utf8"));
  } catch {
    out = deny("Die Richtlinie des Cockpits ist nicht lesbar; aus Sicherheitsgründen wird nichts ausgeführt.");
  }
  if (out) process.stdout.write(`${out}\n`);
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) await main();
