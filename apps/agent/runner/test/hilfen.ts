import type { Ereignis } from "../src/ereignis.ts";
import { type Auftrag, pruefeAuftrag } from "../src/auftrag.ts";
import type { Eingang, QueryOptionen, Sdk, SdkNachricht } from "../src/adapter/claude-agent-sdk.ts";

export function auftrag(teil: Record<string, unknown> = {}): Auftrag {
  return pruefeAuftrag({
    modus: "umsetzen",
    ticket: { key: "BAU-142", titel: "Fokus-Ring fehlt", beschreibung: "Der Toggle zeigt keinen Fokus.", kommentare: ["Bitte auch Tastatur"] },
    branch: "claude/BAU-142-fokus-ring",
    ...teil,
  });
}

/** Ein SDK, das vorbereitete Nachrichten liefert. Es ruft nichts auf. */
export function sdkAttrappe(nachrichten: SdkNachricht[], pruefer?: (o: QueryOptionen, prompt: unknown[]) => Promise<void> | void): Sdk & { optionen?: QueryOptionen; fragen: string[] } {
  const s: Sdk & { optionen?: QueryOptionen; fragen: string[]; frageFn?: (t: string, v: string[]) => Promise<string> } = {
    fragen: [],
    query({ prompt, options }) {
      s.optionen = options;
      const gelesen: unknown[] = [];
      return (async function* () {
        const it = prompt[Symbol.asyncIterator]();
        const erste = await it.next();
        gelesen.push(erste.value);
        await pruefer?.(options, gelesen);
        for (const n of nachrichten) {
          if (n.type === "__frage") {
            await s.frageFn?.(String(n.result), []);
            continue;
          }
          yield n;
        }
      })();
    },
    rueckfrageServer(frage) {
      s.frageFn = (t, v) => {
        s.fragen.push(t);
        return frage(t, v);
      };
      return { attrappe: true };
    },
  };
  return s;
}

export function eingang(zeilen: Array<{ t: string; text: string }> = []): Eingang {
  const rest = [...zeilen];
  return {
    naechste: () => (rest.length ? Promise.resolve(rest.shift()!) : new Promise(() => {})),
  };
}

export function sammler() {
  const e: Ereignis[] = [];
  return { e, senke: (x: Ereignis) => void e.push(x) };
}

export function assistant(id: string, bloecke: Array<Record<string, unknown>>, usage = { input_tokens: 100, output_tokens: 20 }): SdkNachricht {
  return { type: "assistant", message: { id, usage, content: bloecke } };
}

export const ergebnisOk = (text: string): SdkNachricht => ({ type: "result", subtype: "success", result: text });
