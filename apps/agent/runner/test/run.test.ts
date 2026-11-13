import assert from "node:assert/strict";
import { test } from "node:test";
import { run, planAusAnswer } from "../src/adapter/claude-agent-sdk.ts";
import type { Run } from "../src/git.ts";
import { assistant, job, input, resultOk, collector, sdkFake } from "./hilfen.ts";

/** Ein Git, das nichts ausführt und festgelegte Antworten gibt. */
function gitFake(opt: { branch?: string; status?: string } = {}) {
  const calls: string[][] = [];
  const git: Run = async (args) => {
    calls.push(args);
    if (args[0] === "rev-parse" && args[1] === "--abbrev-ref") return `${opt.branch ?? "claude/Build-142-fokus-ring"}\n`;
    if (args[0] === "status") return opt.status ?? "";
    if (args[0] === "rev-parse") return "abc1234\n";
    return "";
  };
  return { git, calls };
}

const env = { ANTHROPIC_MODEL: "claude-sonnet-5-5", ANTHROPIC_BASE_URL: "https://litellm.example", ANTHROPIC_AUTH_TOKEN: "streng-geheim" };

function deps(sdk: ReturnType<typeof sdkFake>, s: ReturnType<typeof collector>, g = gitFake(), ein = input()) {
  let t = 0;
  return { sdk, sink: s.sink, git: g.git, input: ein, wurzel: "/work", now: () => (t += 1000), env };
}

test("Umsetzen: Schritte mit Verbrauch, Dateien, Commit und Ergebnis", async () => {
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
  // Commit ohne Hooks, ohne Push.
  assert.ok(g.calls.some((a) => a.includes("commit") && a.includes("--no-verify")));
  assert.ok(!g.calls.some((a) => a.includes("push")));
});

test("derselbe Verbrauch einer Nachricht zählt nur einmal", async () => {
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

test("Plan: nur lesen, Plan aus dem JSON-Block", async () => {
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

test("Plan ohne JSON: die Antwort wird zur Zusammenfassung", () => {
  assert.deepEqual(planAusAnswer("Ich ändere den Ring."), { summary: "Ich ändere den Ring.", files: [], steps: [] });
  assert.equal(planAusAnswer("```json\n{kaputt\n```").summary.length > 0, true);
});

test("Optionen: Modell überall gesetzt, kein Anthropic-Dienst, keine Projekteinstellungen", async () => {
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

test("canUseTool lehnt Push und fremde Pfade ab", async () => {
  const sdk = sdkFake([resultOk("ok")]);
  await run(job({ rules: { forbidden_paths: ["*.lock"] } }), deps(sdk, collector(), gitFake({ status: " M a\0" })));
  const allowed = sdk.options!.canUseTool;
  assert.equal((await allowed("Bash", { command: "git push" })).behavior, "deny");
  assert.equal((await allowed("Edit", { file_path: "/work/Cargo.lock" })).behavior, "deny");
  assert.equal((await allowed("Edit", { file_path: "/work/src/a.rs" })).behavior, "allow");
});

test("Fehler des Modells wird gemeldet, Exit 1", async () => {
  const sdk = sdkFake([{ type: "result", subtype: "error_max_turns", is_error: true }]);
  const s = collector();
  assert.equal(await run(job(), deps(sdk, s)), 1);
  assert.deepEqual(s.e.at(-1)?.t, "error");
  assert.match((s.e.at(-1) as any).text, /error_max_turns/);
});

test("Eine Ausnahme des SDK wird zu einem Fehler in Klartext", async () => {
  const sdk = sdkFake([]);
  sdk.query = () => (async function* () { throw new Error("Verbindung abgewiesen"); })() as never;
  const s = collector();
  assert.equal(await run(job(), deps(sdk, s)), 1);
  assert.match((s.e.at(-1) as any).text, /Das SDK reports: Connection rejected/);
});

test("Ohne Änderung kein Ergebnis", async () => {
  const sdk = sdkFake([resultOk("nichts zu tun")]);
  const s = collector();
  assert.equal(await run(job(), deps(sdk, s, gitFake({ status: "" }))), 1);
  assert.match((s.e.at(-1) as any).text, /nothing geändert/);
});

test("Falscher Branch im Klon: Fehler statt Commit", async () => {
  const sdk = sdkFake([resultOk("ok")]);
  const s = collector();
  const g = gitFake({ branch: "main", status: " M a\0" });
  assert.equal(await run(job(), deps(sdk, s, g)), 1);
  assert.match((s.e.at(-1) as any).text, /stands auf „main“/);
  assert.ok(!g.calls.some((a) => a.includes("commit")));
});

test("Rückfrage: Frage geht raus, die Antwort von stdin kommt als Werkzeugergebnis zurück", async () => {
  const sdk = sdkFake([{ type: "__frage", result: "Welchen Farbton?" }, resultOk("ok")]);
  const s = collector();
  const code = await run(job(), deps(sdk, s, gitFake({ status: " M a\0" }), input([{ t: "answer", text: "blau" }])));
  assert.equal(code, 0);
  const question = s.e.find((e) => e.t === "question") as any;
  assert.equal(question.text, "Welchen Farbton?");
  assert.deepEqual(sdk.ask, ["Welchen Farbton?"]);
});

test("Hinweis des Nutzers wird bestätigt", async () => {
  const sdk = sdkFake([assistant("m1", [{ type: "text", text: "x" }]), resultOk("ok")], async () => {
    await new Promise((r) => setTimeout(r, 30));
  });
  const s = collector();
  await run(job(), deps(sdk, s, gitFake({ status: " M a\0" }), input([{ t: "hint", text: "Kleine Commits" }])));
  assert.ok(s.e.some((e) => e.t === "hints_arrived"));
});

test("Zeitlimit im Container bricht ab und sagt es", async () => {
  const sdk = sdkFake([]);
  sdk.query = ({ options }) =>
    (async function* () {
      await new Promise((_, no) => options.abortController.signal.addEventListener("abort", () => no(new Error("abgebrochen"))));
      yield { type: "x" };
    })() as never;
  const s = collector();
  const code = await run(job({ max_seconds: 1 }), deps(sdk, s));
  assert.equal(code, 1);
  assert.match((s.e.at(-1) as any).text, /Timeout/);
});

test("Ohne Modell läuft nichts los", async () => {
  const s = collector();
  const d = deps(sdkFake([]), s);
  assert.equal(await run(job(), { ...d, env: {} }), 1);
  assert.match((s.e[0] as any).text, /no Model/);
});
