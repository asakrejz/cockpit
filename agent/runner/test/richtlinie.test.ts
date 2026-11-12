import assert from "node:assert/strict";
import { test } from "node:test";
import { entscheide, globPasst, relativ } from "../src/richtlinie.ts";

const r = { wurzel: "/work", erlaubt: [], verboten: ["*.lock", ".github/", "styles/*.css"] };

test("Muster wie in cockpit-core", () => {
  assert.ok(globPasst("*.lock", "Cargo.lock"));
  assert.ok(globPasst("*.lock", "a/b/yarn.lock"));
  assert.ok(globPasst(".github/", ".github/workflows/ci.yml"));
  assert.ok(globPasst("styles/*.css", "styles/tokens.css"));
  assert.ok(!globPasst("styles/*.css", "a/styles/tokens.css"));
  assert.ok(globPasst("src/**/mod.rs", "src/mod.rs"));
  assert.ok(globPasst("src/**/mod.rs", "src/a/b/mod.rs"));
  assert.ok(!globPasst("", "x"));
});

test("Schreiben nur im Repo, nicht in .git und nicht auf verbotene Pfade", () => {
  assert.equal(entscheide(r, "Edit", { file_path: "/work/src/lib.rs" }).behavior, "allow");
  assert.equal(entscheide(r, "Write", { file_path: "src/neu.rs" }).behavior, "allow");
  assert.equal(entscheide(r, "Write", { file_path: "/etc/passwd" }).behavior, "deny");
  assert.equal(entscheide(r, "Write", { file_path: "/work/../etc/passwd" }).behavior, "deny");
  assert.equal(entscheide(r, "Edit", { file_path: "/work/.git/config" }).behavior, "deny");
  const d = entscheide(r, "Edit", { file_path: "/work/Cargo.lock" });
  assert.equal(d.behavior, "deny");
  assert.match(d.message ?? "", /verboten/);
});

test("erlaubte Pfade schränken ein", () => {
  const e = { ...r, erlaubt: ["src/"] };
  assert.equal(entscheide(e, "Edit", { file_path: "/work/src/a.rs" }).behavior, "allow");
  assert.equal(entscheide(e, "Edit", { file_path: "/work/README.md" }).behavior, "deny");
});

test("Bash: kein Push, kein Netz, kein Hauptbranch, keine Zugangsdaten", () => {
  const ok = (c: string) => entscheide(r, "Bash", { command: c }).behavior === "allow";
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

test("Lesen außerhalb des Repos und unbekannte Werkzeuge werden abgelehnt", () => {
  assert.equal(entscheide(r, "Read", { file_path: "/work/src/a.rs" }).behavior, "allow");
  assert.equal(entscheide(r, "Read", { file_path: "/home/node/.ssh/id" }).behavior, "deny");
  assert.equal(entscheide(r, "WebFetch", { url: "https://x" }).behavior, "deny");
  assert.equal(entscheide(r, "mcp__cockpit__rueckfrage", {}).behavior, "allow");
});

test("relativ löst Pfade auf", () => {
  assert.equal(relativ("/work", "/work/a/b"), "a/b");
  assert.equal(relativ("/work", "a/b"), "a/b");
  assert.equal(relativ("/work", "/work"), null);
  assert.equal(relativ("/work", "/workx/a"), null);
});
