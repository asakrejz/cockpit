// Events of the runner: JSON lines on stdout. The backend reads them line by line and checks them
// (it discards unknown or too large lines). Secrets never belong in them.

export type Event =
  | { t: "plan"; summary: string; files: string[]; steps: string[] }
  | { t: "step"; title: string; text: string; tokens_in: number; tokens_out: number; ms: number }
  | { t: "files"; files: string[] }
  | { t: "question"; text: string; suggestions: string[] }
  | { t: "hints_arrived" }
  | { t: "result"; text: string; branch: string; commit: string }
  | { t: "check"; reachable: boolean; key_ok: boolean; hint: string }
  | { t: "error"; text: string };

/** Longest string of a field; the backend discards lines over 64 KiB. */
export const MAX_TEXT = 2000;
const MAX_LIST = 200;

export function truncate(text: string, max = MAX_TEXT): string {
  const t = String(text ?? "");
  return t.length <= max ? t : `${t.slice(0, max - 1)}…`;
}

function list(values: unknown): string[] {
  return Array.isArray(values) ? values.slice(0, MAX_LIST).map((w) => truncate(String(w), 300)) : [];
}

/** Turns an event into a line without a line break (JSON escapes it) and truncates long fields. */
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

/** Writes events to stdout (one line per event). */
export function stdoutSink(write: (s: string) => void = (s) => process.stdout.write(s)): Sink {
  return (e) => write(`${line(e)}\n`);
}
