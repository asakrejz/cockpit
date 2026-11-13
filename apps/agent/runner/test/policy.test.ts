import assert from "node:assert/strict";
import { test } from "node:test";
import { decide, globFits, repoRelative } from "../src/policy.ts";

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

test("writing only in the repo, not in .git and not to forbidden paths", () => {
  assert.equal(decide(r, "Edit", { file_path: "/work/src/lib.rs" }).behavior, "allow");
  assert.equal(decide(r, "Write", { file_path: "src/neu.rs" }).behavior, "allow");
  assert.equal(decide(r, "Write", { file_path: "/etc/passwd" }).behavior, "deny");
  assert.equal(decide(r, "Write", { file_path: "/work/../etc/passwd" }).behavior, "deny");
  assert.equal(decide(r, "Edit", { file_path: "/work/.git/config" }).behavior, "deny");
  const d = decide(r, "Edit", { file_path: "/work/Cargo.lock" });
  assert.equal(d.behavior, "deny");
  assert.match(d.message ?? "", /verboten/);
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
