import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { hookAnswer } from "../src/hook.ts";
import { decide, globFits, pathViolation, policyOf, repoRelative } from "../src/policy.ts";

const r = { root: "/work", allowed: [], forbidden: ["*.lock", ".github/", "styles/*.css"] };

test("patterns as in cockpit-core", () => {
  assert.ok(globFits("*.lock", "Cargo.lock"));
  assert.ok(globFits("*.lock", "a/b/yarn.lock"));
  assert.ok(globFits(".github/", ".github/workflows/ci.yml"));
  assert.ok(globFits("styles/*.css", "styles/tokens.css"));
  assert.ok(!globFits("styles/*.css", "a/styles/tokens.css"));
  assert.ok(globFits("src/**/mod.rs", "src/mod.rs"));
  assert.ok(globFits("src/**/mod.rs", "src/a/b/mod.rs"));
  assert.ok(!globFits("", "x"));
});

// The same cases are checked against `glob_match` in crates/cockpit-core/tests/glob_cases.rs.
const GLOB_CASES: Array<{ pattern: string; path: string; expected: boolean }> = JSON.parse(
  readFileSync(new URL("../../../../crates/cockpit-core/tests/fixtures/glob_cases.json", import.meta.url), "utf8"),
);

test("globFits follows the cases shared with cockpit-core", () => {
  assert.ok(GLOB_CASES.length > 100, "the shared glob cases went missing");
  const wrong = GLOB_CASES.filter((c) => globFits(c.pattern, c.path) !== c.expected).map(
    (c) => `${JSON.stringify(c.pattern)} against ${JSON.stringify(c.path)}: expected ${c.expected}`,
  );
  assert.deepEqual(wrong, []);
});

test("writing only in the repo, not in .git and not to forbidden paths", () => {
  assert.equal(decide(r, "Edit", { file_path: "/work/src/lib.rs" }).behavior, "allow");
  assert.equal(decide(r, "Write", { file_path: "src/neu.rs" }).behavior, "allow");
  assert.equal(decide(r, "Write", { file_path: "/etc/passwd" }).behavior, "deny");
  assert.equal(decide(r, "Write", { file_path: "/work/../etc/passwd" }).behavior, "deny");
  assert.equal(decide(r, "Edit", { file_path: "/work/.git/config" }).behavior, "deny");
  const d = decide(r, "Edit", { file_path: "/work/Cargo.lock" });
  assert.equal(d.behavior, "deny");
  assert.match(d.message ?? "", /forbidden by the rule “\*\.lock”/);
});

test("allowed paths restrict", () => {
  const e = { ...r, allowed: ["src/"] };
  assert.equal(decide(e, "Edit", { file_path: "/work/src/a.rs" }).behavior, "allow");
  assert.equal(decide(e, "Edit", { file_path: "/work/README.md" }).behavior, "deny");
});

test("Bash: no push, no network, no main branch, no credentials", () => {
  const ok = (c: string) => decide(r, "Bash", { command: c }).behavior === "allow";
  assert.ok(ok("cargo test"));
  assert.ok(ok("git status && git diff"));
  assert.ok(!ok("git push origin HEAD"));
  assert.ok(!ok("git -C /work push"));
  assert.ok(!ok("git checkout main"));
  assert.ok(!ok("git remote add x https://x"));
  assert.ok(!ok("curl https://example.com"));
  assert.ok(!ok("cat .git/config"));
  assert.ok(!ok("echo $ANTHROPIC_AUTH_TOKEN"));
  assert.ok(!ok("printenv"));
});

test("reading outside the repo and unknown tools are rejected", () => {
  assert.equal(decide(r, "Read", { file_path: "/work/src/a.rs" }).behavior, "allow");
  assert.equal(decide(r, "Read", { file_path: "/home/node/.ssh/id" }).behavior, "deny");
  assert.equal(decide(r, "WebFetch", { url: "https://x" }).behavior, "deny");
  assert.equal(decide(r, "mcp__cockpit__ask_user", {}).behavior, "allow");
});

