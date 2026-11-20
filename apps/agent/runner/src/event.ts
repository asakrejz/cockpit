// Events of the runner: JSON lines on stdout. The backend reads them line by line and checks them
// (it discards unknown or too large lines). Secrets never belong in them.

export type Event =
  | { t: "plan"; summary: string; files: string[]; steps: string[] }
  | { t: "step"; title: string; text: string; tokens_in: number; tokens_out: number; ms: number }
  | { t: "files"; files: string[] }
  | { t: "question"; text: string; suggestions: string[] }
  | { t: "hints_arrived" }
  // Review mode: draft comments on lines of the diff and a summary. The backend checks every line
  // against the diff and keeps them as drafts; nothing goes to GitHub from here.
  | { t: "review_comment"; path: string; line: number; start_line?: number; side: "RIGHT" | "LEFT"; text: string; suggestion?: string }
  | { t: "review_summary"; text: string }
  // `repos` only for a job in several repos: the branch and commit of each repo that has changes.
  | { t: "result"; text: string; branch: string; commit: string; repos?: Array<{ repo: string; branch: string; commit: string }> }
  | { t: "check"; reachable: boolean; key_ok: boolean; hint: string }
  | { t: "error"; text: string };

/** Longest string of a field; the backend discards lines over 64 KiB. */
export const MAX_TEXT = 2000;
const MAX_LIST = 200;
const MAX_REPOS = 10;

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
    if (Array.isArray(v) && v.every((x) => typeof x === "object" && x !== null)) {
      // Lists of objects (the repos of a result): the text fields are truncated, nothing else is let through.
      clean[k] = v.slice(0, MAX_REPOS).map((o) =>
        Object.fromEntries(Object.entries(o as Record<string, unknown>).map(([a, b]) => [a, typeof b === "string" ? truncate(b, 200) : b])),
      );
    } else if (Array.isArray(v)) clean[k] = list(v);
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
