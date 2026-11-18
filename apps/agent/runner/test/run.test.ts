import assert from "node:assert/strict";
import { test } from "node:test";
import { run, planFromAnswer } from "../src/adapter/claude-agent-sdk.ts";
import type { Run } from "../src/git.ts";
import { assistant, job, input, resultOk, collector, sdkFake } from "./helpers.ts";

/** A Git that runs nothing and gives fixed answers. */
function gitFake(opt: { branch?: string; status?: string } = {}) {
  const calls: string[][] = [];
  const git: Run = async (args) => {
    calls.push(args);
    if (args[0] === "rev-parse" && args[1] === "--abbrev-ref") return `${opt.branch ?? "claude/BAU-142-fokus-ring"}\n`;
    if (args[0] === "status") return opt.status ?? "";
    if (args[0] === "rev-parse") return "abc1234\n";
    return "";
  };
  return { git, calls };
}

const env = { ANTHROPIC_MODEL: "claude-sonnet-5-5", ANTHROPIC_BASE_URL: "https://litellm.example", ANTHROPIC_AUTH_TOKEN: "streng-geheim" };

function deps(sdk: ReturnType<typeof sdkFake>, s: ReturnType<typeof collector>, g = gitFake(), ein = input()) {
  let t = 0;
  return { sdk, sink: s.sink, git: g.git, input: ein, root: "/work", now: () => (t += 1000), env };
}

test("implement: steps with usage, files, commit and result", async () => {
  const sdk = sdkFake([
    assistant("m1", [{ type: "tool_use", name: "Read", input: { file_path: "/work/src/ring.rs" } }]),
    assistant("m2", [{ type: "tool_use", name: "Edit", input: { file_path: "/work/src/ring.rs" } }], { input_tokens: 300, output_tokens: 50 }),
    resultOk("Fokus-Ring ergänzt."),
  ]);
  const s = collector();
  const g = gitFake({ status: " M src/ring.rs\0" });
  const code = await run(job(), deps(sdk, s, g));
  assert.equal(code, 0);
  const t = s.e.map((e) => e.t);
  assert.deepEqual(t, ["step", "step", "files", "files", "result"]);
  assert.deepEqual(s.e[0], { t: "step", title: "Datei lesen", text: "/work/src/ring.rs", tokens_in: 100, tokens_out: 20, ms: 1000 });
  assert.deepEqual((s.e[2] as any).files, ["src/ring.rs"]);
  const erg = s.e[4] as any;
  assert.equal(erg.branch, "claude/BAU-142-fokus-ring");
  assert.equal(erg.commit, "abc1234");
  assert.equal(erg.text, "Fokus-Ring ergänzt.");
  // Commit without hooks, without push.
  assert.ok(g.calls.some((a) => a.includes("commit") && a.includes("--no-verify")));
  assert.ok(!g.calls.some((a) => a.includes("push")));
});

test("the same usage of a message counts only once", async () => {
  const sdk = sdkFake([
    assistant("m1", [{ type: "tool_use", name: "Read", input: { file_path: "a" } }]),
    assistant("m1", [{ type: "tool_use", name: "Grep", input: { pattern: "x" } }]),
    resultOk("ok"),
  ]);
  const s = collector();
  await run(job({ mode: "plan" }), deps(sdk, s));
  const steps = s.e.filter((e) => e.t === "step") as any[];
  assert.equal(steps[0].tokens_in + steps[1].tokens_in, 100);
});

test("plan: read only, plan from the JSON block", async () => {
  const answer = 'Hier der Plan:\n```json\n{"summary":"Ring ergänzen","files":["src/ring.rs"],"steps":["Lesen","Ändern"]}\n```';
  const sdk = sdkFake([assistant("m1", [{ type: "text", text: answer }]), resultOk(answer)]);
  const s = collector();
  const code = await run(job({ mode: "plan", branch: "" }), deps(sdk, s));
  assert.equal(code, 0);
  assert.deepEqual(s.e.at(-1), { t: "plan", summary: "Ring ergänzen", files: ["src/ring.rs"], steps: ["Lesen", "Ändern"] });
  assert.equal(sdk.options?.permissionMode, "plan");
  assert.deepEqual(sdk.options?.allowedTools, ["Read", "Glob", "Grep"]);
  assert.equal(sdk.options?.mcpServers, undefined);
});

test("plan without JSON: the answer becomes the summary", () => {
  assert.deepEqual(planFromAnswer("Ich ändere den Ring."), { summary: "Ich ändere den Ring.", files: [], steps: [] });
  assert.equal(planFromAnswer("```json\n{kaputt\n```").summary.length > 0, true);
});

test("options: model set everywhere, no Anthropic service, no project settings", async () => {
  const sdk = sdkFake([resultOk("ok")]);
  const s = collector();
  await run(job(), deps(sdk, s, gitFake({ status: " M a\0" })));
  const o = sdk.options!;
  assert.equal(o.model, "claude-sonnet-5-5");
  assert.equal(o.env.ANTHROPIC_DEFAULT_HAIKU_MODEL, "claude-sonnet-5-5");
  assert.equal(o.env.ANTHROPIC_SMALL_FAST_MODEL, "claude-sonnet-5-5");
  assert.equal(o.env.CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC, "1");
  assert.deepEqual(o.settingSources, []);
  assert.equal(o.cwd, "/work");
  assert.ok(!JSON.stringify(s.e).includes("streng-geheim"));
});

