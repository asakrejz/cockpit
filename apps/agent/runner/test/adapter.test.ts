import assert from "node:assert/strict";
import { test } from "node:test";
import { STILL_NOT_IMPLEMENTED, DEFAULT_PROVIDER, chooseAdapter } from "../src/adapter/index.ts";
import { claudeAgentSdk } from "../src/adapter/claude-agent-sdk.ts";
import { job, input, collector } from "./hilfen.ts";

test("ohne Angabe gilt das Claude Agent SDK", () => {
  assert.equal(DEFAULT_PROVIDER, "claude-agent-sdk");
  assert.equal(chooseAdapter(undefined).name, "claude-agent-sdk");
  assert.equal(chooseAdapter("  ").name, "claude-agent-sdk");
  assert.equal(chooseAdapter("claude-agent-sdk").name, "claude-agent-sdk");
});

test("Claude Code und Codex sind bekannt, aber noch nicht umgesetzt", () => {
  assert.deepEqual([...STILL_NOT_IMPLEMENTED], ["claude-code", "codex"]);
  for (const n of STILL_NOT_IMPLEMENTED) {
    assert.throws(() => chooseAdapter(n), (e: Error) => e.message.includes(n) && e.message.includes("noch nicht umgesetzt"));
  }
  assert.throws(() => chooseAdapter("gpt"), /unknown/);
});

test("ein SDK, das sich nicht laden lässt, ergibt ein fehler-Ereignis und Code 2", async () => {
  const s = collector();
  const adapter = claudeAgentSdk(async () => {
    throw new Error("Modul fehlt");
  });
  const code = await adapter.run(job(), {
    sink: s.sink,
    git: async () => "",
    input: input(),
    wurzel: "/work",
    now: () => 0,
    env: {},
  });
  assert.equal(code, 2);
  assert.equal(s.e[0].t, "error");
  assert.match((s.e[0] as any).text, /Module missing/);
});
