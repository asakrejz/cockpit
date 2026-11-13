// Einstieg des Läufers im Container. Alles Geheime kommt über die Umgebung (ANTHROPIC_MODEL und
// entweder ANTHROPIC_BASE_URL + ANTHROPIC_AUTH_TOKEN für einen Proxy oder ANTHROPIC_API_KEY direkt
// bei Anthropic), der Auftrag aus einer Datei. COCKPIT_ANBIETER wählt den Adapter (src/adapter/).
// Auf stdout stehen ausschließlich JSON-Zeilen (siehe ereignis.ts), Diagnose auf stderr.

import { createInterface } from "node:readline";
import { readJob } from "./auftrag.ts";
import { stdoutSink } from "./ereignis.ts";
import { gitRun } from "./git.ts";
import { chooseAdapter } from "./adapter/index.ts";
import type { Input } from "./adapter/schnittstelle.ts";
import { checkEndpoint } from "./pruefung.ts";

process.umask(0); // das Backend soll den Klon nach dem Lauf lesen und löschen können

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
          /* unlesbare Zeilen überspringen */
        }
      }
    },
  };
}

async function main(): Promise<number> {
  const sink = stdoutSink();
  const path = process.env.COCKPIT_JOB ?? "/auftrag/auftrag.json";
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
    wurzel: process.env.COCKPIT_WURZEL ?? "/work",
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
