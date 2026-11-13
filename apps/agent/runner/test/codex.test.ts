import assert from "node:assert/strict";
import { test } from "node:test";
import { codex, codexArgs, codexBase, codexEnv } from "../src/adapter/codex.ts";
import { job } from "./helpers.ts";
import { TOKEN, adapterEnv, commits, fakeBin, repo } from "./cli-helpers.ts";

const ENV = { OPENAI_BASE_URL: "http://cockpit-host:4000", OPENAI_API_KEY: TOKEN, COCKPIT_MODEL: "gpt-test" };
const j = (line: unknown) => JSON.stringify(line);
const item = (it: Record<string, unknown>) => j({ type: "item.completed", item: it });
const done = j({ type: "turn.completed", usage: { input_tokens: 500, cached_input_tokens: 100, output_tokens: 40 } });

test("success: messages, commands, file changes and usage become events", async () => {
  const bin = fakeBin();
  const root = repo();
  bin.install("codex", {
    write: { "src/b.txt": "b\n" },
    lines: [
      j({ type: "thread.started", thread_id: "t1" }),
      j({ type: "turn.started" }),
      item({ id: "i0", type: "reasoning", text: "denke" }),
      item({ id: "i1", type: "command_execution", command: "ls src", aggregated_output: "a.txt", exit_code: 0, status: "completed" }),
      item({ id: "i2", type: "file_change", changes: [{ path: `${root}/src/b.txt`, kind: "add" }], status: "completed" }),
      item({ id: "i3", type: "agent_message", text: `Fertig, Key ${TOKEN}.` }),
      done,
    ],
  });
  const { s, d } = adapterEnv(bin.dir, root, ENV);
  assert.equal(await codex().run(job(), d), 0);
  assert.deepEqual(s.e.map((e) => e.t), ["step", "step", "files", "step", "files", "result"]);
  assert.equal((s.e[0] as any).title, "Befehl ausführen");
  assert.equal((s.e[0] as any).text, "ls src");
  assert.deepEqual((s.e[2] as any).files, ["src/b.txt"]);
  assert.deepEqual([(s.e[3] as any).title, (s.e[3] as any).tokens_in, (s.e[3] as any).tokens_out], ["Überlegt", 500, 40]);
  assert.match((s.e[5] as any).commit, /^[0-9a-f]{40}$/);
  assert.equal(commits(root), 2);
  assert.ok(!JSON.stringify(s.e).includes(TOKEN));
});

test("the call: sandbox without network, never asks, key only in the environment", async () => {
  const bin = fakeBin();
  const root = repo();
  bin.install("codex", { write: { "src/b.txt": "b" }, lines: [item({ type: "agent_message", text: "ok" }), done] });
  const { d } = adapterEnv(bin.dir, root, { ...ENV, ANTHROPIC_AUTH_TOKEN: "sk-ant-fremd-123456", ANTHROPIC_API_KEY: "sk-ant-fremd-123456", RANDOM_SECRET: "zzz" });
  await codex().run(job({ repo: "firma/web" }), d);
  const seen = bin.seen();
  const a = seen.args.join("\n");
  assert.ok(!a.includes(TOKEN) && !a.includes("sk-"), "no key in the arguments");
  assert.deepEqual(seen.args.slice(0, 2), ["exec", "--json"]);
  assert.equal(seen.args[seen.args.indexOf("--sandbox") + 1], "workspace-write");
  assert.ok(seen.args.includes('approval_policy="never"'));
  assert.ok(seen.args.includes("sandbox_workspace_write.network_access=false"));
  assert.ok(seen.args.includes('model_providers.cockpit.base_url="http://cockpit-host:4000/v1"'));
  assert.ok(seen.args.includes('model_providers.cockpit.env_key="OPENAI_API_KEY"'));
  assert.ok(!/danger|bypass/i.test(a));
  assert.equal(seen.args[seen.args.indexOf("-m") + 1], "gpt-test");
  assert.equal(seen.args.at(-1), "-");
  assert.match(seen.stdin, /BAU-142/);
  assert.match(seen.stdin, /pushst nicht/);
  assert.match(seen.stdin, /firma\/web/);
  assert.equal(seen.env.OPENAI_API_KEY, TOKEN);
  assert.match(seen.env.CODEX_HOME, /cockpit-codex-/);
  for (const gone of ["ANTHROPIC_AUTH_TOKEN", "ANTHROPIC_API_KEY", "RANDOM_SECRET", "OPENAI_BASE_URL", "COCKPIT_MODEL"]) {
    assert.equal(seen.env[gone], undefined, `${gone} is removed`);
  }
});

