// Der Auftrag, den das Backend in eine Datei schreibt (`/auftrag/auftrag.json`, schreibgeschützt
// eingebunden). Er enthält nie Geheimnisse – Adresse, Key und Modell kommen über die Umgebung.

import { readFileSync } from "node:fs";

export type Mode = "plan" | "implement" | "check";

export interface Rules {
  allowed_paths: string[];
  forbidden_paths: string[];
  required_tests: string[];
  instructions: string[];
}

export interface Job {
  mode: Mode;
  ticket: { key: string; title: string; description: string; comments: string[] };
  hints: string[];
  plan_notes: string[];
  /** Der freigegebene Plan (nur im Modus „umsetzen“, wenn es einen gab). */
  plan: { summary: string; files: string[]; steps: string[] } | null;
  /** Was bei einem früheren Anlauf schon gesagt wurde (nach einem Fortsetzen). */
  earlier: string[];
  rules: Rules;
  run_tests: boolean;
  branch: string;
  base_branch: string;
  /** Höchstens so viele Sekunden darf der Läufer arbeiten. */
  max_seconds: number;
}

export function emptyRules(): Rules {
  return { allowed_paths: [], forbidden_paths: [], required_tests: [], instructions: [] };
}

function texts(w: unknown): string[] {
  return Array.isArray(w) ? w.filter((x) => typeof x === "string") : [];
}

/** Prüft und ergänzt einen eingelesenen Auftrag; wirft mit deutscher Meldung, wenn er nicht passt. */
export function checkJob(raw: unknown): Job {
  if (typeof raw !== "object" || raw === null) throw new Error("Der Auftrag ist kein Objekt.");
  const o = raw as Record<string, any>;
  if (!["plan", "implement", "check"].includes(o.mode)) {
    throw new Error(`Unbekannter Modus „${String(o.mode)}“.`);
  }
  const t = o.ticket ?? {};
  const r = o.rules ?? {};
  const branch = String(o.branch ?? "");
  if (o.mode === "implement" && !/^claude\/[A-Za-z0-9._\/-]+$/.test(branch)) {
    throw new Error(`Der Branch „${branch}“ muss mit „claude/“ beginnen.`);
  }
  return {
    mode: o.mode,
    ticket: {
      key: String(t.key ?? ""),
      title: String(t.title ?? ""),
      description: String(t.description ?? ""),
      comments: texts(t.comments),
    },
    hints: texts(o.hints),
    plan_notes: texts(o.plan_notes),
    plan: o.plan && typeof o.plan === "object"
      ? { summary: String(o.plan.summary ?? ""), files: texts(o.plan.files), steps: texts(o.plan.steps) }
      : null,
    earlier: texts(o.earlier),
    rules: {
      allowed_paths: texts(r.allowed_paths),
      forbidden_paths: texts(r.forbidden_paths),
      required_tests: texts(r.required_tests),
      instructions: texts(r.instructions),
    },
    run_tests: o.run_tests !== false,
    branch,
    base_branch: String(o.base_branch ?? "main"),
    max_seconds: Number.isFinite(o.max_seconds) && o.max_seconds > 0 ? Math.floor(o.max_seconds) : 1800,
  };
}

export function readJob(path: string): Job {
  let text: string;
  try {
    text = readFileSync(path, "utf8");
  } catch (e) {
    throw new Error(`Der Auftrag unter ${path} ist nicht lesbar: ${(e as Error).message}`);
  }
  return checkJob(JSON.parse(text));
}

const RULES_TEXT = (r: Rules): string[] => {
  const z: string[] = [];
  if (r.allowed_paths.length) z.push(`Du darfst nur diese Pfade ändern: ${r.allowed_paths.join(", ")}.`);
  if (r.forbidden_paths.length) z.push(`Diese Pfade darfst du nie ändern: ${r.forbidden_paths.join(", ")}.`);
  if (r.required_tests.length) z.push(`Führe vor dem Ende diese Tests aus: ${r.required_tests.join(", ")}.`);
  for (const a of r.instructions) z.push(a);
  return z;
};

/** Feste Regeln des Cockpits, unabhängig vom Repo. */
export function systemText(a: Job): string {
  return [
    "Du bist der Agent des Cockpits und arbeitest im Verzeichnis /work an einem geklonten Repository.",
    "Feste Regeln: Du arbeitest nur in /work. Du pushst nicht, mergst nicht und wechselst nicht auf den Hauptbranch;",
    `den Commit auf dem Branch ${a.branch || "(Branch des Auftrags)"} legt der Läufer selbst an, du musst nicht committen.`,
    "Du hast kein Netz außer dem Modellzugang. Antworte auf Deutsch.",
    ...RULES_TEXT(a.rules),
  ].join("\n");
}

function ticketText(a: Job): string {
  const t = a.ticket;
  const z = [`Ticket ${t.key}: ${t.title}`];
  if (t.description.trim()) z.push("", t.description.trim());
  if (t.comments.length) z.push("", "Kommentare am Ticket:", ...t.comments.map((k) => `- ${k}`));
  if (a.hints.length) z.push("", "Hinweise des Nutzers:", ...a.hints.map((h) => `- ${h}`));
  return z.join("\n");
}

/** Die erste Nachricht an das Modell. */
export function tasksText(a: Job): string {
  if (a.mode === "plan") {
    const z = [
      ticketText(a),
      "",
      "Lies den Code (nur lesen, nichts ändern) und lege einen Plan vor. Antworte zuletzt mit genau einem JSON-Objekt",
      'in einem ```json-Block: {"summary": "ein Satz", "files": ["Pfade, die du ändern würdest"], "steps": ["Schritte"]}.',
    ];
    if (a.plan_notes.length) {
      z.push("", "Der Nutzer hat den letzten Plan beanstandet. Berücksichtige:", ...a.plan_notes.map((n) => `- ${n}`));
    }
    return z.join("\n");
  }
  const z = [ticketText(a), "", "Setze das Ticket um. Ändere nur, was dafür nötig ist."];
  if (a.plan) {
    z.push("", `Freigegebener Plan: ${a.plan.summary}`, ...a.plan.steps.map((s, i) => `${i + 1}. ${s}`));
  }
  if (a.run_tests) z.push("", "Führe danach die Tests des Projekts aus und behebe Fehler, die du verursacht hast.");
  else z.push("", "Führe keine Tests aus.");
  z.push("Hast du eine Frage, die nur der Nutzer beantworten kann, stelle sie mit dem Werkzeug „rueckfrage“.");
  if (a.earlier.length) z.push("", "Bisher geschah (Lauf wurde fortgesetzt):", ...a.earlier.map((b) => `- ${b}`));
  return z.join("\n");
}
