import assert from "node:assert/strict";
import { test } from "node:test";
import { tasksText, checkJob, systemText } from "../src/job.ts";
import { job } from "./helpers.ts";

test("ein Auftrag bekommt Standardwerte", () => {
  const a = checkJob({ mode: "plan", ticket: { key: "X-1" } });
  assert.equal(a.max_seconds, 1800);
  assert.equal(a.base_branch, "main");
  assert.deepEqual(a.rules.forbidden_paths, []);
});

test("unbekannter Modus und fremder Branch werden abgelehnt", () => {
  assert.throws(() => checkJob({ mode: "böse" }), /Unbekannter Modus/);
  assert.throws(() => checkJob({ mode: "implement", branch: "main" }), /claude\//);
  assert.throws(() => checkJob({ mode: "implement", branch: "claude/../main x" }), /claude\//);
  assert.throws(() => checkJob(null), /kein Objekt/);
});

test("die festen Regeln stehen im Systemtext, die Regeln des Repos auch", () => {
  const t = systemText(job({ rules: { forbidden_paths: ["*.lock"], required_tests: ["cargo test"], instructions: ["Kleine Commits"] } }));
  assert.match(t, /pushst nicht, mergst nicht/);
  assert.match(t, /\*\.lock/);
  assert.match(t, /cargo test/);
  assert.match(t, /Kleine Commits/);
});

test("Aufgabentext je Modus", () => {
  assert.match(tasksText(job({ mode: "plan", plan_notes: ["Auch Tests"] })), /nur lesen[\s\S]*Auch Tests/);
  const u = tasksText(job({ plan: { summary: "Ring", files: [], steps: ["a"] }, earlier: ["Datei gelesen"] }));
  assert.match(u, /Freigegebener Plan: Ring/);
  assert.match(u, /rueckfrage/);
  assert.match(u, /Datei gelesen/);
  assert.match(tasksText(job({ run_tests: false })), /keine Tests/);
});
