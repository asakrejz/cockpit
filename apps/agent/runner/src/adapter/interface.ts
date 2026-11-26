// The common interface of the adapters. An adapter runs a job with an agent tool and reports the result
// as JSON-line events (see event.ts). Everything a tool does differently stays in the adapter; the backend
// always sees the same event shapes.

import type { Job } from "../job.ts";
import type { Sink } from "../event.ts";
import type { Run } from "../git.ts";

/** Lines from stdin: answers and hints from the user. */
export interface Input {
  next(): Promise<{ t: string; text: string } | null>;
}

/** What every adapter gets from outside. None of it is tied to one tool. */
export interface AdapterEnv {
  /** All events belong here. */
  sink: Sink;
  /** Git in the clone (to record the changes on the branch). */
  git: Run;
  /** Answers and hints from the user, from stdin. */
  input: Input;
  /** Working directory in the container. */
  root: string;
  now: () => number;
  /** The environment variables of the container (model, endpoint, key). Never print. */
  env: Record<string, string | undefined>;
}

export interface Adapter {
  /** The id from `[agent] provider`, e.g. `claude-agent-sdk`. */
  readonly name: string;
  /** Runs the job. Return value: exit code (0 = done, 2 = abort with an `error` event). */
  run(job: Job, d: AdapterEnv): Promise<number>;
}
