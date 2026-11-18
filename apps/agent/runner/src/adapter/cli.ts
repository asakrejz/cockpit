// The shared course of the CLI adapters (Claude Code, Codex): start the tool as a child process, read its JSON
// lines, translate them into events and, above all, check afterwards what it changed.
//
// What the CLI cannot enforce itself (path rules, `.git`, files outside the repo) is enforced here after the run: the
// changed files are checked against the rules of the job, a violation aborts the run with an `error` event and
// nothing is committed. Secrets never appear in arguments (the prompt goes through stdin, the keys through the
// environment) and are scrubbed from every event.

import { spawn } from "node:child_process";
import { createInterface } from "node:readline";
import { type Event, type Sink, truncate } from "../event.ts";
import { type Job, commitMessage } from "../job.ts";
import { changedFiles, record } from "../git.ts";
import { type Policy, violations } from "../policy.ts";
import { finishReview } from "../review.ts";
import { planFromAnswer } from "./claude-messages.ts";
import type { AdapterEnv } from "./interface.ts";

/** What a translator reports at the end of the stream. */
export interface Outcome {
  /** The final answer of the model. */
  text: string;
  /** Reason of a failed run reported by the tool itself. */
  error: string | null;
  /** The tool reported a final result (a stream that ends without one is cut off). */
  finished: boolean;
  /** Files the tool reported as changed (for the `files` events; the check uses Git). */
  files: string[];
}

/** One translator per run: gets the parsed JSON lines of the tool. */
export interface Translator {
  line(value: Record<string, any>): void;
  outcome(): Outcome;
}

export interface Spec {
  /** Display name, e.g. "Claude Code". */
  label: string;
  command: string;
  args: string[];
  /** The complete environment of the child process (nothing is inherited). */
  env: Record<string, string>;
  /** The prompt, written to stdin. */
  stdin: string;
  translator: Translator;
  /** Called at the end, e.g. to remove temporary files. */
  cleanup?: () => void;
}

/** Names of the environment variables of the parent that a CLI child may inherit besides its own endpoint values. */
const PASS_ON = ["PATH", "HOME", "LANG", "LC_ALL", "TERM", "TMPDIR", "TZ"];

/** The harmless base of the child environment: no keys, only what a program needs to start. */
export function baseEnv(env: Record<string, string | undefined>): Record<string, string> {
  const out: Record<string, string> = {};
  for (const n of PASS_ON) {
    const v = env[n];
    if (v !== undefined) out[n] = v;
  }
  return out;
}

/** The secret values of the environment (anything named like a key, token or secret). */
export function secretsOf(env: Record<string, string | undefined>): string[] {
  return Object.entries(env)
    .filter(([n, v]) => /KEY|TOKEN|SECRET|PASSWORD/i.test(n) && (v ?? "").length >= 8)
    .map(([, v]) => v as string);
}

/** A sink that removes secret values from everything that is reported. */
export function scrubbingSink(sink: Sink, secrets: string[]): Sink {
  if (secrets.length === 0) return sink;
  const forms = secrets.flatMap((s) => [s, JSON.stringify(s).slice(1, -1)]);
  return (e: Event) => {
    let text = JSON.stringify(e);
    for (const s of forms) text = text.split(s).join("[entfernt]");
    sink(JSON.parse(text) as Event);
  };
}

function scrub(text: string, secrets: string[]): string {
  let t = text;
  for (const s of secrets) t = t.split(s).join("[entfernt]");
  return t;
}

