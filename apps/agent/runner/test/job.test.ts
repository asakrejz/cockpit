import assert from "node:assert/strict";
import { test } from "node:test";
import { tasksText, checkJob, systemText } from "../src/job.ts";
import { job } from "./helpers.ts";

test("a job gets default values", () => {
  const a = checkJob({ mode: "plan", ticket: { key: "X-1" } });
  assert.equal(a.max_seconds, 1800);
  assert.equal(a.base_branch, "main");
  assert.deepEqual(a.rules.forbidden_paths, []);
});

test("unknown mode and foreign branch are rejected", () => {
  assert.throws(() => checkJob({ mode: "böse" }), /Unbekannter Modus/);
  assert.throws(() => checkJob({ mode: "implement", branch: "main" }), /claude\//);
  assert.throws(() => checkJob({ mode: "implement", branch: "claude/../main x" }), /claude\//);
  assert.throws(() => checkJob(null), /kein Objekt/);
});

test("the fixed rules are in the system text, so are the rules of the repo", () => {
  const t = systemText(job({ rules: { forbidden_paths: ["*.lock"], required_tests: ["cargo test"], instructions: ["Kleine Commits"] } }));
  assert.match(t, /pushst nicht, mergst nicht/);
  assert.match(t, /\*\.lock/);
  assert.match(t, /cargo test/);
  assert.match(t, /Kleine Commits/);
});

test("task text per mode", () => {
  assert.match(tasksText(job({ mode: "plan", plan_notes: ["Auch Tests"] })), /nur lesen[\s\S]*Auch Tests/);
  const u = tasksText(job({ plan: { summary: "Ring", files: [], steps: ["a"] }, earlier: ["Datei gelesen"] }));
  assert.match(u, /Freigegebener Plan: Ring/);
  assert.match(u, /ask_user/);
  assert.match(u, /Datei gelesen/);
  assert.match(tasksText(job({ run_tests: false })), /keine Tests/);
});

test("the repo of the job is named in the system text; odd values are dropped", () => {
  const a = checkJob({ mode: "plan", ticket: { key: "X-1" }, repo: "firma/web" });
  assert.equal(a.repo, "firma/web");
  assert.match(systemText(a), /firma\/web/);
  for (const bad of ["../x", "a/b/c", "x y/z", 5, null]) {
    assert.equal(checkJob({ mode: "plan", ticket: { key: "X-1" }, repo: bad }).repo, "");
  }
  assert.doesNotMatch(systemText(checkJob({ mode: "plan", ticket: { key: "X-1" } })), /Das Repository in/);
});
