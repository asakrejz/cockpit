import assert from "node:assert/strict";
import { test } from "node:test";
import { aufgabenText, pruefeAuftrag, systemText } from "../src/auftrag.ts";
import { auftrag } from "./hilfen.ts";

test("ein Auftrag bekommt Standardwerte", () => {
  const a = pruefeAuftrag({ modus: "plan", ticket: { key: "X-1" } });
  assert.equal(a.max_sekunden, 1800);
  assert.equal(a.basis_branch, "main");
  assert.deepEqual(a.regeln.verbotene_pfade, []);
});

test("unbekannter Modus und fremder Branch werden abgelehnt", () => {
  assert.throws(() => pruefeAuftrag({ modus: "böse" }), /Unbekannter Modus/);
  assert.throws(() => pruefeAuftrag({ modus: "umsetzen", branch: "main" }), /claude\//);
  assert.throws(() => pruefeAuftrag({ modus: "umsetzen", branch: "claude/../main x" }), /claude\//);
  assert.throws(() => pruefeAuftrag(null), /kein Objekt/);
});

test("die festen Regeln stehen im Systemtext, die Regeln des Repos auch", () => {
  const t = systemText(auftrag({ regeln: { verbotene_pfade: ["*.lock"], pflicht_tests: ["cargo test"], anweisungen: ["Kleine Commits"] } }));
  assert.match(t, /pushst nicht, mergst nicht/);
  assert.match(t, /\*\.lock/);
  assert.match(t, /cargo test/);
  assert.match(t, /Kleine Commits/);
});

test("Aufgabentext je Modus", () => {
  assert.match(aufgabenText(auftrag({ modus: "plan", plan_anmerkungen: ["Auch Tests"] })), /nur lesen[\s\S]*Auch Tests/);
  const u = aufgabenText(auftrag({ plan: { summary: "Ring", files: [], steps: ["a"] }, bisher: ["Datei gelesen"] }));
  assert.match(u, /Freigegebener Plan: Ring/);
  assert.match(u, /rueckfrage/);
  assert.match(u, /Datei gelesen/);
  assert.match(aufgabenText(auftrag({ tests_ausfuehren: false })), /keine Tests/);
});
