import assert from "node:assert/strict";
import { test } from "node:test";
import { lauf, planAusAntwort } from "../src/lauf.ts";
import type { Ausfuehren } from "../src/git.ts";
import { assistant, auftrag, eingang, ergebnisOk, sammler, sdkAttrappe } from "./hilfen.ts";

/** Ein Git, das nichts ausführt und festgelegte Antworten gibt. */
function gitAttrappe(opt: { branch?: string; status?: string } = {}) {
  const aufrufe: string[][] = [];
  const git: Ausfuehren = async (args) => {
    aufrufe.push(args);
    if (args[0] === "rev-parse" && args[1] === "--abbrev-ref") return `${opt.branch ?? "claude/BAU-142-fokus-ring"}\n`;
    if (args[0] === "status") return opt.status ?? "";
    if (args[0] === "rev-parse") return "abc1234\n";
    return "";
  };
  return { git, aufrufe };
}

const umgebung = { ANTHROPIC_MODEL: "claude-sonnet-5-5", ANTHROPIC_BASE_URL: "https://litellm.example", ANTHROPIC_AUTH_TOKEN: "streng-geheim" };

function deps(sdk: ReturnType<typeof sdkAttrappe>, s: ReturnType<typeof sammler>, g = gitAttrappe(), ein = eingang()) {
  let t = 0;
  return { sdk, senke: s.senke, git: g.git, eingang: ein, wurzel: "/work", jetzt: () => (t += 1000), umgebung };
}

test("Umsetzen: Schritte mit Verbrauch, Dateien, Commit und Ergebnis", async () => {
  const sdk = sdkAttrappe([
    assistant("m1", [{ type: "tool_use", name: "Read", input: { file_path: "/work/src/ring.rs" } }]),
    assistant("m2", [{ type: "tool_use", name: "Edit", input: { file_path: "/work/src/ring.rs" } }], { input_tokens: 300, output_tokens: 50 }),
    ergebnisOk("Fokus-Ring ergänzt."),
  ]);
  const s = sammler();
  const g = gitAttrappe({ status: " M src/ring.rs\0" });
  const code = await lauf(auftrag(), deps(sdk, s, g));
  assert.equal(code, 0);
  const t = s.e.map((e) => e.t);
  assert.deepEqual(t, ["schritt", "schritt", "dateien", "dateien", "ergebnis"]);
  assert.deepEqual(s.e[0], { t: "schritt", titel: "Datei lesen", text: "/work/src/ring.rs", token_ein: 100, token_aus: 20, ms: 1000 });
  assert.deepEqual((s.e[2] as any).dateien, ["src/ring.rs"]);
  const erg = s.e[4] as any;
  assert.equal(erg.branch, "claude/BAU-142-fokus-ring");
  assert.equal(erg.commit, "abc1234");
  assert.equal(erg.text, "Fokus-Ring ergänzt.");
  // Commit ohne Hooks, ohne Push.
  assert.ok(g.aufrufe.some((a) => a.includes("commit") && a.includes("--no-verify")));
  assert.ok(!g.aufrufe.some((a) => a.includes("push")));
});

test("derselbe Verbrauch einer Nachricht zählt nur einmal", async () => {
  const sdk = sdkAttrappe([
    assistant("m1", [{ type: "tool_use", name: "Read", input: { file_path: "a" } }]),
    assistant("m1", [{ type: "tool_use", name: "Grep", input: { pattern: "x" } }]),
    ergebnisOk("ok"),
  ]);
  const s = sammler();
  await lauf(auftrag({ modus: "plan" }), deps(sdk, s));
  const schritte = s.e.filter((e) => e.t === "schritt") as any[];
  assert.equal(schritte[0].token_ein + schritte[1].token_ein, 100);
});

test("Plan: nur lesen, Plan aus dem JSON-Block", async () => {
  const antwort = 'Hier der Plan:\n```json\n{"summary":"Ring ergänzen","files":["src/ring.rs"],"steps":["Lesen","Ändern"]}\n```';
  const sdk = sdkAttrappe([assistant("m1", [{ type: "text", text: antwort }]), ergebnisOk(antwort)]);
  const s = sammler();
  const code = await lauf(auftrag({ modus: "plan", branch: "" }), deps(sdk, s));
  assert.equal(code, 0);
  assert.deepEqual(s.e.at(-1), { t: "plan", summary: "Ring ergänzen", files: ["src/ring.rs"], steps: ["Lesen", "Ändern"] });
  assert.equal(sdk.optionen?.permissionMode, "plan");
  assert.deepEqual(sdk.optionen?.allowedTools, ["Read", "Glob", "Grep"]);
  assert.equal(sdk.optionen?.mcpServers, undefined);
});

test("Plan ohne JSON: die Antwort wird zur Zusammenfassung", () => {
  assert.deepEqual(planAusAntwort("Ich ändere den Ring."), { summary: "Ich ändere den Ring.", files: [], steps: [] });
  assert.equal(planAusAntwort("```json\n{kaputt\n```").summary.length > 0, true);
});

