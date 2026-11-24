import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { tasksText, checkJob, systemText, reviewText, commitMessage, repoNameValid, LIMITS } from "../src/job.ts";
import { job } from "./helpers.ts";

test("a job gets default values", () => {
  const a = checkJob({ mode: "plan", ticket: { key: "X-1" } });
  assert.equal(a.max_seconds, 1800);
  assert.equal(a.base_branch, "main");
  assert.deepEqual(a.rules.forbidden_paths, []);
});

test("unknown mode and foreign branch are rejected", () => {
  assert.throws(() => checkJob({ mode: "böse" }), /Unknown mode/);
  assert.throws(() => checkJob({ mode: "implement", branch: "main" }), /not an allowed branch name/);
  assert.throws(() => checkJob({ mode: "implement", branch: "claude/../main x" }), /not an allowed branch name/);
  assert.throws(() => checkJob({ mode: "implement", branch: "-x" }), /not an allowed branch name/);
  assert.throws(() => checkJob({ mode: "implement", branch: "" }), /not an allowed branch name/);
  assert.throws(() => checkJob(null), /not an object/);
});

test("a branch from another pattern of the repo is let through (the backend enforces the pattern)", () => {
  assert.equal(checkJob({ mode: "implement", branch: "feature/BAU-1-x" }).branch, "feature/BAU-1-x");
  assert.equal(checkJob({ mode: "implement", branch: "BAU-1-x" }).branch, "BAU-1-x");
});

test("the guidelines of the repo are pointed out to the agent, only from the usual places", () => {
  const none = checkJob({ mode: "plan", ticket: { key: "X-1" } });
  assert.equal(none.contributing, "");
  assert.doesNotMatch(systemText(none), /guidelines/);
  const a = checkJob({ mode: "plan", ticket: { key: "X-1" }, contributing: ".github/CONTRIBUTING.md" });
  assert.match(systemText(a), /guidelines in \.github\/CONTRIBUTING\.md[\s\S]*commit messages/);
  assert.equal(checkJob({ mode: "plan", ticket: { key: "X-1" }, contributing: "/etc/passwd" }).contributing, "");
  assert.equal(checkJob({ mode: "plan", ticket: { key: "X-1" }, contributing: "../../x.md" }).contributing, "");
});

test("the fixed rules are in the system text, so are the rules of the repo", () => {
  const t = systemText(job({ rules: { forbidden_paths: ["*.lock"], required_tests: ["cargo test"], instructions: ["Kleine Commits"] } }));
  assert.match(t, /do not push, do not merge/);
  assert.match(t, /\*\.lock/);
  assert.match(t, /cargo test/);
  assert.match(t, /Kleine Commits/);
});

test("task text per mode", () => {
  assert.match(tasksText(job({ mode: "plan", plan_notes: ["Auch Tests"] })), /read only[\s\S]*Auch Tests/);
  const u = tasksText(job({ plan: { summary: "Ring", files: [], steps: ["a"] }, earlier: ["Datei gelesen"] }));
  assert.match(u, /Approved plan: Ring/);
  assert.match(u, /ask_user/);
  assert.match(u, /Datei gelesen/);
  assert.match(tasksText(job({ run_tests: false })), /Do not run any tests/);
});

test("the repo of the job is named in the system text; odd values are dropped", () => {
  const a = checkJob({ mode: "plan", ticket: { key: "X-1" }, repo: "firma/web" });
  assert.equal(a.repo, "firma/web");
  assert.match(systemText(a), /firma\/web/);
  for (const bad of ["../x", "a/b/c", "x y/z", 5, null]) {
    assert.equal(checkJob({ mode: "plan", ticket: { key: "X-1" }, repo: bad }).repo, "");
  }
  assert.doesNotMatch(systemText(checkJob({ mode: "plan", ticket: { key: "X-1" } })), /The repository in/);
});

// The same cases are checked against `repo_name_valid` in crates/cockpit-core/tests/repo_name_cases.rs.
const REPO_NAME_CASES: Array<{ repo: string; expected: boolean }> = JSON.parse(
  readFileSync(new URL("../../../../crates/cockpit-core/tests/fixtures/repo_name_cases.json", import.meta.url), "utf8"),
);

test("repoNameValid follows the cases shared with cockpit-core", () => {
  assert.ok(REPO_NAME_CASES.length > 80, "the shared repo name cases went missing");
  const wrong = REPO_NAME_CASES.filter((c) => repoNameValid(c.repo) !== c.expected).map(
    (c) => `${JSON.stringify(c.repo)}: expected ${c.expected}`,
  );
  assert.deepEqual(wrong, []);
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
  assert.match(tasksText(job()), /Implement the ticket/);
});

