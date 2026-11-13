// Was der Agent im Container darf. Das SDK fragt vor jedem Werkzeug (`canUseTool`); hier steht die
// Antwort. Die maßgebliche Prüfung der geänderten Dateien macht danach das Backend (Regeln des
// Repos) – das hier hält den Agenten nur früh von Fehlern ab.

import { isAbsolute, relative, resolve } from "node:path";

/** `*` = beliebig innerhalb eines Ordnernamens, `?` = ein Zeichen, `**` = beliebig viele Ordner;
 * ein Muster ohne `/` gilt in jedem Ordner, ein `/` am Ende meint den ganzen Ordner
 * (gleiche Regeln wie `glob_match` in cockpit-core). */
export function globFits(muster: string, path: string): boolean {
  let m = muster.trim().replace(/^\.\//, "").replace(/^\//, "");
  if (!m) return false;
  if (m.endsWith("/")) m += "**";
  if (!m.includes("/")) m = `**/${m}`;
  const p = path.trim().replace(/^\.\//, "").replace(/^\//, "");
  const segs = m.split("/");
  const parts = p.split("/");
  const name = (mu: string, n: string): boolean => {
    const re = new RegExp(
      `^${mu.replace(/[.+^${}()|[\]\\]/g, "\\$&").replace(/\*/g, "[^/]*").replace(/\?/g, "[^/]")}$`,
    );
    return re.test(n);
  };
  const rec = (i: number, j: number): boolean => {
    if (i === segs.length) return j === parts.length;
    if (segs[i] === "**") {
      for (let skip = 0; j + skip <= parts.length; skip++) if (rec(i + 1, j + skip)) return true;
      return false;
    }
    return j < parts.length && name(segs[i], parts[j]) && rec(i + 1, j + 1);
  };
  return rec(0, 0);
}

export interface Decision {
  behavior: "allow" | "deny";
  message?: string;
}

export interface Policy {
  wurzel: string;
  allowed: string[];
  forbidden: string[];
}

const Writing = new Set(["Write", "Edit", "MultiEdit", "NotebookEdit"]);

/** Pfad relativ zum Repo, oder `null`, wenn er außerhalb liegt. */
export function relative(wurzel: string, path: string): string | null {
  const abs = isAbsolute(path) ? resolve(path) : resolve(wurzel, path);
  const rel = relative(resolve(wurzel), abs);
  if (rel === "" || rel.startsWith("..") || isAbsolute(rel)) return null;
  return rel.split("\\").join("/");
}

/** Befehle, die der Agent nie ausführen soll: Netz, Git-Fernbefehle, Hauptbranch, Rechte, Aufräumen außerhalb. */
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
    const rel = relative(r.wurzel, raw);
    if (rel === null) return { behavior: "deny", message: `Außerhalb von ${r.wurzel} darf der Agent nichts ändern.` };
    if (rel === ".git" || rel.startsWith(".git/")) {
      return { behavior: "deny", message: "Das .git-Verzeichnis fasst der Agent nicht an." };
    }
    for (const v of r.forbidden) {
      if (globFits(v, rel)) return { behavior: "deny", message: `„${rel}“ ist laut Regel „${v}“ verboten.` };
    }
    if (r.allowed.length && !r.allowed.some((e) => globFits(e, rel))) {
      return { behavior: "deny", message: `„${rel}“ liegt außerhalb der erlaubten Pfade (${r.allowed.join(", ")}).` };
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
    if (typeof raw === "string" && raw !== "" && relative(r.wurzel, raw) === null && resolve(raw) !== resolve(r.wurzel)) {
      return { behavior: "deny", message: `Außerhalb von ${r.wurzel} liest der Agent nichts.` };
    }
    return { behavior: "allow" };
  }
  if (tool === "mcp__cockpit__rueckfrage" || tool === "TodoWrite") return { behavior: "allow" };
  return { behavior: "deny", message: `Das Werkzeug „${tool}“ ist im Cockpit nicht freigegeben.` };
}
