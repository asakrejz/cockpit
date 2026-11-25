// What the agent may do in the container. The SDK asks before every tool (`canUseTool`); this is the
// answer. The authoritative check of the changed files is done afterwards by the backend (rules of the
// repo) – this only keeps the agent from mistakes early.

import { isAbsolute, relative, resolve } from "node:path";

import { type Lang, type TextId, tx } from "./texts.ts";

/** Drops blanks, every leading `./` and then every leading `/` (as `glob_match` does). */
function trimmed(w: string): string {
  return w.trim().replace(/^(?:\.\/)+/, "").replace(/^\/+/, "");
}

/** `*` = anything within a folder name, `?` = one character, `**` = any number of folders;
 * a pattern without `/` applies in every folder, a `/` at the end means the whole folder
 * (same rules as `glob_match` in cockpit-core; both are tested against `glob_cases.json` there). */
export function globFits(pattern: string, path: string): boolean {
  let m = trimmed(pattern);
  if (!m) return false;
  if (m.endsWith("/")) m += "**";
  if (!m.includes("/")) m = `**/${m}`;
  const segments = m.split("/");
  const parts = trimmed(path).split("/");
  const name = (glob: string, n: string): boolean => {
    // The `u` flag makes `?` one character (code point), as in Rust, not one UTF-16 unit.
    const re = new RegExp(
      `^${glob.replace(/[.+^${}()|[\]\\]/g, "\\$&").replace(/\*/g, "[^/]*").replace(/\?/g, "[^/]")}$`,
      "u",
    );
    return re.test(n);
  };
  const rec = (i: number, j: number): boolean => {
    if (i === segments.length) return j === parts.length;
    if (segments[i] === "**") {
      for (let skip = 0; j + skip <= parts.length; skip++) if (rec(i + 1, j + skip)) return true;
      return false;
    }
    return j < parts.length && name(segments[i], parts[j]) && rec(i + 1, j + 1);
  };
  return rec(0, 0);
}

export interface Decision {
  behavior: "allow" | "deny";
  message?: string;
}

export interface Policy {
  root: string;
  allowed: string[];
  forbidden: string[];
  /** A job in several repos: the rules of each repo, by its folder below the root. Then `allowed`/`forbidden` are unused. */
  repos?: Array<{ dir: string; allowed: string[]; forbidden: string[] }>;
  /** Language of the deny messages. Missing: the language of the job (`texts.ts`). */
  language?: Lang;
}

/** The rules that apply to a path (relative to the root) and the path inside its repo.
 * `null` if the path is in none of the repos of a job in several repos. */
export function scope(r: Policy, rel: string): { rel: string; allowed: string[]; forbidden: string[] } | null {
  if (!r.repos) return { rel, allowed: r.allowed, forbidden: r.forbidden };
  const [first, ...rest] = rel.split("/");
  const repo = r.repos.find((x) => x.dir === first);
  if (!repo || rest.length === 0) return null;
  return { rel: rest.join("/"), allowed: repo.allowed, forbidden: repo.forbidden };
}

const Writing = new Set(["Write", "Edit", "MultiEdit", "NotebookEdit"]);

/** Path relative to the repo, or `null` if it lies outside. */
export function repoRelative(root: string, path: string): string | null {
  const abs = isAbsolute(path) ? resolve(path) : resolve(root, path);
  const rel = relative(resolve(root), abs);
  if (rel === "" || rel.startsWith("..") || isAbsolute(rel)) return null;
  return rel.split("\\").join("/");
}