/** Runs the tool of `spec` and reports as the SDK adapter does. Exit code: 0 = reported, 1 = error event, 2 = tool missing. */
export async function runCli(job: Job, d: AdapterEnv, make: (sink: Sink) => Spec | string): Promise<number> {
  const secrets = secretsOf(d.env);
  const sink = scrubbingSink(d.sink, secrets);
  const spec = make(sink);
  if (typeof spec === "string") {
    sink({ t: "error", text: spec });
    return 1;
  }
  const policy: Policy = { root: d.root, allowed: job.rules.allowed_paths, forbidden: job.rules.forbidden_paths };
  const review = job.mode === "review";
  const plan = job.mode === "plan" || review;
  try {
    let head = "";
    try {
      head = (await d.git(["rev-parse", "HEAD"], d.root)).trim();
    } catch {
      /* no readable HEAD: the check below then only looks at the working tree */
    }

    const child = spawn(spec.command, spec.args, { cwd: d.root, env: spec.env, stdio: ["pipe", "pipe", "pipe"] });
    let missing = false;
    let timedOut = false;
    const stderrTail: string[] = [];
    let broken = 0;
    child.on("error", (e: NodeJS.ErrnoException) => {
      missing = e.code === "ENOENT";
      if (!missing) stderrTail.push(String(e.message));
    });
    const exited = new Promise<number | null>((ok) => {
      child.on("close", (code) => ok(code));
      child.on("error", () => ok(null));
    });
    createInterface({ input: child.stderr }).on("line", (z) => {
      stderrTail.push(z);
      if (stderrTail.length > 10) stderrTail.shift();
    });
    const lines = createInterface({ input: child.stdout });
    const read = (async () => {
      for await (const z of lines) {
        if (!z.trim()) continue;
        try {
          const v = JSON.parse(z);
          if (v && typeof v === "object" && !Array.isArray(v)) spec.translator.line(v);
          else broken++;
        } catch {
          broken++;
        }
      }
    })();
    child.stdin.on("error", () => {});
    child.stdin.end(spec.stdin);

    const deadline = setTimeout(() => {
      timedOut = true;
      child.kill("SIGTERM");
      setTimeout(() => child.kill("SIGKILL"), 5000).unref();
    }, job.max_seconds * 1000);
    const code = await exited;
    clearTimeout(deadline);
    await read.catch(() => {});

    if (missing) {
      sink({ t: "error", text: `${spec.label} ist im Image nicht vorhanden. Image neu bauen?` });
      return 2;
    }
    const out = spec.translator.outcome();
    let error = out.error;
    if (timedOut) error = `Das Zeitlimit von ${Math.round(job.max_seconds / 60)} Minuten im Container ist überschritten.`;
    else if (!error && !out.finished) {
      const tail = scrub(truncate(stderrTail.join(" ").trim(), 300), secrets);
      error = `${spec.label} hat den Lauf ${code === 0 ? "ohne Ergebnis" : `mit Status ${code ?? "?"}`} beendet${broken ? ` (${broken} unlesbare Zeilen)` : ""}.${tail ? ` ${tail}` : ""}`;
    } else if (!error && code !== 0 && code !== null) {
      error = `${spec.label} hat sich mit Status ${code} beendet.`;
    }
    if (error) {
      sink({ t: "error", text: error });
      return 1;
    }

    // Whatever the tool did, the rules of the job decide: check every file that differs from the start.
    let files: string[];
    try {
      files = await changedFiles(d.git, d.root);
      const now = head ? (await d.git(["rev-parse", "HEAD"], d.root)).trim() : head;
      if (head && now !== head) {
        const moved = await d.git(["diff", "--name-only", "-z", `${head}..${now}`], d.root);
        files = [...new Set([...files, ...moved.split("\0").filter(Boolean)])];
      }
    } catch (e) {
      sink({ t: "error", text: (e as Error).message });
      return 1;
    }
    if (plan && files.length > 0) {
      sink({ t: "error", text: `${spec.label} hat im ${review ? "Review" : "Planmodus"} Dateien verändert (${files.slice(0, 5).join(", ")}). ${review ? "Das Review" : "Der Plan"} wird verworfen.` });
      return 1;
    }
    const bad = violations(policy, files);
    if (bad.length > 0) {
      sink({ t: "error", text: truncate(`${spec.label} hat gegen die Regeln verstoßen, nichts wird übernommen: ${bad.slice(0, 5).join(" ")}`, 1500) });
      return 1;
    }

    if (review) {
      finishReview(sink, job, out.text);
      return 0;
    }
    if (plan) {
      const p = planFromAnswer(out.text);
      sink({ t: "plan", summary: p.summary, files: p.files, steps: p.steps });
      return 0;
    }
    const f = await record(d.git, d.root, job.branch, commitMessage(job));
    if (!f) {
      sink({ t: "error", text: "Der Agent hat nichts geändert, es gibt nichts zu pushen." });
      return 1;
    }
    sink({ t: "files", files: f.files });
    sink({ t: "result", text: truncate(out.text.trim() || `${job.ticket.key} umgesetzt.`, 1500), branch: job.branch, commit: f.commit });
    return 0;
  } catch (e) {
    sink({ t: "error", text: scrub((e as Error).message, secrets) });
    return 1;
  } finally {
    spec.cleanup?.();
  }
}