test("repoRelative resolves paths", () => {
  assert.equal(repoRelative("/work", "/work/a/b"), "a/b");
  assert.equal(repoRelative("/work", "a/b"), "a/b");
  assert.equal(repoRelative("/work", "/work"), null);
  assert.equal(repoRelative("/work", "/workx/a"), null);
});

test("deny messages are English by default and German for a German policy; one text for both checks", () => {
  const en = { root: "/work", allowed: ["src/"], forbidden: ["*.lock"] };
  const de = { ...en, language: "de" as const };
  const cases: Array<{ file: string; en: RegExp; de: RegExp }> = [
    { file: "/work/Cargo.lock", en: /forbidden by the rule “\*\.lock”/, de: /laut Regel „\*\.lock“ verboten/ },
    { file: "/work/README.md", en: /outside the allowed paths \(src\/\)/, de: /außerhalb der erlaubten Pfade \(src\/\)/ },
    { file: "/work/src/.git/config", en: /\.git directory must not be changed/, de: /\.git-Verzeichnis darf nicht verändert werden/ },
    { file: "/etc/passwd", en: /“\/etc\/passwd” is outside \/work/, de: /„\/etc\/passwd“ liegt außerhalb von \/work/ },
  ];
  for (const c of cases) {
    for (const [policy, pattern] of [[en, c.en], [de, c.de]] as const) {
      const said = decide(policy, "Edit", { file_path: c.file });
      assert.equal(said.behavior, "deny");
      assert.match(said.message ?? "", pattern);
      // After the run the same rule gives the same text for the path relative to the repo.
      const rel = c.file.startsWith("/work/") ? c.file.slice("/work/".length) : c.file;
      assert.equal(pathViolation(policy, rel), said.message === undefined ? null : said.message.replace(c.file, rel));
    }
  }
  const tool = (p: typeof en) => decide(p, "WebFetch", { url: "https://x" }).message;
  assert.equal(tool(en), "The tool “WebFetch” is not enabled in Cockpit.");
  assert.equal(tool(de), "Das Werkzeug „WebFetch“ ist im Cockpit nicht freigegeben.");
  const bash = (p: typeof en, command: string) => decide(p, "Bash", { command }).message;
  assert.equal(bash(en, "curl https://x"), "Network access is not allowed.");
  assert.equal(bash(de, "curl https://x"), "Netzwerkzugriffe sind nicht erlaubt.");
  assert.match(bash(en, "git push") ?? "", /^Git commands that use the network/);
  assert.match(bash(de, "git push") ?? "", /^Git-Befehle, die ins Netz gehen/);
  assert.match(decide(de, "Read", { file_path: "/etc/passwd" }).message ?? "", /liegt außerhalb von \/work/);
});

test("a policy that follows no repo says so, in the language of the policy", () => {
  const p = { root: "/work", allowed: [], forbidden: [], repos: [{ dir: "acme__app", allowed: [], forbidden: [] }], language: "de" as const };
  assert.match(pathViolation(p, "nope/a.md") ?? "", /liegt in keinem der Repos dieses Auftrags/);
  assert.match(pathViolation({ ...p, language: "en" }, "nope/a.md") ?? "", /is in none of the repos of this job/);
});

test("the policy of a job carries the language of the job, the hook uses it", () => {
  const job = { rules: { allowed_paths: [], forbidden_paths: ["*.lock"] }, repos: [] };
  assert.equal(policyOf("/work", { ...job, language: "de" }).language, "de");
  const out = (language: "de" | "en") =>
    JSON.parse(hookAnswer(policyOf("/work", { ...job, language }), JSON.stringify({ tool_name: "Edit", tool_input: { file_path: "/work/a.lock" } }))).hookSpecificOutput;
  assert.equal(out("en").permissionDecisionReason, "“a.lock” is forbidden by the rule “*.lock”.");
  assert.equal(out("de").permissionDecisionReason, "„a.lock“ ist laut Regel „*.lock“ verboten.");
});