test("review comments become the task, marked as texts of third parties", () => {
  const a = job({ followup: followupReview });
  const t = tasksText(a);
  assert.match(t, /follow-up job for the existing pull request #85 on the branch claude\/BAU-142-fokus-ring/);
  assert.match(t, /src\/a\.rs:4/);
  assert.match(t, /mira: Warum unwrap\?/);
  assert.match(t, /Review by mira \(CHANGES_REQUESTED\): Bitte Tests\./);
  assert.match(t, /from third parties/);
  assert.doesNotMatch(t, /Implement the ticket/);
  assert.match(t, /Review comments:/);
  // The plan mode gets the same input.
  assert.match(tasksText(job({ mode: "plan", followup: followupReview })), /Warum unwrap/);
});

test("failed checks are named with their excerpt", () => {
  const a = job({ followup: { kind: "failed_checks", pull_request: 85, threads: [], notes: [], checks: [{ name: "Lint", excerpt: "src/a.rs:7: ungenutzte Variable" }] } });
  const t = tasksText(a);
  assert.match(t, /Failed checks:/);
  assert.match(t, /- Lint/);
  assert.match(t, /ungenutzte Variable/);
  assert.match(t, /Fix the cause of the failed checks/);
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
  assert.throws(() => job({ followup: { kind: "böse" } }), /Unknown follow-up kind/);
  const a = job({ followup: { kind: "review_comments", threads: "kaputt", notes: [1, null], checks: {} } });
  assert.deepEqual(a.followup!.threads, []);
  assert.deepEqual(a.followup!.notes, []);
});

test("commit message names what was done, in the language of the pull request", () => {
  assert.match(commitMessage(job()), /^BAU-142: Fokus-Ring fehlt/);
  // The pull request language is English unless the repo says otherwise.
  assert.match(commitMessage(job({ followup: followupReview })), /^BAU-142: Address review comments \(#85\)/);
  assert.match(commitMessage(job({ followup: { ...followupReview, kind: "failed_checks" } })), /Fix failing checks/);
  assert.match(commitMessage(job()), /Implemented by the Cockpit agent\./);
  const de = job({ followup: followupReview, pr_language: "de" });
  assert.match(commitMessage(de), /^BAU-142: Review-Kommentare umsetzen \(#85\)/);
  assert.match(commitMessage({ ...de, followup: { ...followupReview, kind: "failed_checks" } }), /Rote Checks beheben/);
  assert.match(commitMessage(de), /Umgesetzt vom Cockpit-Agenten\./);
});

test("the language of the interface and of the pull request come from the job, English by default", () => {
  assert.equal(job().language, "en");
  assert.equal(job().pr_language, "en");
  assert.equal(checkJob({ mode: "plan", language: "de", pr_language: "en" }).language, "de");
  assert.equal(checkJob({ mode: "plan", language: "en", pr_language: "de" }).pr_language, "de");
  assert.equal(checkJob({ mode: "plan", language: "fr" }).language, "en");
  assert.equal(checkJob({ mode: "plan", pr_language: 3 }).pr_language, "en");
});

test("the model is told to answer in the chosen language", () => {
  assert.match(systemText(job({ language: "de" })), /Write your answer in German\./);
  assert.match(systemText(job({ language: "en" })), /Write your answer in English\./);
  assert.doesNotMatch(systemText(job({ language: "en" })), /in German/);
  assert.match(systemText(job({ mode: "review", language: "en" })), /in English, the language of the pull request/);
});

test("the prompts are English whatever the language of the job is; only the answer language differs", () => {
  const shown = (a: ReturnType<typeof job>) => [systemText(a), tasksText(a), tasksText(a, { canAsk: false })].join("\n----\n");
  const withRepos = { repos: [{ repo: "acme/app", dir: "acme__app", branch: "b1", base_branch: "main" }, { repo: "acme/web", dir: "acme__web", branch: "b2", base_branch: "main" }] };
  for (const part of [{}, { mode: "plan" }, withRepos, { followup: followupReview }]) {
    const de = shown(job({ ...part, language: "de" }));
    const en = shown(job({ ...part, language: "en" }));
    assert.equal(de.replaceAll("Write your answer in German.", "Write your answer in English."), en);
  }
  const review = { mode: "review", review: { pull_request: 7, title: "T", description: "", branch: "b", base: "main", files: [], omitted_files: 0 } };
  const de = job({ ...review, language: "en", review_language: "de" });
  const en = job({ ...review, language: "de", review_language: "en" });
  const both = (a: ReturnType<typeof job>) => `${systemText(a)}\n${reviewText(a)}`;
  assert.equal(both(de).replaceAll("in German, the language", "in English, the language"), both(en));
});

test("a review is written in the language of the pull request, not of the interface", () => {
  const a = job({ mode: "review", language: "de", review_language: "en" });
  assert.match(systemText(a), /in English, the language of the pull request/);
  assert.doesNotMatch(systemText(a), /in German/);
  assert.match(reviewText(a), /in English, the language of the pull request/);
  const de = job({ mode: "review", language: "en", review_language: "de" });
  assert.match(systemText(de), /in German, the language of the pull request/);
  assert.match(reviewText(de), /in German, the language of the pull request/);
});

test("review_language comes from the job, falls back to the interface language, then to English", () => {
  assert.equal(checkJob({ mode: "review", language: "en" }).review_language, "en");
  assert.equal(checkJob({ mode: "review", language: "de" }).review_language, "de");
  assert.equal(checkJob({ mode: "review", language: "en", review_language: "de" }).review_language, "de");
  assert.equal(checkJob({ mode: "review", review_language: "fr" }).review_language, "en");
  assert.equal(checkJob({ mode: "review" }).review_language, "en");
});
