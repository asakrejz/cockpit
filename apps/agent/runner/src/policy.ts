// What the agent may do in the container. The SDK asks before every tool (`canUseTool`); this is the
// answer. The authoritative check of the changed files is done afterwards by the backend (rules of the
// repo) – this only keeps the agent from mistakes early.

import { isAbsolute, relative, resolve } from "node:path";

/** `*` = anything within a folder name, `?` = one character, `**` = any number of folders;
 * a pattern without `/` applies in every folder, a `/` at the end means the whole folder
 * (same rules as `glob_match` in cockpit-core). */
export function globFits(pattern: string, path: string): boolean {
  let m = pattern.trim().replace(/^\.\//, "").replace(/^\//, "");
  if (!m) return false;
  if (m.endsWith("/")) m += "**";
  if (!m.includes("/")) m = `**/${m}`;
  const p = path.trim().replace(/^\.\//, "").replace(/^\//, "");
  const segments = m.split("/");
  const parts = p.split("/");
  const name = (mu: string, n: string): boolean => {
    const re = new RegExp(
      `^${mu.replace(/[.+^${}()|[\]\\]/g, "\\$&").replace(/\*/g, "[^/]*").replace(/\?/g, "[^/]")}$`,
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
const FORBIDDEN_COMMANDS: [RegExp, string][] = [
  [/\bgit\b[^;&|]*\b(push|remote|fetch|pull|clone|merge|rebase|reset\s+--hard|config|credential|worktree|submodule)\b/, "Git-Befehle, die ins Netz gehen oder Verlauf umschreiben, sind nicht erlaubt"],
  [/\bgit\s+(-\S+\s+)*(checkout|switch)\s+(-\S+\s+)*(main|master|develop)\b/, "Auf den Hauptbranch wechselt der Agent nie"],
  [/\b(curl|wget|nc|ncat|ssh|scp|sftp|ftp|telnet)\b/, "Netzwerkzugriffe sind nicht erlaubt"],
  [/\b(sudo|su|chmod\s+-R|chown)\b/, "Rechte ändert der Agent nicht"],
  [/\brm\s+(-\S+\s+)*\/(\s|$)/, "Aufräumen außerhalb von /work ist nicht erlaubt"],
  [/(^|[\s;&|])\.git\//, "Das .git-Verzeichnis fasst der Agent nicht an"],
  [/\b(printenv|env)\b|\$\{?(ANTHROPIC_AUTH_TOKEN|ANTHROPIC_API_KEY)/, "Zugangsdaten liest der Agent nicht aus"],
];

export function decide(r: Policy, tool: string, input: Record<string, unknown>): Decision {
  if (Writing.has(tool)) {
    const raw = String(input.file_path ?? input.notebook_path ?? input.path ?? "");
    const rel = repoRelative(r.root, raw);
    if (rel === null) return { behavior: "deny", message: `Außerhalb von ${r.root} darf der Agent nichts ändern.` };
    const sc = scope(r, rel);
    if (sc === null) {
      return { behavior: "deny", message: `„${rel}“ liegt in keinem der Repos dieses Auftrags.` };
    }
    if (sc.rel === ".git" || sc.rel.startsWith(".git/") || sc.rel.includes("/.git/")) {
      return { behavior: "deny", message: "Das .git-Verzeichnis fasst der Agent nicht an." };
    }
    for (const v of sc.forbidden) {
      if (globFits(v, sc.rel)) return { behavior: "deny", message: `„${rel}“ ist laut Regel „${v}“ verboten.` };
    }
    if (sc.allowed.length && !sc.allowed.some((e) => globFits(e, sc.rel))) {
      return { behavior: "deny", message: `„${rel}“ liegt außerhalb der erlaubten Pfade (${sc.allowed.join(", ")}).` };
    }
    return { behavior: "allow" };
  }
  if (tool === "Bash") {
    const command = String(input.command ?? "");
    for (const [re, why] of FORBIDDEN_COMMANDS) {
      if (re.test(command)) return { behavior: "deny", message: why };
    }
    return { behavior: "allow" };
  }
  if (tool === "Read" || tool === "Glob" || tool === "Grep" || tool === "LS") {
    const raw = input.file_path ?? input.path;
    if (typeof raw === "string" && raw !== "" && repoRelative(r.root, raw) === null && resolve(raw) !== resolve(r.root)) {
      return { behavior: "deny", message: `Außerhalb von ${r.root} liest der Agent nichts.` };
    }
    return { behavior: "allow" };
  }
  if (tool === "mcp__cockpit__ask_user" || tool === "TodoWrite") return { behavior: "allow" };
  return { behavior: "deny", message: `Das Werkzeug „${tool}“ ist im Cockpit nicht freigegeben.` };
}

/** Why a changed file (path relative to the repo) is not allowed, or `null` if it is fine. This is the check after
 * the run for tools that cannot enforce the path rules themselves (Claude Code and Codex CLI). */
export function pathViolation(r: Policy, rel: string): string | null {
  if (rel.startsWith("../") || isAbsolute(rel)) return `„${rel}“ liegt außerhalb von ${r.root}.`;
  const sc = scope(r, rel);
  if (sc === null) return `„${rel}“ liegt in keinem der Repos dieses Auftrags.`;
  if (sc.rel === ".git" || sc.rel.startsWith(".git/") || sc.rel.includes("/.git/")) {
    return `„${rel}“: Das .git-Verzeichnis darf nicht verändert werden.`;
  }
  for (const v of sc.forbidden) {
    if (globFits(v, sc.rel)) return `„${rel}“ ist laut Regel „${v}“ verboten.`;
  }
  if (sc.allowed.length && !sc.allowed.some((e) => globFits(e, sc.rel))) {
    return `„${rel}“ liegt außerhalb der erlaubten Pfade (${sc.allowed.join(", ")}).`;
  }
  return null;
}

/** The policy of a job: one repo directly in the root, or the rules of each repo of a job in several repos. */
export function policyOf(
  root: string,
  job: { rules: { allowed_paths: string[]; forbidden_paths: string[] }; repos: Array<{ dir: string; rules: { allowed_paths: string[]; forbidden_paths: string[] } }> },
): Policy {
  if (job.repos.length > 1) {
    return {
      root,
      allowed: [],
      forbidden: [],
      repos: job.repos.map((x) => ({ dir: x.dir, allowed: x.rules.allowed_paths, forbidden: x.rules.forbidden_paths })),
    };
  }
  return { root, allowed: job.rules.allowed_paths, forbidden: job.rules.forbidden_paths };
}

/** All violations among the changed files. */
export function violations(r: Policy, files: string[]): string[] {
  return files.map((f) => pathViolation(r, f)).filter((v): v is string => v !== null);
}
