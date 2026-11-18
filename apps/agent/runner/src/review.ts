// Review mode: pulls the draft comments out of the model's answer and reports them as events. The
// runner only reports; the backend checks every line against the diff and keeps the drafts for the person
// to decide. Nothing here talks to GitHub.

import { type Sink, truncate } from "./event.ts";
import type { Job } from "./job.ts";

/** At most this many drafts leave the runner (the backend limits again). */
export const MAX_COMMENTS = 40;

export interface Draft {
  path: string;
  line: number;
  start_line?: number;
  side: "RIGHT" | "LEFT";
  text: string;
  suggestion?: string;
}

function whole(x: unknown): number | null {
  const n = typeof x === "string" && /^\d+$/.test(x.trim()) ? Number(x) : x;
  return typeof n === "number" && Number.isInteger(n) && n >= 1 && n <= 10_000_000 ? n : null;
}

/** Reads one entry; `null` for anything that is not a usable comment. */
export function draftFrom(raw: unknown): Draft | null {
  if (typeof raw !== "object" || raw === null) return null;
  const c = raw as Record<string, unknown>;
  const path = typeof c.path === "string" ? c.path.trim().replace(/^[ab]\//, "") : "";
  const line = whole(c.line);
  const side = String(c.side ?? "RIGHT").trim().toUpperCase();
  const text = typeof c.text === "string" ? c.text.trim() : "";
  const suggestion = typeof c.suggestion === "string" && c.suggestion.trim() !== "" ? c.suggestion.replace(/\n+$/, "") : undefined;
  if (!path || path.length > 300 || line === null || (side !== "RIGHT" && side !== "LEFT")) return null;
  if (!text && !suggestion) return null;
  const start = whole(c.start_line);
  const d: Draft = { path, line, side, text: truncate(text, 2000) };
  if (start !== null && start < line) d.start_line = start;
  // A replacement only makes sense for lines of the new file.
  if (suggestion && side === "RIGHT") d.suggestion = truncate(suggestion, 2000);
  return d;
}

/** Pulls summary and comments (JSON block) out of the answer; without a block the answer becomes the summary. */
export function reviewFromAnswer(text: string): { summary: string; comments: Draft[] } {
  const block = /```json\s*([\s\S]*?)```/i.exec(text)?.[1] ?? /(\{[\s\S]*\})/.exec(text)?.[1];
  if (block) {
    try {
      const j = JSON.parse(block);
      const comments = (Array.isArray(j.comments) ? j.comments : [])
        .map(draftFrom)
        .filter((d: Draft | null): d is Draft => d !== null)
        .slice(0, MAX_COMMENTS);
      return { summary: truncate(String(j.summary ?? "").trim(), 4000), comments };
    } catch {
      /* falls back to the answer below */
    }
  }
  return { summary: truncate(text.trim(), 1500), comments: [] };
}

/** Reports the result of a review run: the drafts, the summary, then the end of the run. */
export function finishReview(sink: Sink, job: Job, answer: string): void {
  const r = reviewFromAnswer(answer);
  for (const c of r.comments) sink({ t: "review_comment", ...c });
  if (r.summary) sink({ t: "review_summary", text: r.summary });
  const n = r.comments.length;
  sink({
    t: "result",
    text: n === 0 ? "Nichts zu beanstanden." : `${n} ${n === 1 ? "Kommentar" : "Kommentare"} als Entwurf vorbereitet.`,
    branch: job.branch,
    commit: "",
  });
}
