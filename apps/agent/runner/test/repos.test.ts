// A job in several repos: every repo has its folder under /work, its own branch and its own rules.

import assert from "node:assert/strict";
import { test } from "node:test";
import { run } from "../src/adapter/claude-agent-sdk.ts";
import { line } from "../src/event.ts";
import { type Run, recordJob } from "../src/git.ts";
import { checkJob, systemText } from "../src/job.ts";
import { decide, pathViolation, policyOf } from "../src/policy.ts";
import { assistant, collector, input, job, resultOk, sdkFake } from "./helpers.ts";

const repos = [
  {
    repo: "acme/app",
    dir: "acme__app",
    branch: "claude/BAU-142-fokus-ring",
    base_branch: "main",
    rules: { allowed_paths: [], forbidden_paths: ["secrets/"], required_tests: ["cargo test"], instructions: [] },
    contributing: "CONTRIBUTING.md",
    pr_language: "en",
  },
  {
    repo: "acme/web",
    dir: "acme__web",
    branch: "feature/BAU-142",
    base_branch: "develop",
    rules: { allowed_paths: ["src/"], forbidden_paths: [], required_tests: [], instructions: ["Keine neuen Abhängigkeiten."] },
    contributing: "",
    pr_language: "de",
  },
];

const two = () => job({ repos });

/** A Git per folder: `changed` lists the folders with a change; the branch of each is the one of the job. */
function gitByDir(changed: string[]) {
  const calls: Array<{ cwd: string; args: string[] }> = [];
  const branches: Record<string, string> = { "/work/acme__app": "claude/BAU-142-fokus-ring", "/work/acme__web": "feature/BAU-142" };
  const git: Run = async (args, cwd) => {
    calls.push({ cwd, args });
    if (args[0] === "rev-parse" && args[1] === "--abbrev-ref") return `${branches[cwd]}\n`;
    if (args[0] === "status") return changed.includes(cwd) ? " M src/ring.rs\0" : "";
    if (args[0] === "rev-parse") return cwd.endsWith("app") ? "aaa1111\n" : "bbb2222\n";
    return "";
  };
  return { git, calls };
}

test("the job reads all repos and checks their folders and branches", () => {
  const j = two();
  assert.equal(j.repos.length, 2);
  assert.equal(j.repos[1].base_branch, "develop");
  assert.equal(j.repos[1].pr_language, "de");
  // A single entry is no job in several repos.
  assert.deepEqual(job({ repos: [repos[0]] }).repos, []);
  assert.throws(() => job({ repos: [repos[0], { ...repos[1], dir: "../etc" }] }), /passt nicht/);
  assert.throws(() => job({ repos: [repos[0], { ...repos[1], branch: "main" }] }), /kein erlaubter Branch/);
  assert.throws(() => job({ repos: [repos[0], { ...repos[0] }] }), /doppelt/);
  assert.throws(() => job({ repos: [repos[0], { ...repos[1], repo: "evil" }] }), /ungültig/);
  assert.throws(() => checkJob({ mode: "implement", ticket: { key: "B-1", title: "t" }, branch: "claude/B-1", repos: Array(11).fill(repos[0]) }), /höchstens/);
});

