import assert from "node:assert/strict";
import { test } from "node:test";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { claudeArgs, claudeCode, claudeEnv, claudeSettings } from "../src/adapter/claude-code.ts";
import { hookAnswer } from "../src/hook.ts";
import { job } from "./helpers.ts";
import { TOKEN, adapterEnv, commits, fakeBin, repo } from "./cli-helpers.ts";

const PROXY = { ANTHROPIC_BASE_URL: "http://cockpit-host:4000", ANTHROPIC_AUTH_TOKEN: TOKEN, ANTHROPIC_MODEL: "claude-sonnet-5-5" };
const j = (line: unknown) => JSON.stringify(line);
const assistant = (id: string, content: unknown[]) => j({ type: "assistant", message: { id, usage: { input_tokens: 100, output_tokens: 20 }, content } });
const ok = (text: string) => j({ type: "result", subtype: "success", is_error: false, result: text, total_cost_usd: 0.01 });

test("success: lines become steps, files and a result with a commit", async () => {
  const bin = fakeBin();
  const root = repo();
  bin.install("claude", {
    write: { "src/b.txt": "b\n" },
    lines: [
      j({ type: "system", subtype: "init" }),
      assistant("m1", [{ type: "text", text: "Ich lese." }, { type: "tool_use", name: "Read", input: { file_path: "/work/src/a.txt" } }]),
      assistant("m2", [{ type: "tool_use", name: "Write", input: { file_path: `${root}/src/b.txt` } }]),
      ok(`Fertig. Token ${TOKEN} nie ausgeben.`),
    ],
  });
  const { s, d } = adapterEnv(bin.dir, root, { ...PROXY, OPENAI_API_KEY: "sk-fremd-0123456789", SHELL_SECRET: "x" });
  const code = await claudeCode().run(job(), d);
  assert.equal(code, 0);
  const types = s.e.map((e) => e.t);
  assert.deepEqual(types, ["step", "step", "files", "files", "result"]);
  assert.deepEqual(s.e[0], { t: "step", title: "Read file", text: "/work/src/a.txt", tokens_in: 100, tokens_out: 20, ms: (s.e[0] as any).ms });
  assert.deepEqual((s.e[3] as any).files, ["src/b.txt"]);
  const r = s.e[4] as any;
  assert.equal(r.branch, "claude/BAU-142-fokus-ring");
  assert.match(r.commit, /^[0-9a-f]{40}$/);
  assert.equal(commits(root), 2);
  assert.ok(!JSON.stringify(s.e).includes(TOKEN), "the key is scrubbed from every event");
  assert.match(r.text, /\[entfernt\]/);
});

test("the call: no key in arguments, a clean environment, restrictive tools, no bypass mode", async () => {
  const bin = fakeBin();
  const root = repo();
  bin.install("claude", { write: { "src/b.txt": "b\n" }, lines: [ok("fertig")] });
  const { d } = adapterEnv(bin.dir, root, { ...PROXY, ANTHROPIC_API_KEY: "sk-ant-aus-der-shell-123", OPENAI_API_KEY: "sk-fremd-0123456789", COCKPIT_PROVIDER: "claude-code", RANDOM_SECRET: "zzz" });
  await claudeCode().run(job({ repo: "firma/web", rules: { forbidden_paths: [".github/"] } }), d);
  const seen = bin.seen();
  const all = seen.args.join("\n");
  assert.ok(!all.includes(TOKEN) && !all.includes("sk-"), "no key in the arguments");
  assert.ok(!/bypass|dangerously|acceptEdits/i.test(all), "no mode that skips the permission checks");
  assert.deepEqual(seen.args.slice(0, 4), ["-p", "--output-format", "stream-json", "--verbose"]);
  assert.equal(seen.args[seen.args.indexOf("--permission-mode") + 1], "default");
  assert.equal(seen.args[seen.args.indexOf("--disallowedTools") + 1], "WebFetch,WebSearch");
  assert.equal(seen.args[seen.args.indexOf("--setting-sources") + 1], "user");
  assert.ok(seen.args.includes("--strict-mcp-config"));
  assert.equal(seen.args[seen.args.indexOf("--model") + 1], "claude-sonnet-5-5");
  assert.match(seen.args[seen.args.indexOf("--append-system-prompt") + 1], /do not push/);
  // the prompt (ticket) goes through stdin
  assert.match(seen.stdin, /BAU-142/);
  assert.doesNotMatch(seen.stdin, /ask_user/);
  // the repo of the job is named in the rules
  assert.match(seen.args[seen.args.indexOf("--append-system-prompt") + 1], /firma\/web/);
  // environment: proxy mode passes the token, not the api key; nothing else from the container
  assert.equal(seen.env.ANTHROPIC_AUTH_TOKEN, TOKEN);
  assert.equal(seen.env.ANTHROPIC_BASE_URL, "http://cockpit-host:4000");
  for (const gone of ["ANTHROPIC_API_KEY", "OPENAI_API_KEY", "RANDOM_SECRET", "COCKPIT_PROVIDER"]) {
    assert.equal(seen.env[gone], undefined, `${gone} is removed`);
  }
});