/** Commands the agent must never run: network, Git remote commands, main branch, permissions, cleaning up outside. */
const FORBIDDEN_COMMANDS: [RegExp, TextId][] = [
  [/\bgit\b[^;&|]*\b(push|remote|fetch|pull|clone|merge|rebase|reset\s+--hard|config|credential|worktree|submodule)\b/, "deny_command_git"],
  [/\bgit\s+(-\S+\s+)*(checkout|switch)\s+(-\S+\s+)*(main|master|develop)\b/, "deny_command_main"],
  [/\b(curl|wget|nc|ncat|ssh|scp|sftp|ftp|telnet)\b/, "deny_command_network"],
  [/\b(sudo|su|chmod\s+-R|chown)\b/, "deny_command_rights"],
  [/\brm\s+(-\S+\s+)*\/(\s|$)/, "deny_command_rm"],
  [/(^|[\s;&|])\.git\//, "deny_command_git_dir"],
  [/\b(printenv|env)\b|\$\{?(ANTHROPIC_AUTH_TOKEN|ANTHROPIC_API_KEY)/, "deny_command_secrets"],
];

/** Why the agent must not change the file `path` (relative to the root, inside it), or `null` if it may.
 * One rule set for the check before a tool call (`decide`) and after the run (`pathViolation`). */
function changeDenial(r: Policy, path: string): string | null {
  const sc = scope(r, path);
  if (sc === null) return tx("deny_no_repo", { path }, r.language);
  if (sc.rel === ".git" || sc.rel.startsWith(".git/") || sc.rel.includes("/.git/")) {
    return tx("deny_git_dir", { path }, r.language);
  }
  for (const rule of sc.forbidden) {
    if (globFits(rule, sc.rel)) return tx("deny_forbidden", { path, rule }, r.language);
  }
  if (sc.allowed.length && !sc.allowed.some((e) => globFits(e, sc.rel))) {
    return tx("deny_not_allowed", { path, allowed: sc.allowed.join(", ") }, r.language);
  }
  return null;
}

export function decide(r: Policy, tool: string, input: Record<string, unknown>): Decision {
  if (Writing.has(tool)) {
    const raw = String(input.file_path ?? input.notebook_path ?? input.path ?? "");
    const rel = repoRelative(r.root, raw);
    const why = rel === null ? tx("deny_outside", { path: raw, root: r.root }, r.language) : changeDenial(r, rel);
    return why === null ? { behavior: "allow" } : { behavior: "deny", message: why };
  }
  if (tool === "Bash") {
    const command = String(input.command ?? "");
    for (const [re, why] of FORBIDDEN_COMMANDS) {
      if (re.test(command)) return { behavior: "deny", message: tx(why, {}, r.language) };
    }
    return { behavior: "allow" };
  }
  if (tool === "Read" || tool === "Glob" || tool === "Grep" || tool === "LS") {
    const raw = input.file_path ?? input.path;
    if (typeof raw === "string" && raw !== "" && repoRelative(r.root, raw) === null && resolve(raw) !== resolve(r.root)) {
      return { behavior: "deny", message: tx("deny_outside", { path: raw, root: r.root }, r.language) };
    }
    return { behavior: "allow" };
  }
  if (tool === "mcp__cockpit__ask_user" || tool === "TodoWrite") return { behavior: "allow" };
  return { behavior: "deny", message: tx("deny_tool", { tool }, r.language) };
}

/** Why a changed file (path relative to the repo) is not allowed, or `null` if it is fine. This is the check after
 * the run for tools that cannot enforce the path rules themselves (Claude Code and Codex CLI). */
export function pathViolation(r: Policy, rel: string): string | null {
  if (rel.startsWith("../") || isAbsolute(rel)) return tx("deny_outside", { path: rel, root: r.root }, r.language);
  return changeDenial(r, rel);
}

/** The policy of a job: one repo directly in the root, or the rules of each repo of a job in several repos. */
export function policyOf(
  root: string,
  job: {
    rules: { allowed_paths: string[]; forbidden_paths: string[] };
    repos: Array<{ dir: string; rules: { allowed_paths: string[]; forbidden_paths: string[] } }>;
    language?: Lang;
  },
): Policy {
  if (job.repos.length > 1) {
    return {
      root,
      allowed: [],
      forbidden: [],
      repos: job.repos.map((x) => ({ dir: x.dir, allowed: x.rules.allowed_paths, forbidden: x.rules.forbidden_paths })),
      language: job.language,
    };
  }
  return { root, allowed: job.rules.allowed_paths, forbidden: job.rules.forbidden_paths, language: job.language };
}

/** All violations among the changed files. */
export function violations(r: Policy, files: string[]): string[] {
  return files.map((f) => pathViolation(r, f)).filter((v): v is string => v !== null);
}
