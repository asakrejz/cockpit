import assert from "node:assert/strict";
import { test } from "node:test";
import { pruefeEndpunkt } from "../src/pruefung.ts";
import { sammler } from "./hilfen.ts";

test("erreichbar mit gültigem Key", async () => {
  const s = sammler();
  const aufgerufen: string[] = [];
  const code = await pruefeEndpunkt(
    { ANTHROPIC_BASE_URL: "https://litellm.example/", ANTHROPIC_AUTH_TOKEN: "geheim" },
    s.senke,
    async (url, init) => {
      aufgerufen.push(url);
      assert.equal(init.headers.authorization, "Bearer geheim");
      return { status: 200 };
    },
  );
  assert.equal(code, 0);
  assert.deepEqual(aufgerufen, ["https://litellm.example/v1/models"]);
  assert.deepEqual(s.e[0], { t: "pruefung", erreichbar: true, key_ok: true, hinweis: "Der Endpunkt antwortet aus dem Container." });
  assert.ok(!JSON.stringify(s.e).includes("geheim"), "der Key steht nie in der Ausgabe");
});

test("Key abgelehnt", async () => {
  const s = sammler();
  const code = await pruefeEndpunkt({ ANTHROPIC_BASE_URL: "https://litellm.example", ANTHROPIC_AUTH_TOKEN: "x" }, s.senke, async () => ({ status: 401 }));
  assert.equal(code, 1);
  assert.equal((s.e[0] as any).erreichbar, true);
  assert.equal((s.e[0] as any).key_ok, false);
});

test("nicht erreichbar nennt den Grund", async () => {
  const s = sammler();
  const code = await pruefeEndpunkt({ ANTHROPIC_BASE_URL: "https://litellm.example", ANTHROPIC_AUTH_TOKEN: "x" }, s.senke, async () => {
    throw Object.assign(new Error("fetch failed"), { cause: { code: "ENOTFOUND" } });
  });
  assert.equal(code, 1);
  assert.match((s.e[0] as any).hinweis, /ENOTFOUND/);
  assert.equal((s.e[0] as any).erreichbar, false);
});

test("ohne Adresse", async () => {
  const s = sammler();
  assert.equal(await pruefeEndpunkt({}, s.senke, async () => ({ status: 200 })), 1);
  assert.match((s.e[0] as any).hinweis, /weder eine Endpunkt-Adresse noch ein Anthropic-Key/);
});

test("direkt bei Anthropic: x-api-key und Version, kein Bearer", async () => {
  const s = sammler();
  const aufgerufen: Array<{ url: string; headers: Record<string, string> }> = [];
  const code = await pruefeEndpunkt({ ANTHROPIC_API_KEY: "sk-ant-geheim" }, s.senke, async (url, init) => {
    aufgerufen.push({ url, headers: init.headers });
    return { status: 200 };
  });
  assert.equal(code, 0);
  assert.equal(aufgerufen[0].url, "https://api.anthropic.com/v1/models");
  assert.equal(aufgerufen[0].headers["x-api-key"], "sk-ant-geheim");
  assert.equal(aufgerufen[0].headers["anthropic-version"], "2023-06-01");
  assert.equal(aufgerufen[0].headers.authorization, undefined);
  assert.match((s.e[0] as any).hinweis, /Anthropic antwortet/);
  assert.ok(!JSON.stringify(s.e).includes("geheim"), "der Key steht nie in der Ausgabe");
});

test("direkt: abgelehnter Key und nicht erreichbar", async () => {
  const s = sammler();
  assert.equal(await pruefeEndpunkt({ ANTHROPIC_API_KEY: "x" }, s.senke, async () => ({ status: 401 })), 1);
  assert.equal((s.e[0] as any).key_ok, false);
  const t = sammler();
  await pruefeEndpunkt({ ANTHROPIC_API_KEY: "x" }, t.senke, async () => {
    throw Object.assign(new Error("fetch failed"), { cause: { code: "ENOTFOUND" } });
  });
  assert.match((t.e[0] as any).hinweis, /Anthropic nicht \(ENOTFOUND\)|Anthropic nicht/);
});

test("mit Adresse gewinnt der Endpunkt, auch wenn ein API-Key in der Umgebung steht", async () => {
  const s = sammler();
  const urls: string[] = [];
  await pruefeEndpunkt({ ANTHROPIC_BASE_URL: "https://litellm.example", ANTHROPIC_AUTH_TOKEN: "t", ANTHROPIC_API_KEY: "k" }, s.senke, async (url, init) => {
    urls.push(url);
    assert.ok(init.headers.authorization?.startsWith("Bearer "));
    return { status: 200 };
  });
  assert.deepEqual(urls, ["https://litellm.example/v1/models"]);
});
