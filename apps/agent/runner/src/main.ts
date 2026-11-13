// Entry point of the runner in the container. Everything secret comes via the environment (ANTHROPIC_MODEL and
// either ANTHROPIC_BASE_URL + ANTHROPIC_AUTH_TOKEN for a proxy or ANTHROPIC_API_KEY directly
// at Anthropic), the job from a file. COCKPIT_PROVIDER chooses the adapter (src/adapter/).
// Only JSON lines go to stdout (see event.ts), diagnostics to stderr.

import { createInterface } from "node:readline";
import { readJob } from "./job.ts";
import { stdoutSink } from "./event.ts";
import { gitRun } from "./git.ts";
import { chooseAdapter } from "./adapter/index.ts";
import type { Input } from "./adapter/interface.ts";
import { checkEndpoint } from "./check.ts";

process.umask(0); // the backend should be able to read and delete the clone after the run

function stdinInput(): Input {
  const lines = createInterface({ input: process.stdin })[Symbol.asyncIterator]();
  return {
    async next() {
      for (;;) {
        const n = await lines.next();
        if (n.done) return null;
        try {
          const j = JSON.parse(n.value);
          if ((j.t === "answer" || j.t === "hint") && typeof j.text === "string") return { t: j.t, text: j.text };
        } catch {
          /* skip unreadable lines */
        }
      }
    },
  };
}

async function main(): Promise<number> {
  const sink = stdoutSink();
  const path = process.env.COCKPIT_JOB ?? "/job/job.json";
  let job;
  try {
    job = readJob(path);
  } catch (e) {
    sink({ t: "error", text: (e as Error).message });
    return 2;
  }
  if (job.mode === "check") return checkEndpoint(process.env, sink);
  let adapter;
  try {
    adapter = chooseAdapter(process.env.COCKPIT_PROVIDER);
  } catch (e) {
    sink({ t: "error", text: (e as Error).message });
    return 2;
  }
  return adapter.run(job, {
    sink,
    git: gitRun,
    input: stdinInput(),
    root: process.env.COCKPIT_ROOT ?? "/work",
    now: () => Date.now(),
    env: process.env,
  });
}

main().then(
  (code) => process.exit(code),
  (e) => {
    stdoutSink()({ t: "error", text: `Unerwarteter Fehler im Läufer: ${(e as Error).message}` });
    process.exit(2);
  },
);
