// Ereignisse des Läufers: JSON-Zeilen auf stdout. Das Backend liest sie zeilenweise und prüft sie
// (unbekannte oder zu große Zeilen verwirft es). Geheimnisse gehören nie hinein.

export type Event =
  | { t: "plan"; summary: string; files: string[]; steps: string[] }
  | { t: "step"; title: string; text: string; tokens_in: number; tokens_out: number; ms: number }
  | { t: "files"; files: string[] }
  | { t: "question"; text: string; suggestions: string[] }
  | { t: "hints_arrived" }
  | { t: "result"; text: string; branch: string; commit: string }
  | { t: "check"; reachable: boolean; key_ok: boolean; hint: string }
  | { t: "error"; text: string };

/** Längste Zeichenkette eines Feldes; das Backend verwirft Zeilen über 64 KiB. */
export const MAX_TEXT = 2000;
const MAX_LIST = 200;

export function truncate(text: string, max = MAX_TEXT): string {
  const t = String(text ?? "");
  return t.length <= max ? t : `${t.slice(0, max - 1)}…`;
}

function list(values: unknown): string[] {
  return Array.isArray(values) ? values.slice(0, MAX_LIST).map((w) => truncate(String(w), 300)) : [];
}

/** Macht aus einem Ereignis eine Zeile ohne Zeilenumbruch (JSON escaped sie) und kürzt lange Felder. */
export function line(e: Event): string {
  const clean: Record<string, unknown> = {};
  for (const [k, v] of Object.entries(e)) {
    if (Array.isArray(v)) clean[k] = list(v);
    else if (typeof v === "string") clean[k] = truncate(v);
    else clean[k] = v;
  }
  return JSON.stringify(clean);
}

export type Sink = (e: Event) => void;

/** Schreibt Ereignisse auf stdout (eine Zeile je Ereignis). */
export function stdoutSink(write: (s: string) => void = (s) => process.stdout.write(s)): Sink {
  return (e) => write(`${line(e)}\n`);
}