test("plan mode runs read-only", async () => {
  const bin = fakeBin();
  const root = repo();
  const plan = '```json\n{"summary":"s","files":["src/a.txt"],"steps":["x"]}\n```';
  bin.install("codex", { lines: [item({ type: "agent_message", text: plan }), done] });
  const { s, d } = adapterEnv(bin.dir, root, ENV);
  assert.equal(await codex().run(job({ mode: "plan" }), d), 0);
  const a = bin.seen().args;
  assert.equal(a[a.indexOf("--sandbox") + 1], "read-only");
  assert.equal((s.e.at(-1) as any).t, "plan");
});

test("turn.failed becomes an error event", async () => {
  const bin = fakeBin();
  bin.install("codex", { lines: [j({ type: "turn.started" }), j({ type: "turn.failed", error: { message: "Model not found" } })], exit: 1 });
  const { s, d } = adapterEnv(bin.dir, repo(), ENV);
  assert.equal(await codex().run(job(), d), 1);
  assert.deepEqual(s.e, [{ t: "error", text: "Model not found" }]);
});

test("an error event without a completed turn is the reason, retries before a result are not", async () => {
  const bin = fakeBin();
  const root = repo();
  bin.install("codex", { lines: [j({ type: "error", message: "Reconnecting... 2/5" }), j({ type: "error", message: "stream closed" })], exit: 1 });
  const a = adapterEnv(bin.dir, root, ENV);
  assert.equal(await codex().run(job(), a.d), 1);
  assert.equal((a.s.e.at(-1) as any).text, "stream closed");
  bin.install("codex", { write: { "src/b.txt": "b" }, lines: [j({ type: "error", message: "Reconnecting... 1/5" }), item({ type: "agent_message", text: "ok" }), done] });
  const b = adapterEnv(bin.dir, root, ENV);
  assert.equal(await codex().run(job(), b.d), 0);
});

test("broken line and cut-off output", async () => {
  const bin = fakeBin();
  const root = repo();
  bin.install("codex", { write: { "src/b.txt": "b" }, lines: ["nicht json", item({ type: "agent_message", text: "halb" }), '{"type":"turn.comp'] });
  const { s, d } = adapterEnv(bin.dir, root, ENV);
  assert.equal(await codex().run(job(), d), 1);
  assert.match((s.e.at(-1) as any).text, /ohne Ergebnis.*2 unlesbare Zeilen/);
});

test("abort by the time limit", async () => {
  const bin = fakeBin();
  bin.install("codex", { sleep: 10_000, lines: [done] });
  const { s, d } = adapterEnv(bin.dir, repo(), ENV);
  assert.equal(await codex().run(job({ max_seconds: 1 }), d), 1);
  assert.match((s.e.at(-1) as any).text, /Zeitlimit/);
});

test("files against the rules are rejected after the run (Codex cannot enforce them itself)", async () => {
  const bin = fakeBin();
  const root = repo();
  bin.install("codex", {
    write: { ".github/workflows/x.yml": "on: push", ".git/hooks/pre-commit": "#!/bin/sh", "src/b.txt": "b" },
    lines: [item({ type: "file_change", changes: [{ path: ".github/workflows/x.yml", kind: "add" }] }), item({ type: "agent_message", text: "ok" }), done],
  });
  const { s, d } = adapterEnv(bin.dir, root, ENV);
  assert.equal(await codex().run(job({ rules: { forbidden_paths: [".github/"] } }), d), 1);
  const e = s.e.at(-1) as any;
  assert.match(e.text, /verstoßen/);
  assert.match(e.text, /\.github\/workflows\/x\.yml/);
  assert.equal(commits(root), 1);
  assert.ok(!s.e.some((x) => x.t === "result"));
});

test("base address and environment helpers", () => {
  assert.equal(codexBase({}), "https://api.openai.com/v1");
  assert.equal(codexBase({ OPENAI_BASE_URL: "http://h:4000/" }), "http://h:4000/v1");
  assert.equal(codexBase({ OPENAI_BASE_URL: "http://h:4000/v1" }), "http://h:4000/v1");
  assert.equal(codexEnv({ PATH: "/bin", OPENAI_API_KEY: "k" }, "/h").CODEX_HOME, "/h");
  assert.ok(codexArgs({ model: "m", plan: true, root: "/work", base: "b" }).includes("read-only"));
});

test("missing program, model and key", async () => {
  const a = adapterEnv("/nicht/da", repo(), ENV);
  assert.equal(await codex().run(job(), a.d), 2);
  assert.match((a.s.e[0] as any).text, /Codex ist im Image nicht vorhanden/);
  const b = adapterEnv("/nicht/da", repo(), { OPENAI_API_KEY: TOKEN });
  await codex().run(job(), b.d);
  assert.match((b.s.e[0] as any).text, /COCKPIT_MODEL fehlt/);
  const c = adapterEnv("/nicht/da", repo(), { COCKPIT_MODEL: "m" });
  await codex().run(job(), c.d);
  assert.match((c.s.e[0] as any).text, /OPENAI_API_KEY fehlt/);
});
