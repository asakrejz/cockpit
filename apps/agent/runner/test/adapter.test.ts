import assert from "node:assert/strict";
import { test } from "node:test";
import { NOCH_NICHT_UMGESETZT, STANDARD_ANBIETER, waehleAdapter } from "../src/adapter/index.ts";
import { claudeAgentSdk } from "../src/adapter/claude-agent-sdk.ts";
import { auftrag, eingang, sammler } from "./hilfen.ts";

test("ohne Angabe gilt das Claude Agent SDK", () => {
  assert.equal(STANDARD_ANBIETER, "claude-agent-sdk");
  assert.equal(waehleAdapter(undefined).name, "claude-agent-sdk");
  assert.equal(waehleAdapter("  ").name, "claude-agent-sdk");
  assert.equal(waehleAdapter("claude-agent-sdk").name, "claude-agent-sdk");
});

test("Claude Code und Codex sind bekannt, aber noch nicht umgesetzt", () => {
  assert.deepEqual([...NOCH_NICHT_UMGESETZT], ["claude-code", "codex"]);
  for (const n of NOCH_NICHT_UMGESETZT) {
    assert.throws(() => waehleAdapter(n), (e: Error) => e.message.includes(n) && e.message.includes("noch nicht umgesetzt"));
  }
  assert.throws(() => waehleAdapter("gpt"), /unbekannt/);
});

test("ein SDK, das sich nicht laden lässt, ergibt ein fehler-Ereignis und Code 2", async () => {
  const s = sammler();
  const adapter = claudeAgentSdk(async () => {
    throw new Error("Modul fehlt");
  });
  const code = await adapter.ausfuehren(auftrag(), {
    senke: s.senke,
    git: async () => "",
    eingang: eingang(),
    wurzel: "/work",
    jetzt: () => 0,
    umgebung: {},
  });
  assert.equal(code, 2);
  assert.equal(s.e[0].t, "fehler");
  assert.match((s.e[0] as any).text, /Modul fehlt/);
});
