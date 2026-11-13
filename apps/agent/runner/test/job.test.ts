import assert from "node:assert/strict";
import { test } from "node:test";
import { tasksText, checkJob, systemText, commitMessage, LIMITS } from "../src/job.ts";
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

// ---------- Follow-up jobs ----------

const followupReview = {
  kind: "review_comments",
  pull_request: 85,
  threads: [{ path: "src/a.rs", line: 4, comments: [{ author: "mira", text: "Warum unwrap?" }, { author: "ich", text: "Stimmt." }] }],
  notes: [{ author: "mira", state: "CHANGES_REQUESTED", text: "Bitte Tests." }],
  checks: [],
};

test("a job without follow-up input keeps working as before", () => {
  assert.equal(job().followup, null);
  assert.match(tasksText(job()), /Setze das Ticket um/);
});

test("review comments become the task, marked as texts of third parties", () => {
  const a = job({ followup: followupReview });
  const t = tasksText(a);
  assert.match(t, /Folgeauftrag zum bestehenden Pull Request #85 auf dem Branch claude\/BAU-142-fokus-ring/);
  assert.match(t, /src\/a\.rs:4/);
  assert.match(t, /mira: Warum unwrap\?/);
  assert.match(t, /Review von mira \(CHANGES_REQUESTED\): Bitte Tests\./);
  assert.match(t, /von Dritten/);
  assert.doesNotMatch(t, /Setze das Ticket um/);
  assert.match(t, /Review-Kommentare/);
  // The plan mode gets the same input.
  assert.match(tasksText(job({ mode: "plan", followup: followupReview })), /Warum unwrap/);
});

test("failed checks are named with their excerpt", () => {
  const a = job({ followup: { kind: "failed_checks", pull_request: 85, threads: [], notes: [], checks: [{ name: "Lint", excerpt: "src/a.rs:7: ungenutzte Variable" }] } });
  const t = tasksText(a);
  assert.match(t, /Fehlgeschlagene Checks/);
  assert.match(t, /- Lint/);
  assert.match(t, /ungenutzte Variable/);
  assert.match(t, /Behebe die Ursache/);
});

test("amounts and sizes are limited again in the runner", () => {
  const many = Array.from({ length: 100 }, (_, i) => ({ path: `f${i}.rs`, line: 1, comments: Array.from({ length: 20 }, () => ({ author: "x", text: "ä".repeat(5000) })) }));
  const a = job({ followup: { ...followupReview, threads: many, checks: Array.from({ length: 50 }, (_, i) => ({ name: `c${i}`, excerpt: "y".repeat(10000) })) } });
  assert.ok(a.followup!.threads.length <= LIMITS.threads);
  assert.ok(a.followup!.threads[0].comments.length <= LIMITS.comments);
  // The overall budget cuts earlier than the per-thread limits.
  assert.ok(a.followup!.threads.length < 30);
  assert.ok(a.followup!.threads[0].comments[0].text.length <= LIMITS.text);
  assert.ok(a.followup!.checks.length <= LIMITS.checks);
  for (const c of a.followup!.checks) assert.ok(c.excerpt.length <= LIMITS.excerpt);
  assert.ok(tasksText(a).length < LIMITS.total + 5_000);
});

test("an unknown kind of follow-up is rejected, garbage is ignored", () => {
  assert.throws(() => job({ followup: { kind: "böse" } }), /Unbekannter Folgeauftrag/);
  const a = job({ followup: { kind: "review_comments", threads: "kaputt", notes: [1, null], checks: {} } });
  assert.deepEqual(a.followup!.threads, []);
  assert.deepEqual(a.followup!.notes, []);
});

test("commit message names what was done", () => {
  assert.match(commitMessage(job()), /^BAU-142: Fokus-Ring fehlt/);
  assert.match(commitMessage(job({ followup: followupReview })), /^BAU-142: Review-Kommentare umsetzen \(#85\)/);
  assert.match(commitMessage(job({ followup: { ...followupReview, kind: "failed_checks" } })), /Rote Checks beheben/);
});
