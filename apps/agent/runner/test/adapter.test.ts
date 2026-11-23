import assert from "node:assert/strict";
import { test } from "node:test";
import { DEFAULT_PROVIDER, chooseAdapter } from "../src/adapter/index.ts";
import { claudeAgentSdk } from "../src/adapter/claude-agent-sdk.ts";
import { job, input, collector } from "./helpers.ts";

test("without a value the Claude Agent SDK applies", () => {
  assert.equal(DEFAULT_PROVIDER, "claude-agent-sdk");
  assert.equal(chooseAdapter(undefined).name, "claude-agent-sdk");
  assert.equal(chooseAdapter("  ").name, "claude-agent-sdk");
  assert.equal(chooseAdapter("claude-agent-sdk").name, "claude-agent-sdk");
});

test("all three tools have an adapter, unknown names give a plain error", () => {
  assert.equal(chooseAdapter("claude-code").name, "claude-code");
  assert.equal(chooseAdapter("codex").name, "codex");
  assert.throws(() => chooseAdapter("gpt"), (e: Error) => e.message.includes("unknown") && e.message.includes("codex"));
});

test("an SDK that cannot be loaded gives an error event and exit code 2", async () => {
  const s = collector();
  const adapter = claudeAgentSdk(async () => {
    throw new Error("Modul fehlt");
  });
  const code = await adapter.run(job(), {
    sink: s.sink,
    git: async () => "",
    input: input(),
    root: "/work",
    now: () => 0,
    env: {},
  });
  assert.equal(code, 2);
  assert.equal(s.e[0].t, "error");
  assert.match((s.e[0] as any).text, /Modul fehlt/);
});