test("Optionen: Modell überall gesetzt, kein Anthropic-Dienst, keine Projekteinstellungen", async () => {
  const sdk = sdkAttrappe([ergebnisOk("ok")]);
  const s = sammler();
  await lauf(auftrag(), deps(sdk, s, gitAttrappe({ status: " M a\0" })));
  const o = sdk.optionen!;
  assert.equal(o.model, "claude-sonnet-5-5");
  assert.equal(o.env.ANTHROPIC_DEFAULT_HAIKU_MODEL, "claude-sonnet-5-5");
  assert.equal(o.env.ANTHROPIC_SMALL_FAST_MODEL, "claude-sonnet-5-5");
  assert.equal(o.env.CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC, "1");
  assert.deepEqual(o.settingSources, []);
  assert.equal(o.cwd, "/work");
  assert.ok(!JSON.stringify(s.e).includes("streng-geheim"));
});

test("canUseTool lehnt Push und fremde Pfade ab", async () => {
  const sdk = sdkAttrappe([ergebnisOk("ok")]);
  await lauf(auftrag({ regeln: { verbotene_pfade: ["*.lock"] } }), deps(sdk, sammler(), gitAttrappe({ status: " M a\0" })));
  const erlaubt = sdk.optionen!.canUseTool;
  assert.equal((await erlaubt("Bash", { command: "git push" })).behavior, "deny");
  assert.equal((await erlaubt("Edit", { file_path: "/work/Cargo.lock" })).behavior, "deny");
  assert.equal((await erlaubt("Edit", { file_path: "/work/src/a.rs" })).behavior, "allow");
});

test("Fehler des Modells wird gemeldet, Exit 1", async () => {
  const sdk = sdkAttrappe([{ type: "result", subtype: "error_max_turns", is_error: true }]);
  const s = sammler();
  assert.equal(await lauf(auftrag(), deps(sdk, s)), 1);
  assert.deepEqual(s.e.at(-1)?.t, "fehler");
  assert.match((s.e.at(-1) as any).text, /error_max_turns/);
});

test("Eine Ausnahme des SDK wird zu einem Fehler in Klartext", async () => {
  const sdk = sdkAttrappe([]);
  sdk.query = () => (async function* () { throw new Error("Verbindung abgewiesen"); })() as never;
  const s = sammler();
  assert.equal(await lauf(auftrag(), deps(sdk, s)), 1);
  assert.match((s.e.at(-1) as any).text, /Das SDK meldet: Verbindung abgewiesen/);
});

test("Ohne Änderung kein Ergebnis", async () => {
  const sdk = sdkAttrappe([ergebnisOk("nichts zu tun")]);
  const s = sammler();
  assert.equal(await lauf(auftrag(), deps(sdk, s, gitAttrappe({ status: "" }))), 1);
  assert.match((s.e.at(-1) as any).text, /nichts geändert/);
});

test("Falscher Branch im Klon: Fehler statt Commit", async () => {
  const sdk = sdkAttrappe([ergebnisOk("ok")]);
  const s = sammler();
  const g = gitAttrappe({ branch: "main", status: " M a\0" });
  assert.equal(await lauf(auftrag(), deps(sdk, s, g)), 1);
  assert.match((s.e.at(-1) as any).text, /steht auf „main“/);
  assert.ok(!g.aufrufe.some((a) => a.includes("commit")));
});

test("Rückfrage: Frage geht raus, die Antwort von stdin kommt als Werkzeugergebnis zurück", async () => {
  const sdk = sdkAttrappe([{ type: "__frage", result: "Welchen Farbton?" }, ergebnisOk("ok")]);
  const s = sammler();
  const code = await lauf(auftrag(), deps(sdk, s, gitAttrappe({ status: " M a\0" }), eingang([{ t: "antwort", text: "blau" }])));
  assert.equal(code, 0);
  const frage = s.e.find((e) => e.t === "frage") as any;
  assert.equal(frage.text, "Welchen Farbton?");
  assert.deepEqual(sdk.fragen, ["Welchen Farbton?"]);
});

test("Hinweis des Nutzers wird bestätigt", async () => {
  const sdk = sdkAttrappe([assistant("m1", [{ type: "text", text: "x" }]), ergebnisOk("ok")], async () => {
    await new Promise((r) => setTimeout(r, 30));
  });
  const s = sammler();
  await lauf(auftrag(), deps(sdk, s, gitAttrappe({ status: " M a\0" }), eingang([{ t: "hinweis", text: "Kleine Commits" }])));
  assert.ok(s.e.some((e) => e.t === "hinweise_angekommen"));
});

test("Zeitlimit im Container bricht ab und sagt es", async () => {
  const sdk = sdkAttrappe([]);
  sdk.query = ({ options }) =>
    (async function* () {
      await new Promise((_, nein) => options.abortController.signal.addEventListener("abort", () => nein(new Error("abgebrochen"))));
      yield { type: "x" };
    })() as never;
  const s = sammler();
  const code = await lauf(auftrag({ max_sekunden: 1 }), deps(sdk, s));
  assert.equal(code, 1);
  assert.match((s.e.at(-1) as any).text, /Zeitlimit/);
});

test("Ohne Modell läuft nichts los", async () => {
  const s = sammler();
  const d = deps(sdkAttrappe([]), s);
  assert.equal(await lauf(auftrag(), { ...d, umgebung: {} }), 1);
  assert.match((s.e[0] as any).text, /kein Modell/);
});