test("environment: directly at Anthropic only the api key, never a token", () => {
  const e = claudeEnv({ PATH: "/bin", ANTHROPIC_API_KEY: "k-123456789", ANTHROPIC_AUTH_TOKEN: "t-123456789" }, "m");
  assert.equal(e.ANTHROPIC_API_KEY, "k-123456789");
  assert.equal(e.ANTHROPIC_AUTH_TOKEN, undefined);
  assert.equal(e.ANTHROPIC_BASE_URL, undefined);
  assert.equal(e.ANTHROPIC_DEFAULT_SONNET_MODEL, "m");
});

test("plan mode only gets the reading tools and ends with a plan", async () => {
  const bin = fakeBin();
  const root = repo();
  const plan = '```json\n{"summary":"Ring ergänzen","files":["src/a.txt"],"steps":["a"]}\n```';
  bin.install("claude", { lines: [assistant("m1", [{ type: "text", text: plan }]), ok(plan)] });
  const { s, d } = adapterEnv(bin.dir, root, PROXY);
  assert.equal(await claudeCode().run(job({ mode: "plan" }), d), 0);
  const tools = bin.seen().args;
  assert.equal(tools[tools.indexOf("--allowedTools") + 1], "Read,Glob,Grep");
  const p = s.e.at(-1) as any;
  assert.deepEqual([p.t, p.summary, p.files, p.steps], ["plan", "Ring ergänzen", ["src/a.txt"], ["a"]]);
});

test("plan mode that changes a file is discarded", async () => {
  const bin = fakeBin();
  const root = repo();
  bin.install("claude", { write: { "src/x.txt": "x" }, lines: [ok("{}")] });
  const { s, d } = adapterEnv(bin.dir, root, PROXY);
  assert.equal(await claudeCode().run(job({ mode: "plan" }), d), 1);
  assert.match((s.e.at(-1) as any).text, /changed files in plan mode/);
});

test("an error result becomes an error event", async () => {
  const bin = fakeBin();
  const { s, d } = adapterEnv(bin.dir, repo(), PROXY);
  bin.install("claude", { lines: [j({ type: "result", subtype: "error_max_turns", is_error: true })] });
  assert.equal(await claudeCode().run(job(), d), 1);
  assert.deepEqual(s.e, [{ t: "error", text: "The model ended the run with “error_max_turns”." }]);
});

test("a broken line is skipped, the rest still counts", async () => {
  const bin = fakeBin();
  const root = repo();
  bin.install("claude", { write: { "src/b.txt": "b" }, lines: ["{kaputt", assistant("m1", [{ type: "text", text: "hm" }]), "", ok("fertig")] });
  const { s, d } = adapterEnv(bin.dir, root, PROXY);
  assert.equal(await claudeCode().run(job(), d), 0);
  assert.equal((s.e.at(-1) as any).t, "result");
});

test("partial output without a result is an error with the end of stderr", async () => {
  const bin = fakeBin();
  const root = repo();
  bin.install("claude", { lines: [assistant("m1", [{ type: "text", text: "halb" }]), '{"type":"assis'], stderr: ["API Error: 529 overloaded"], exit: 3 });
  const { s, d } = adapterEnv(bin.dir, root, PROXY);
  assert.equal(await claudeCode().run(job(), d), 1);
  const e = s.e.at(-1) as any;
  assert.equal(e.t, "error");
  assert.match(e.text, /status 3/);
  assert.match(e.text, /529 overloaded/);
  assert.match(e.text, /1 unreadable/);
});

test("abort by the time limit", async () => {
  const bin = fakeBin();
  bin.install("claude", { sleep: 10_000, lines: [ok("zu spät")] });
  const { s, d } = adapterEnv(bin.dir, repo(), PROXY);
  assert.equal(await claudeCode().run(job({ max_seconds: 1 }), d), 1);
  assert.match((s.e.at(-1) as any).text, /time limit/);
});

