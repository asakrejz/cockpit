// Einstieg des Läufers im Container. Alles Geheime kommt über die Umgebung
// (ANTHROPIC_BASE_URL, ANTHROPIC_AUTH_TOKEN, ANTHROPIC_MODEL), der Auftrag aus einer Datei.
// Auf stdout stehen ausschließlich JSON-Zeilen (siehe ereignis.ts), Diagnose auf stderr.

import { createInterface } from "node:readline";
import { leseAuftrag } from "./auftrag.ts";
import { stdoutSenke } from "./ereignis.ts";
import { gitAusfuehren } from "./git.ts";
import { type Eingang, type Sdk, lauf } from "./lauf.ts";
import { pruefeProxy } from "./pruefung.ts";

process.umask(0); // das Backend soll den Klon nach dem Lauf lesen und löschen können

function stdinEingang(): Eingang {
  const zeilen = createInterface({ input: process.stdin })[Symbol.asyncIterator]();
  return {
    async naechste() {
      for (;;) {
        const n = await zeilen.next();
        if (n.done) return null;
        try {
          const j = JSON.parse(n.value);
          if ((j.t === "antwort" || j.t === "hinweis") && typeof j.text === "string") return { t: j.t, text: j.text };
        } catch {
          /* unlesbare Zeilen überspringen */
        }
      }
    },
  };
}

async function ladeSdk(): Promise<Sdk> {
  const m: any = await import("@anthropic-ai/claude-agent-sdk");
  const { z } = await import("zod");
  return {
    query: (p) => m.query(p),
    rueckfrageServer: (frage) =>
      m.createSdkMcpServer({
        name: "cockpit",
        version: "1.0.0",
        tools: [
          m.tool(
            "rueckfrage",
            "Stellt dem Nutzer eine Frage und wartet auf seine Antwort. Nur für Dinge, die du aus dem Code nicht klären kannst.",
            { frage: z.string(), vorschlaege: z.array(z.string()).optional() },
            async (a: { frage: string; vorschlaege?: string[] }) => ({
              content: [{ type: "text", text: await frage(a.frage, a.vorschlaege ?? []) }],
            }),
          ),
        ],
      }),
  };
}

async function main(): Promise<number> {
  const senke = stdoutSenke();
  const pfad = process.env.COCKPIT_AUFTRAG ?? "/auftrag/auftrag.json";
  let auftrag;
  try {
    auftrag = leseAuftrag(pfad);
  } catch (e) {
    senke({ t: "fehler", text: (e as Error).message });
    return 2;
  }
  if (auftrag.modus === "pruefung") return pruefeProxy(process.env, senke);
  let sdk: Sdk;
  try {
    sdk = await ladeSdk();
  } catch (e) {
    senke({ t: "fehler", text: `Das Agent SDK lässt sich nicht laden (${(e as Error).message}). Image neu bauen?` });
    return 2;
  }
  return lauf(auftrag, {
    sdk,
    senke,
    git: gitAusfuehren,
    eingang: stdinEingang(),
    wurzel: process.env.COCKPIT_WURZEL ?? "/work",
    jetzt: () => Date.now(),
    umgebung: process.env,
  });
}

main().then(
  (code) => process.exit(code),
  (e) => {
    stdoutSenke()({ t: "fehler", text: `Unerwarteter Fehler im Läufer: ${(e as Error).message}` });
    process.exit(2);
  },
);
