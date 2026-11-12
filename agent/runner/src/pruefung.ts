// Prüfmodus: Erreicht der Container den LiteLLM-Proxy? Ein einziger Abruf der Modellliste,
// ohne Modellaufruf und ohne Kosten. Der Key kommt aus der Umgebung und wird nie ausgegeben.

import type { Senke } from "./ereignis.ts";

export type Abruf = (url: string, init: { headers: Record<string, string>; signal: AbortSignal }) => Promise<{ status: number }>;

export async function pruefeProxy(
  umgebung: Record<string, string | undefined>,
  senke: Senke,
  abruf: Abruf = (u, i) => fetch(u, i),
): Promise<number> {
  const basis = (umgebung.ANTHROPIC_BASE_URL ?? "").replace(/\/+$/, "");
  const key = umgebung.ANTHROPIC_AUTH_TOKEN ?? "";
  if (!basis) {
    senke({ t: "pruefung", erreichbar: false, key_ok: false, hinweis: "Im Container ist keine Proxy-Adresse eingestellt." });
    return 1;
  }
  const ac = new AbortController();
  const frist = setTimeout(() => ac.abort(), 10_000);
  try {
    const r = await abruf(`${basis}/v1/models`, { headers: { authorization: `Bearer ${key}` }, signal: ac.signal });
    const keyOk = r.status !== 401 && r.status !== 403;
    senke({
      t: "pruefung",
      erreichbar: true,
      key_ok: keyOk,
      hinweis: keyOk ? "Der Proxy antwortet aus dem Container." : `Der Proxy antwortet aus dem Container, lehnt den Key aber ab (Status ${r.status}).`,
    });
    return keyOk ? 0 : 1;
  } catch (e) {
    const grund = ac.signal.aborted ? "keine Antwort innerhalb von 10 Sekunden" : (e as Error & { cause?: { code?: string } }).cause?.code ?? (e as Error).message;
    senke({ t: "pruefung", erreichbar: false, key_ok: false, hinweis: `Der Container erreicht den Proxy nicht (${grund}).` });
    return 1;
  } finally {
    clearTimeout(frist);
  }
}
