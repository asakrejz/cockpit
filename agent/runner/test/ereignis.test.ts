import assert from "node:assert/strict";
import { test } from "node:test";
import { MAX_TEXT, stdoutSenke, zeile } from "../src/ereignis.ts";

test("eine Zeile enthält keinen Zeilenumbruch und ist gültiges JSON", () => {
  const z = zeile({ t: "fehler", text: "a\nb\r\nc" });
  assert.ok(!z.includes("\n"));
  assert.deepEqual(JSON.parse(z), { t: "fehler", text: "a\nb\r\nc" });
});

test("lange Texte und Listen werden gekürzt, damit die Zeile klein bleibt", () => {
  const z = zeile({ t: "plan", summary: "x".repeat(10_000), files: Array.from({ length: 500 }, (_, i) => `f${i}`), steps: [] });
  const j = JSON.parse(z);
  assert.equal(j.summary.length, MAX_TEXT);
  assert.equal(j.files.length, 200);
  assert.ok(z.length < 64 * 1024);
});

test("die Senke schreibt genau eine Zeile je Ereignis", () => {
  const out: string[] = [];
  const senke = stdoutSenke((s) => out.push(s));
  senke({ t: "hinweise_angekommen" });
  senke({ t: "dateien", dateien: ["a.rs"] });
  assert.deepEqual(out, ['{"t":"hinweise_angekommen"}\n', '{"t":"dateien","dateien":["a.rs"]}\n']);
});