test("canUseTool rejects push and foreign paths", async () => {
  const sdk = sdkFake([resultOk("ok")]);
  await run(job({ rules: { forbidden_paths: ["*.lock"] } }), deps(sdk, collector(), gitFake({ status: " M a\0" })));
  const allowed = sdk.options!.canUseTool;
  assert.equal((await allowed("Bash", { command: "git push" })).behavior, "deny");
  assert.equal((await allowed("Edit", { file_path: "/work/Cargo.lock" })).behavior, "deny");
  assert.equal((await allowed("Edit", { file_path: "/work/src/a.rs" })).behavior, "allow");
});

test("error of the model is reported, exit 1", async () => {
  const sdk = sdkFake([{ type: "result", subtype: "error_max_turns", is_error: true }]);
  const s = collector();
  assert.equal(await run(job(), deps(sdk, s)), 1);
  assert.deepEqual(s.e.at(-1)?.t, "error");
  assert.match((s.e.at(-1) as any).text, /error_max_turns/);
});

test("an exception of the SDK becomes an error in plain text", async () => {
  const sdk = sdkFake([]);
  sdk.query = () => (async function* () { throw new Error("Verbindung abgewiesen"); })() as never;
  const s = collector();
  assert.equal(await run(job(), deps(sdk, s)), 1);
  assert.match((s.e.at(-1) as any).text, /Das SDK meldet: Verbindung abgewiesen/);
});

test("without a change no result", async () => {
  const sdk = sdkFake([resultOk("nichts zu tun")]);
  const s = collector();
  assert.equal(await run(job(), deps(sdk, s, gitFake({ status: "" }))), 1);
  assert.match((s.e.at(-1) as any).text, /nichts geändert/);
});

test("wrong branch in the clone: error instead of commit", async () => {
  const sdk = sdkFake([resultOk("ok")]);
  const s = collector();
  const g = gitFake({ branch: "main", status: " M a\0" });
  assert.equal(await run(job(), deps(sdk, s, g)), 1);
  assert.match((s.e.at(-1) as any).text, /steht auf „main“/);
  assert.ok(!g.calls.some((a) => a.includes("commit")));
});

test("question: the question goes out, the answer from stdin comes back as the tool result", async () => {
  const sdk = sdkFake([{ type: "__frage", result: "Welchen Farbton?" }, resultOk("ok")]);
  const s = collector();
  const code = await run(job(), deps(sdk, s, gitFake({ status: " M a\0" }), input([{ t: "answer", text: "blau" }])));
  assert.equal(code, 0);
  const question = s.e.find((e) => e.t === "question") as any;
  assert.equal(question.text, "Welchen Farbton?");
  assert.deepEqual(sdk.ask, ["Welchen Farbton?"]);
});

test("hint of the user is acknowledged", async () => {
  const sdk = sdkFake([assistant("m1", [{ type: "text", text: "x" }]), resultOk("ok")], async () => {
    await new Promise((r) => setTimeout(r, 30));
  });
  const s = collector();
  await run(job(), deps(sdk, s, gitFake({ status: " M a\0" }), input([{ t: "hint", text: "Kleine Commits" }])));
  assert.ok(s.e.some((e) => e.t === "hints_arrived"));
});

test("time limit in the container aborts and says so", async () => {
  const sdk = sdkFake([]);
  sdk.query = ({ options }) =>
    (async function* () {
      await new Promise((_, no) => options.abortController.signal.addEventListener("abort", () => no(new Error("abgebrochen"))));
      yield { type: "x" };
    })() as never;
  const s = collector();
  const code = await run(job({ max_seconds: 1 }), deps(sdk, s));
  assert.equal(code, 1);
  assert.match((s.e.at(-1) as any).text, /Zeitlimit/);
});

test("without a model nothing starts", async () => {
  const s = collector();
  const d = deps(sdkFake([]), s);
  assert.equal(await run(job(), { ...d, env: {} }), 1);
  assert.match((s.e[0] as any).text, /kein Modell/);
});

test("follow-up: same branch, the input is in the prompt, the commit names the task, no push, no GitHub token anywhere", async () => {
  const f = { kind: "review_comments", pull_request: 85, threads: [{ path: "a.rs", line: 2, comments: [{ author: "mira", text: "Bitte umbenennen" }] }], notes: [], checks: [] };
  const sdk = sdkFake(
    [assistant("m1", [{ type: "tool_use", name: "Edit", input: { file_path: "/work/a.rs" } }]), resultOk("Umbenannt.")],
    (o, read) => {
      const prompt = JSON.stringify(read);
      assert.match(prompt, /Bitte umbenennen/);
      // Neither the prompt nor the system text nor the environment of the SDK contain a GitHub token.
      assert.doesNotMatch(prompt + o.systemPrompt, /ghp_|github_pat_|x-access-token/);
      assert.ok(!Object.keys(o.env).some((k) => /^(GH|GITHUB)_?TOKEN/i.test(k)));
    },
  );
  const s = collector();
  const g = gitFake({ status: " M a.rs\0" });
  const code = await run(job({ followup: f }), deps(sdk, s, g));
  assert.equal(code, 0);
  assert.equal((s.e.at(-1) as any).t, "result");
  assert.equal((s.e.at(-1) as any).branch, "claude/BAU-142-fokus-ring");
  const commit = g.calls.find((a) => a.includes("commit"))!;
  assert.ok(commit.some((x) => x.includes("Address review comments (#85)")));
  assert.ok(!g.calls.some((a) => a.includes("push")));
  // No secret in the events either.
  assert.doesNotMatch(JSON.stringify(s.e), /streng-geheim|ghp_/);
});
