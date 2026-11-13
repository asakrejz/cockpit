// Die Adapter und ihre Auswahl über `COCKPIT_PROVIDER` (Standard: claude-agent-sdk).
//
// Neue Adapter (Claude Code mit `claude -p --output-format stream-json`, Codex mit
// `codex exec --json`) müssen nur die Ausgabe ihres Werkzeugs in die Ereignisse aus ereignis.ts
// übersetzen; siehe apps/agent/README.md.

import { claudeAgentSdk } from "./claude-agent-sdk.ts";
import type { Adapter } from "./interface.ts";

/** Bekannte, aber noch nicht umgesetzte Werkzeuge. */
export const NOT_IMPLEMENTED_YET = ["claude-code", "codex"] as const;

export const DEFAULT_PROVIDER = "claude-agent-sdk";

/** Wählt den Adapter; unbekannte und noch nicht umgesetzte Namen ergeben einen Fehler in Klartext. */
export function chooseAdapter(name: string | undefined, adapter: Record<string, () => Adapter> = { [DEFAULT_PROVIDER]: () => claudeAgentSdk() }): Adapter {
  const n = (name ?? "").trim() || DEFAULT_PROVIDER;
  const build = adapter[n];
  if (build) return build();
  if ((NOT_IMPLEMENTED_YET as readonly string[]).includes(n)) {
    throw new Error(`Der Anbieter „${n}“ ist noch nicht umgesetzt. Wähle claude-agent-sdk.`);
  }
  throw new Error(`Der Anbieter „${n}“ ist unbekannt. Möglich ist claude-agent-sdk.`);
}
