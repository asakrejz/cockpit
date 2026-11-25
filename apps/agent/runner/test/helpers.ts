import type { Event } from "../src/event.ts";
import { type Job, checkJob } from "../src/job.ts";
import type { Input, QueryOptions, Sdk, SdkMessage } from "../src/adapter/claude-agent-sdk.ts";

export function job(part: Record<string, unknown> = {}): Job {
  return checkJob({
    mode: "implement",
    ticket: { key: "BAU-142", title: "Fokus-Ring fehlt", description: "Der Toggle zeigt keinen Fokus.", comments: ["Bitte auch Tastatur"] },
    branch: "claude/BAU-142-fokus-ring",
    ...part,
  });
}

/** An SDK that delivers prepared messages. It calls nothing. */
export function sdkFake(messages: SdkMessage[], checker?: (o: QueryOptions, prompt: unknown[]) => Promise<void> | void): Sdk & { options?: QueryOptions; ask: string[] } {
  const s: Sdk & { options?: QueryOptions; ask: string[]; questionFn?: (t: string, v: string[]) => Promise<string> } = {
    ask: [],
    query({ prompt, options }) {
      s.options = options;
      const read: unknown[] = [];
      return (async function* () {
        const it = prompt[Symbol.asyncIterator]();
        const first = await it.next();
        read.push(first.value);
        await checker?.(options, read);
        for (const n of messages) {
          if (n.type === "__frage") {
            await s.questionFn?.(String(n.result), []);
            continue;
          }
          yield n;
        }
      })();
    },
    askUserServer(question) {
      s.questionFn = (t, v) => {
        s.ask.push(t);
        return question(t, v);
      };
      return { fake: true };
    },
  };
  return s;
}

export function input(lines: Array<{ t: string; text: string }> = []): Input {
  const rest = [...lines];
  return {
    next: () => (rest.length ? Promise.resolve(rest.shift()!) : new Promise(() => {})),
  };
}

export function collector() {
  const e: Event[] = [];
  return { e, sink: (x: Event) => void e.push(x) };
}

export function assistant(id: string, blocks: Array<Record<string, unknown>>, usage = { input_tokens: 100, output_tokens: 20 }): SdkMessage {
  return { type: "assistant", message: { id, usage, content: blocks } };
}

export const resultOk = (text: string): SdkMessage => ({ type: "result", subtype: "success", result: text });
