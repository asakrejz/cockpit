// Einstieg des Läufers im Container. Alles Geheime kommt über die Umgebung (ANTHROPIC_MODEL und
// entweder ANTHROPIC_BASE_URL + ANTHROPIC_AUTH_TOKEN für einen Proxy oder ANTHROPIC_API_KEY direkt
// bei Anthropic), der Auftrag aus einer Datei. COCKPIT_ANBIETER wählt den Adapter (src/adapter/).
// Auf stdout stehen ausschließlich JSON-Zeilen (siehe ereignis.ts), Diagnose auf stderr.

import { createInterface } from "node:readline";
import { leseAuftrag } from "./auftrag.ts";
import { stdoutSenke } from "./ereignis.ts";
import { gitAusfuehren } from "./git.ts";
import { waehleAdapter } from "./adapter/index.ts";
import type { Eingang } from "./adapter/schnittstelle.ts";
import { pruefeEndpunkt } from "./pruefung.ts";

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
  if (auftrag.modus === "pruefung") return pruefeEndpunkt(process.env, senke);
  let adapter;
  try {
    adapter = waehleAdapter(process.env.COCKPIT_ANBIETER);
  } catch (e) {
    senke({ t: "fehler", text: (e as Error).message });
    return 2;
  }
  return adapter.ausfuehren(auftrag, {
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
