// Ereignisse des Läufers: JSON-Zeilen auf stdout. Das Backend liest sie zeilenweise und prüft sie
// (unbekannte oder zu große Zeilen verwirft es). Geheimnisse gehören nie hinein.

export type Ereignis =
  | { t: "plan"; summary: string; files: string[]; steps: string[] }
  | { t: "schritt"; titel: string; text: string; token_ein: number; token_aus: number; ms: number }
  | { t: "dateien"; dateien: string[] }
  | { t: "frage"; text: string; vorschlaege: string[] }
  | { t: "hinweise_angekommen" }
  | { t: "ergebnis"; text: string; branch: string; commit: string }
  | { t: "pruefung"; erreichbar: boolean; key_ok: boolean; hinweis: string }
  | { t: "fehler"; text: string };

/** Längste Zeichenkette eines Feldes; das Backend verwirft Zeilen über 64 KiB. */
export const MAX_TEXT = 2000;
const MAX_LISTE = 200;

export function kuerzen(text: string, max = MAX_TEXT): string {
  const t = String(text ?? "");
  return t.length <= max ? t : `${t.slice(0, max - 1)}…`;
}

function liste(werte: unknown): string[] {
  return Array.isArray(werte) ? werte.slice(0, MAX_LISTE).map((w) => kuerzen(String(w), 300)) : [];
}

/** Macht aus einem Ereignis eine Zeile ohne Zeilenumbruch (JSON escaped sie) und kürzt lange Felder. */
export function zeile(e: Ereignis): string {
  const sauber: Record<string, unknown> = {};
  for (const [k, v] of Object.entries(e)) {
    if (Array.isArray(v)) sauber[k] = liste(v);
    else if (typeof v === "string") sauber[k] = kuerzen(v);
    else sauber[k] = v;
  }
  return JSON.stringify(sauber);
}

export type Senke = (e: Ereignis) => void;

/** Schreibt Ereignisse auf stdout (eine Zeile je Ereignis). */
export function stdoutSenke(schreibe: (s: string) => void = (s) => process.stdout.write(s)): Senke {
  return (e) => schreibe(`${zeile(e)}\n`);
}
