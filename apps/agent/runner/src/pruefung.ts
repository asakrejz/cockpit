// Prüfmodus: Erreicht der Container den Modell-Endpunkt? Ein einziger Abruf der Modellliste,
// ohne Modellaufruf und ohne Kosten. Der Key kommt aus der Umgebung und wird nie ausgegeben.
//
// * Mit ANTHROPIC_BASE_URL (Proxy wie LiteLLM, anderer kompatibler Endpunkt): `Authorization: Bearer`.
// * Ohne Adresse, mit ANTHROPIC_API_KEY (direkt bei Anthropic): `x-api-key` und `anthropic-version`.

import type { Senke } from "./ereignis.ts";

export type Abruf = (url: string, init: { headers: Record<string, string>; signal: AbortSignal }) => Promise<{ status: number }>;

const ANTHROPIC_API = "https://api.anthropic.com";
const ANTHROPIC_VERSION = "2023-06-01";

export async function pruefeEndpunkt(
  umgebung: Record<string, string | undefined>,
  senke: Senke,
  abruf: Abruf = (u, i) => fetch(u, { ...i, redirect: "manual" }),
): Promise<number> {
  const adresse = (umgebung.ANTHROPIC_BASE_URL ?? "").replace(/\/+$/, "");
  const direkt = !adresse && !!umgebung.ANTHROPIC_API_KEY;
  if (!adresse && !direkt) {
    senke({ t: "pruefung", erreichbar: false, key_ok: false, hinweis: "Im Container ist weder eine Endpunkt-Adresse noch ein Anthropic-Key eingestellt." });
    return 1;
  }
  const basis = direkt ? ANTHROPIC_API : adresse;
  const wer = direkt ? "Anthropic" : "Der Endpunkt";
  const headers: Record<string, string> = direkt
    ? { "x-api-key": umgebung.ANTHROPIC_API_KEY ?? "", "anthropic-version": ANTHROPIC_VERSION }
    : { authorization: `Bearer ${umgebung.ANTHROPIC_AUTH_TOKEN ?? ""}` };
  const ac = new AbortController();
  const frist = setTimeout(() => ac.abort(), 10_000);
  try {
    const r = await abruf(`${basis}/v1/models`, { headers, signal: ac.signal });
    const keyOk = r.status !== 401 && r.status !== 403;
    senke({
      t: "pruefung",
      erreichbar: true,
      key_ok: keyOk,
      hinweis: keyOk ? `${wer} antwortet aus dem Container.` : `${wer} antwortet aus dem Container, lehnt den Key aber ab (Status ${r.status}).`,
    });
    return keyOk ? 0 : 1;
  } catch (e) {
    const grund = ac.signal.aborted ? "keine Antwort innerhalb von 10 Sekunden" : (e as Error & { cause?: { code?: string } }).cause?.code ?? (e as Error).message;
    senke({ t: "pruefung", erreichbar: false, key_ok: false, hinweis: `Der Container erreicht ${direkt ? "Anthropic" : "den Endpunkt"} nicht (${grund}).` });
    return 1;
  } finally {
    clearTimeout(frist);
  }
}
