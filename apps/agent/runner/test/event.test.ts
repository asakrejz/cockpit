import assert from "node:assert/strict";
import { test } from "node:test";
import { MAX_TEXT, stdoutSink, line } from "../src/event.ts";

test("a line contains no line break and is valid JSON", () => {
  const z = line({ t: "error", text: "a\nb\r\nc" });
  assert.ok(!z.includes("\n"));
  assert.deepEqual(JSON.parse(z), { t: "error", text: "a\nb\r\nc" });
});

test("long texts and lists are truncated so that the line stays small", () => {
  const z = line({ t: "plan", summary: "x".repeat(10_000), files: Array.from({ length: 500 }, (_, i) => `f${i}`), steps: [] });
  const j = JSON.parse(z);
  assert.equal(j.summary.length, MAX_TEXT);
  assert.equal(j.files.length, 200);
  assert.ok(z.length < 64 * 1024);
});

test("the sink writes exactly one line per event", () => {
  const out: string[] = [];
  const sink = stdoutSink((s) => out.push(s));
  sink({ t: "hints_arrived" });
  sink({ t: "files", files: ["a.rs"] });
  assert.deepEqual(out, ['{"t":"hints_arrived"}\n', '{"t":"files","files":["a.rs"]}\n']);
});
