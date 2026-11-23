// The adapters and their selection via `COCKPIT_PROVIDER` (default: claude-agent-sdk).
//
// Every adapter translates the output of its tool into the events from event.ts; see apps/agent/README.md.

import { claudeAgentSdk } from "./claude-agent-sdk.ts";
import { claudeCode } from "./claude-code.ts";
import { codex } from "./codex.ts";
import type { Adapter } from "./interface.ts";

export const DEFAULT_PROVIDER = "claude-agent-sdk";

const ADAPTERS: Record<string, () => Adapter> = {
  [DEFAULT_PROVIDER]: () => claudeAgentSdk(),
  "claude-code": () => claudeCode(),
  codex: () => codex(),
};

/** Chooses the adapter; unknown names give an error in plain text. */
export function chooseAdapter(name: string | undefined, adapter: Record<string, () => Adapter> = ADAPTERS): Adapter {
  const n = (name ?? "").trim() || DEFAULT_PROVIDER;
  const build = adapter[n];
  if (build) return build();
  throw new Error(`The provider “${n}” is unknown. Possible: ${Object.keys(adapter).join(", ")}.`);
}
