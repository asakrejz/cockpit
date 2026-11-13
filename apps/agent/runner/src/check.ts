// Prüfmodus: Erreicht der Container den Modell-Endpunkt? Ein einziger Abruf der Modellliste,
// ohne Modellaufruf und ohne Kosten. Der Key kommt aus der Umgebung und wird nie ausgegeben.
//
// * Mit ANTHROPIC_BASE_URL (Proxy wie LiteLLM, anderer kompatibler Endpunkt): `Authorization: Bearer`.
// * Ohne Adresse, mit ANTHROPIC_API_KEY (direkt bei Anthropic): `x-api-key` und `anthropic-version`.

import type { Sink } from "./event.ts";

export type Fetch = (url: string, init: { headers: Record<string, string>; signal: AbortSignal }) => Promise<{ status: number }>;

const ANTHROPIC_API = "https://api.anthropic.com";
const ANTHROPIC_VERSION = "2023-06-01";

export async function checkEndpoint(
  env: Record<string, string | undefined>,
  sink: Sink,
  fetch: Fetch = (u, i) => fetch(u, { ...i, redirect: "manual" }),
): Promise<number> {
  const address = (env.ANTHROPIC_BASE_URL ?? "").replace(/\/+$/, "");
  const direct = !address && !!env.ANTHROPIC_API_KEY;
  if (!address && !direct) {
    sink({ t: "check", reachable: false, key_ok: false, hint: "Im Container ist weder eine Endpunkt-Adresse noch ein Anthropic-Key eingestellt." });
    return 1;
  }
  const base = direct ? ANTHROPIC_API : address;
  const wer = direct ? "Anthropic" : "Der Endpunkt";
  const headers: Record<string, string> = direct
    ? { "x-api-key": env.ANTHROPIC_API_KEY ?? "", "anthropic-version": ANTHROPIC_VERSION }
    : { authorization: `Bearer ${env.ANTHROPIC_AUTH_TOKEN ?? ""}` };
  const ac = new AbortController();
  const deadline = setTimeout(() => ac.abort(), 10_000);
  try {
    const r = await fetch(`${base}/v1/models`, { headers, signal: ac.signal });
    const keyOk = r.status !== 401 && r.status !== 403;
    sink({
      t: "check",
      reachable: true,
      key_ok: keyOk,
      hint: keyOk ? `${wer} antwortet aus dem Container.` : `${wer} antwortet aus dem Container, lehnt den Key aber ab (Status ${r.status}).`,
    });
    return keyOk ? 0 : 1;
  } catch (e) {
    const reason = ac.signal.aborted ? "keine Antwort innerhalb von 10 Sekunden" : (e as Error & { cause?: { code?: string } }).cause?.code ?? (e as Error).message;
    sink({ t: "check", reachable: false, key_ok: false, hint: `Der Container erreicht ${direct ? "Anthropic" : "den Endpunkt"} nicht (${reason}).` });
    return 1;
  } finally {
    clearTimeout(deadline);
  }
}
