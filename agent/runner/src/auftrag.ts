// Der Auftrag, den das Backend in eine Datei schreibt (`/auftrag/auftrag.json`, schreibgeschützt
// eingebunden). Er enthält nie Geheimnisse – Adresse, Key und Modell kommen über die Umgebung.

import { readFileSync } from "node:fs";

export type Modus = "plan" | "umsetzen" | "pruefung";

export interface Regeln {
  erlaubte_pfade: string[];
  verbotene_pfade: string[];
  pflicht_tests: string[];
  anweisungen: string[];
}

export interface Auftrag {
  modus: Modus;
  ticket: { key: string; titel: string; beschreibung: string; kommentare: string[] };
  hinweise: string[];
  plan_anmerkungen: string[];
  /** Der freigegebene Plan (nur im Modus „umsetzen“, wenn es einen gab). */
  plan: { summary: string; files: string[]; steps: string[] } | null;
  /** Was bei einem früheren Anlauf schon gesagt wurde (nach einem Fortsetzen). */
  bisher: string[];
  regeln: Regeln;
  tests_ausfuehren: boolean;
  branch: string;
  basis_branch: string;
  /** Höchstens so viele Sekunden darf der Läufer arbeiten. */
  max_sekunden: number;
}

export function leereRegeln(): Regeln {
  return { erlaubte_pfade: [], verbotene_pfade: [], pflicht_tests: [], anweisungen: [] };
}

function texte(w: unknown): string[] {
  return Array.isArray(w) ? w.filter((x) => typeof x === "string") : [];
}

/** Prüft und ergänzt einen eingelesenen Auftrag; wirft mit deutscher Meldung, wenn er nicht passt. */
export function pruefeAuftrag(roh: unknown): Auftrag {
  if (typeof roh !== "object" || roh === null) throw new Error("Der Auftrag ist kein Objekt.");
  const o = roh as Record<string, any>;
  if (!["plan", "umsetzen", "pruefung"].includes(o.modus)) {
    throw new Error(`Unbekannter Modus „${String(o.modus)}“.`);
  }
  const t = o.ticket ?? {};
  const r = o.regeln ?? {};
  const branch = String(o.branch ?? "");
  if (o.modus === "umsetzen" && !/^claude\/[A-Za-z0-9._\/-]+$/.test(branch)) {
    throw new Error(`Der Branch „${branch}“ muss mit „claude/“ beginnen.`);
  }
  return {
    modus: o.modus,
    ticket: {
      key: String(t.key ?? ""),
      titel: String(t.titel ?? ""),
      beschreibung: String(t.beschreibung ?? ""),
      kommentare: texte(t.kommentare),
    },
    hinweise: texte(o.hinweise),
    plan_anmerkungen: texte(o.plan_anmerkungen),
    plan: o.plan && typeof o.plan === "object"
      ? { summary: String(o.plan.summary ?? ""), files: texte(o.plan.files), steps: texte(o.plan.steps) }
      : null,
    bisher: texte(o.bisher),
    regeln: {
      erlaubte_pfade: texte(r.erlaubte_pfade),
      verbotene_pfade: texte(r.verbotene_pfade),
      pflicht_tests: texte(r.pflicht_tests),
      anweisungen: texte(r.anweisungen),
    },
    tests_ausfuehren: o.tests_ausfuehren !== false,
    branch,
    basis_branch: String(o.basis_branch ?? "main"),
    max_sekunden: Number.isFinite(o.max_sekunden) && o.max_sekunden > 0 ? Math.floor(o.max_sekunden) : 1800,
  };
}

export function leseAuftrag(pfad: string): Auftrag {
  let text: string;
  try {
    text = readFileSync(pfad, "utf8");
  } catch (e) {
    throw new Error(`Der Auftrag unter ${pfad} ist nicht lesbar: ${(e as Error).message}`);
  }
  return pruefeAuftrag(JSON.parse(text));
}

const REGELN_TEXT = (r: Regeln): string[] => {
  const z: string[] = [];
  if (r.erlaubte_pfade.length) z.push(`Du darfst nur diese Pfade ändern: ${r.erlaubte_pfade.join(", ")}.`);
  if (r.verbotene_pfade.length) z.push(`Diese Pfade darfst du nie ändern: ${r.verbotene_pfade.join(", ")}.`);
  if (r.pflicht_tests.length) z.push(`Führe vor dem Ende diese Tests aus: ${r.pflicht_tests.join(", ")}.`);
  for (const a of r.anweisungen) z.push(a);
  return z;
};

/** Feste Regeln des Cockpits, unabhängig vom Repo. */
export function systemText(a: Auftrag): string {
  return [
    "Du bist der Agent des Cockpits und arbeitest im Verzeichnis /work an einem geklonten Repository.",
    "Feste Regeln: Du arbeitest nur in /work. Du pushst nicht, mergst nicht und wechselst nicht auf den Hauptbranch;",
    `den Commit auf dem Branch ${a.branch || "(Branch des Auftrags)"} legt der Läufer selbst an, du musst nicht committen.`,
    "Du hast kein Netz außer dem Modellzugang. Antworte auf Deutsch.",
    ...REGELN_TEXT(a.regeln),
  ].join("\n");
}

function ticketText(a: Auftrag): string {
  const t = a.ticket;
  const z = [`Ticket ${t.key}: ${t.titel}`];
  if (t.beschreibung.trim()) z.push("", t.beschreibung.trim());
  if (t.kommentare.length) z.push("", "Kommentare am Ticket:", ...t.kommentare.map((k) => `- ${k}`));
  if (a.hinweise.length) z.push("", "Hinweise des Nutzers:", ...a.hinweise.map((h) => `- ${h}`));
  return z.join("\n");
}

/** Die erste Nachricht an das Modell. */
export function aufgabenText(a: Auftrag): string {
  if (a.modus === "plan") {
    const z = [
      ticketText(a),
      "",
      "Lies den Code (nur lesen, nichts ändern) und lege einen Plan vor. Antworte zuletzt mit genau einem JSON-Objekt",
      'in einem ```json-Block: {"summary": "ein Satz", "files": ["Pfade, die du ändern würdest"], "steps": ["Schritte"]}.',
    ];
    if (a.plan_anmerkungen.length) {
      z.push("", "Der Nutzer hat den letzten Plan beanstandet. Berücksichtige:", ...a.plan_anmerkungen.map((n) => `- ${n}`));
    }
    return z.join("\n");
  }
  const z = [ticketText(a), "", "Setze das Ticket um. Ändere nur, was dafür nötig ist."];
  if (a.plan) {
    z.push("", `Freigegebener Plan: ${a.plan.summary}`, ...a.plan.steps.map((s, i) => `${i + 1}. ${s}`));
  }
  if (a.tests_ausfuehren) z.push("", "Führe danach die Tests des Projekts aus und behebe Fehler, die du verursacht hast.");
  else z.push("", "Führe keine Tests aus.");
  z.push("Hast du eine Frage, die nur der Nutzer beantworten kann, stelle sie mit dem Werkzeug „rueckfrage“.");
  if (a.bisher.length) z.push("", "Bisher geschah (Lauf wurde fortgesetzt):", ...a.bisher.map((b) => `- ${b}`));
  return z.join("\n");
}
