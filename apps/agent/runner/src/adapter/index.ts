// The adapters and their selection via `COCKPIT_PROVIDER` (default: claude-agent-sdk).
//
// New adapters (Claude Code with `claude -p --output-format stream-json`, Codex with
// `codex exec --json`) only have to translate the output of their tool into the events from event.ts;
// see apps/agent/README.md.

import { claudeAgentSdk } from "./claude-agent-sdk.ts";
import type { Adapter } from "./interface.ts";

/** Known tools that are not implemented yet. */
export const NOT_IMPLEMENTED_YET = ["claude-code", "codex"] as const;

export const DEFAULT_PROVIDER = "claude-agent-sdk";

/** Chooses the adapter; unknown and not yet implemented names give an error in plain text. */
export function chooseAdapter(name: string | undefined, adapter: Record<string, () => Adapter> = { [DEFAULT_PROVIDER]: () => claudeAgentSdk() }): Adapter {
  const n = (name ?? "").trim() || DEFAULT_PROVIDER;
  const build = adapter[n];
  if (build) return build();
  if ((NOT_IMPLEMENTED_YET as readonly string[]).includes(n)) {
    throw new Error(`Der Anbieter „${n}“ ist noch nicht umgesetzt. Wähle claude-agent-sdk.`);
  }
  throw new Error(`Der Anbieter „${n}“ ist unbekannt. Möglich ist claude-agent-sdk.`);
}
