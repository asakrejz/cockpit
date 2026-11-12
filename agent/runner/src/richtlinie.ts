// Was der Agent im Container darf. Das SDK fragt vor jedem Werkzeug (`canUseTool`); hier steht die
// Antwort. Die maßgebliche Prüfung der geänderten Dateien macht danach das Backend (Regeln des
// Repos) – das hier hält den Agenten nur früh von Fehlern ab.

import { isAbsolute, relative, resolve } from "node:path";

/** `*` = beliebig innerhalb eines Ordnernamens, `?` = ein Zeichen, `**` = beliebig viele Ordner;
 * ein Muster ohne `/` gilt in jedem Ordner, ein `/` am Ende meint den ganzen Ordner
 * (gleiche Regeln wie `glob_match` in cockpit-core). */
export function globPasst(muster: string, pfad: string): boolean {
  let m = muster.trim().replace(/^\.\//, "").replace(/^\//, "");
  if (!m) return false;
  if (m.endsWith("/")) m += "**";
  if (!m.includes("/")) m = `**/${m}`;
  const p = pfad.trim().replace(/^\.\//, "").replace(/^\//, "");
  const segs = m.split("/");
  const teile = p.split("/");
  const name = (mu: string, n: string): boolean => {
    const re = new RegExp(
      `^${mu.replace(/[.+^${}()|[\]\\]/g, "\\$&").replace(/\*/g, "[^/]*").replace(/\?/g, "[^/]")}$`,
    );
    return re.test(n);
  };
  const rec = (i: number, j: number): boolean => {
    if (i === segs.length) return j === teile.length;
    if (segs[i] === "**") {
      for (let skip = 0; j + skip <= teile.length; skip++) if (rec(i + 1, j + skip)) return true;
      return false;
    }
    return j < teile.length && name(segs[i], teile[j]) && rec(i + 1, j + 1);
  };
  return rec(0, 0);
}

export interface Entscheidung {
  behavior: "allow" | "deny";
  message?: string;
}

export interface Richtlinie {
  wurzel: string;
  erlaubt: string[];
  verboten: string[];
}

const SCHREIBEND = new Set(["Write", "Edit", "MultiEdit", "NotebookEdit"]);

/** Pfad relativ zum Repo, oder `null`, wenn er außerhalb liegt. */
export function relativ(wurzel: string, pfad: string): string | null {
  const abs = isAbsolute(pfad) ? resolve(pfad) : resolve(wurzel, pfad);
  const rel = relative(resolve(wurzel), abs);
  if (rel === "" || rel.startsWith("..") || isAbsolute(rel)) return null;
  return rel.split("\\").join("/");
}

/** Befehle, die der Agent nie ausführen soll: Netz, Git-Fernbefehle, Hauptbranch, Rechte, Aufräumen außerhalb. */
const VERBOTENE_BEFEHLE: [RegExp, string][] = [
  [/\bgit\b[^;&|]*\b(push|remote|fetch|pull|clone|merge|rebase|reset\s+--hard|config|credential|worktree|submodule)\b/, "Git-Befehle, die ins Netz gehen oder Verlauf umschreiben, sind nicht erlaubt"],
  [/\bgit\s+(-\S+\s+)*(checkout|switch)\s+(-\S+\s+)*(main|master|develop)\b/, "Auf den Hauptbranch wechselt der Agent nie"],
  [/\b(curl|wget|nc|ncat|ssh|scp|sftp|ftp|telnet)\b/, "Netzwerkzugriffe sind nicht erlaubt"],
  [/\b(sudo|su|chmod\s+-R|chown)\b/, "Rechte ändert der Agent nicht"],
  [/\brm\s+(-\S+\s+)*\/(\s|$)/, "Aufräumen außerhalb von /work ist nicht erlaubt"],
  [/(^|[\s;&|])\.git\//, "Das .git-Verzeichnis fasst der Agent nicht an"],
  [/\b(printenv|env)\b|\$\{?(ANTHROPIC_AUTH_TOKEN|ANTHROPIC_API_KEY)/, "Zugangsdaten liest der Agent nicht aus"],
];

export function entscheide(r: Richtlinie, werkzeug: string, eingabe: Record<string, unknown>): Entscheidung {
  if (SCHREIBEND.has(werkzeug)) {
    const roh = String(eingabe.file_path ?? eingabe.notebook_path ?? eingabe.path ?? "");
    const rel = relativ(r.wurzel, roh);
    if (rel === null) return { behavior: "deny", message: `Außerhalb von ${r.wurzel} darf der Agent nichts ändern.` };
    if (rel === ".git" || rel.startsWith(".git/")) {
      return { behavior: "deny", message: "Das .git-Verzeichnis fasst der Agent nicht an." };
    }
    for (const v of r.verboten) {
      if (globPasst(v, rel)) return { behavior: "deny", message: `„${rel}“ ist laut Regel „${v}“ verboten.` };
    }
    if (r.erlaubt.length && !r.erlaubt.some((e) => globPasst(e, rel))) {
      return { behavior: "deny", message: `„${rel}“ liegt außerhalb der erlaubten Pfade (${r.erlaubt.join(", ")}).` };
    }
    return { behavior: "allow" };
  }
  if (werkzeug === "Bash") {
    const befehl = String(eingabe.command ?? "");
    for (const [re, warum] of VERBOTENE_BEFEHLE) {
      if (re.test(befehl)) return { behavior: "deny", message: warum };
    }
    return { behavior: "allow" };
  }
  if (werkzeug === "Read" || werkzeug === "Glob" || werkzeug === "Grep" || werkzeug === "LS") {
    const roh = eingabe.file_path ?? eingabe.path;
    if (typeof roh === "string" && roh !== "" && relativ(r.wurzel, roh) === null && resolve(roh) !== resolve(r.wurzel)) {
      return { behavior: "deny", message: `Außerhalb von ${r.wurzel} liest der Agent nichts.` };
    }
    return { behavior: "allow" };
  }
  if (werkzeug === "mcp__cockpit__rueckfrage" || werkzeug === "TodoWrite") return { behavior: "allow" };
  return { behavior: "deny", message: `Das Werkzeug „${werkzeug}“ ist im Cockpit nicht freigegeben.` };
}