test("a file against the rules aborts the run, nothing is committed", async () => {
  const bin = fakeBin();
  const root = repo();
  bin.install("claude", { write: { ".github/workflows/x.yml": "on: push", "src/b.txt": "b" }, lines: [ok("fertig")] });
  const { s, d } = adapterEnv(bin.dir, root, PROXY);
  assert.equal(await claudeCode().run(job({ rules: { forbidden_paths: [".github/"] } }), d), 1);
  const e = s.e.at(-1) as any;
  assert.equal(e.t, "error");
  assert.match(e.text, /broke the rules/);
  assert.match(e.text, /\.github\/workflows\/x\.yml/);
  assert.equal(commits(root), 1);
});

test("a file outside the allowed paths aborts the run", async () => {
  const bin = fakeBin();
  const root = repo();
  bin.install("claude", { write: { "docs/n.md": "n" }, lines: [ok("fertig")] });
  const { s, d } = adapterEnv(bin.dir, root, PROXY);
  assert.equal(await claudeCode().run(job({ rules: { allowed_paths: ["src/**"] } }), d), 1);
  assert.match((s.e.at(-1) as any).text, /outside the allowed paths/);
});

test("an agent that commits itself is checked as well", async () => {
  const bin = fakeBin();
  const root = repo();
  bin.install("claude", { write: { "secrets/k.env": "k" }, commit: true, lines: [ok("fertig")] });
  const { s, d } = adapterEnv(bin.dir, root, PROXY);
  assert.equal(await claudeCode().run(job({ rules: { forbidden_paths: ["secrets/"] } }), d), 1);
  assert.match((s.e.at(-1) as any).text, /secrets\/k\.env/);
});

test("the hook of the settings file denies what the policy forbids", async () => {
  const bin = fakeBin();
  const root = repo();
  bin.install("claude", {
    write: { "src/b.txt": "b" },
    lines: [ok("fertig")],
    hook: [
      { tool_name: "Write", tool_input: { file_path: `${root}/.github/ci.yml` } },
      { tool_name: "Write", tool_input: { file_path: "/etc/passwd" } },
      { tool_name: "Bash", tool_input: { command: "git push origin HEAD" } },
      { tool_name: "Bash", tool_input: { command: "curl https://example.invalid" } },
      { tool_name: "Bash", tool_input: { command: "printenv" } },
      { tool_name: "WebFetch", tool_input: { url: "https://example.invalid" } },
      { tool_name: "Write", tool_input: { file_path: `${root}/src/b.txt` } },
      { tool_name: "Read", tool_input: { file_path: `${root}/src/a.txt` } },
      { tool_name: "Bash", tool_input: { command: "npm test" } },
    ],
  });
  const { d } = adapterEnv(bin.dir, root, PROXY);
  await claudeCode().run(job({ rules: { forbidden_paths: [".github/"] } }), d);
  const answers = bin.seen().hook.map((h) => (h.answer ? JSON.parse(h.answer).hookSpecificOutput.permissionDecision : "no objection"));
  assert.deepEqual(answers, ["deny", "deny", "deny", "deny", "deny", "deny", "no objection", "no objection", "no objection"]);
});

test("the hook fails closed", async () => {
  const policy = { root: "/work", allowed: [], forbidden: [] };
  assert.match(hookAnswer(policy, "kein json"), /"deny"/);
  assert.match(hookAnswer(policy, "{}"), /"deny"/);
  assert.equal(hookAnswer(policy, JSON.stringify({ tool_name: "Read", tool_input: { file_path: "/work/a" } })), "");
  // as a process with an unreadable policy file
  const { execFileSync } = await import("node:child_process");
  const hook = new URL("../src/hook.ts", import.meta.url).pathname;
  const out = execFileSync(process.execPath, [hook, "/nicht/da.json"], { input: "{}", encoding: "utf8" });
  assert.match(out, /"deny"/);
  const bad = join(repo(), "p.json");
  writeFileSync(bad, "[]");
  assert.match(execFileSync(process.execPath, [hook, bad], { input: "{}", encoding: "utf8" }), /"deny"/);
});

test("settings: hook on every tool and denied tools", () => {
  const s = claudeSettings("cmd") as any;
  assert.equal(s.hooks.PreToolUse[0].matcher, "*");
  assert.deepEqual(s.permissions.deny, ["WebFetch", "WebSearch"]);
  assert.ok(!claudeArgs({ model: "m", plan: false, system: "s", settings: "f" }).some((a) => /skip|bypass/i.test(a)));
});

test("missing program and missing model", async () => {
  const { s, d } = adapterEnv("/nicht/da", repo(), PROXY);
  assert.equal(await claudeCode().run(job(), d), 2);
  assert.match((s.e[0] as any).text, /Claude Code is not in the image/);
  const x = adapterEnv("/nicht/da", repo(), {});
  assert.equal(await claudeCode().run(job(), x.d), 1);
  assert.match((x.s.e[0] as any).text, /ANTHROPIC_MODEL is missing/);
});