test("the instructions name every repo with its folder, branch and rules", () => {
  const text = systemText(two());
  for (const part of ["/work/acme__app", "/work/acme__web", "feature/BAU-142", "Basis-Branch develop", "pro Repository"]) {
    assert.ok(text.includes(part), part);
  }
  assert.match(text, /Im Repository acme\/app \(\/work\/acme__app\): Diese Pfade darfst du nie ändern: secrets\//);
  assert.match(text, /Im Repository acme\/web .*Du darfst nur diese Pfade ändern: src\//);
  assert.match(text, /Keine neuen Abhängigkeiten/);
  assert.match(text, /\/work\/acme__app\/CONTRIBUTING.md/);
  assert.ok(!text.includes("im Verzeichnis /work an einem geklonten"));
});

test("the policy applies the rules of the repo a path lies in", () => {
  const p = policyOf("/work", two());
  assert.equal(decide(p, "Write", { file_path: "/work/acme__app/src/a.rs" }).behavior, "allow");
  assert.equal(decide(p, "Write", { file_path: "/work/acme__app/secrets/k" }).behavior, "deny");
  // `web` may only change src/, `app` has no such limit.
  assert.equal(decide(p, "Edit", { file_path: "/work/acme__web/src/a.ts" }).behavior, "allow");
  assert.equal(decide(p, "Edit", { file_path: "/work/acme__web/README.md" }).behavior, "deny");
  assert.equal(decide(p, "Edit", { file_path: "/work/acme__app/README.md" }).behavior, "allow");
  // Outside of every repo, or in the root itself.
  assert.equal(decide(p, "Write", { file_path: "/work/other/x" }).behavior, "deny");
  assert.equal(decide(p, "Write", { file_path: "/work/acme__app" }).behavior, "deny");
  assert.equal(decide(p, "Write", { file_path: "/work/acme__app/.git/config" }).behavior, "deny");
  assert.match(pathViolation(p, "acme__web/docs/a.md") ?? "", /außerhalb der erlaubten Pfade/);
  assert.equal(pathViolation(p, "acme__web/src/a.md"), null);
  assert.match(pathViolation(p, "nope/a.md") ?? "", /keinem der Repos/);
});

test("a repo without changes gets no commit; the result names the changed one", async () => {
  const g = gitByDir(["/work/acme__app"]);
  const f = await recordJob(g.git, "/work", two());
  assert.deepEqual(f?.files, ["acme__app/src/ring.rs"]);
  assert.deepEqual(f?.repos, [{ repo: "acme/app", branch: "claude/BAU-142-fokus-ring", commit: "aaa1111" }]);
  const commits = g.calls.filter((c) => c.args.includes("commit"));
  assert.equal(commits.length, 1);
  assert.equal(commits[0].cwd, "/work/acme__app");
  // The commit text follows the language of the repo.
  const both = gitByDir(["/work/acme__app", "/work/acme__web"]);
  await recordJob(both.git, "/work", two());
  const messages = both.calls.filter((c) => c.args.includes("commit")).map((c) => c.args[c.args.indexOf("-m") + 1]);
  assert.match(messages[0], /Implemented by the Cockpit agent/);
  assert.match(messages[1], /Umgesetzt vom Cockpit-Agenten/);
  assert.equal(await recordJob(gitByDir([]).git, "/work", two()), null);
});

test("implement in two repos: files with the folder, one result with the repos", async () => {
  const sdk = sdkFake([
    assistant("m1", [{ type: "tool_use", name: "Edit", input: { file_path: "/work/acme__app/src/ring.rs" } }]),
    resultOk("Fertig."),
  ]);
  const s = collector();
  const g = gitByDir(["/work/acme__app", "/work/acme__web"]);
  let t = 0;
  const code = await run(two(), { sdk, sink: s.sink, git: g.git, input: input(), root: "/work", now: () => (t += 1000), env: { ANTHROPIC_MODEL: "m", ANTHROPIC_AUTH_TOKEN: "k" } });
  assert.equal(code, 0);
  const result = s.e.find((e) => e.t === "result") as any;
  assert.equal(result.branch, "claude/BAU-142-fokus-ring", "the first repo is the primary one");
  assert.deepEqual(
    result.repos.map((r: any) => [r.repo, r.branch]),
    [["acme/app", "claude/BAU-142-fokus-ring"], ["acme/web", "feature/BAU-142"]],
  );
  const files = (s.e.filter((e) => e.t === "files").pop() as any).files;
  assert.deepEqual(files, ["acme__app/src/ring.rs", "acme__web/src/ring.rs"]);
  assert.ok(!g.calls.some((c) => c.args.includes("push")));
  // The SDK is started in the folder that holds all repos, with the rules of every repo in the policy.
  assert.equal(sdk.options?.cwd, "/work");
  assert.match(String(sdk.options?.systemPrompt), /acme__web/);
});

test("the event line keeps the repos of a result as objects", () => {
  const l = JSON.parse(line({ t: "result", text: "x", branch: "b", commit: "c", repos: [{ repo: "acme/app", branch: "b", commit: "c" }] }));
  assert.deepEqual(l.repos, [{ repo: "acme/app", branch: "b", commit: "c" }]);
  const many = JSON.parse(line({ t: "result", text: "x", branch: "b", commit: "c", repos: Array(30).fill({ repo: "a/b", branch: "b", commit: "c" }) }));
  assert.equal(many.repos.length, 10);
});
