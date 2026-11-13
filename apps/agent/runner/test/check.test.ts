import assert from "node:assert/strict";
import { test } from "node:test";
import { checkEndpoint } from "../src/pruefung.ts";
import { collector } from "./hilfen.ts";

test("erreichbar mit gültigem Key", async () => {
  const s = collector();
  const called: string[] = [];
  const code = await checkEndpoint(
    { ANTHROPIC_BASE_URL: "https://litellm.example/", ANTHROPIC_AUTH_TOKEN: "geheim" },
    s.sink,
    async (url, init) => {
      called.push(url);
      assert.equal(init.headers.authorization, "Bearer geheim");
      return { status: 200 };
    },
  );
  assert.equal(code, 0);
  assert.deepEqual(called, ["https://litellm.example/v1/models"]);
  assert.deepEqual(s.e[0], { t: "check", reachable: true, key_ok: true, hint: "Der Endpunkt antwortet aus dem Container." });
  assert.ok(!JSON.stringify(s.e).includes("geheim"), "der Key steht nie in der Ausgabe");
});

test("Key abgelehnt", async () => {
  const s = collector();
  const code = await checkEndpoint({ ANTHROPIC_BASE_URL: "https://litellm.example", ANTHROPIC_AUTH_TOKEN: "x" }, s.sink, async () => ({ status: 401 }));
  assert.equal(code, 1);
  assert.equal((s.e[0] as any).reachable, true);
  assert.equal((s.e[0] as any).key_ok, false);
});

test("nicht erreichbar nennt den Grund", async () => {
  const s = collector();
  const code = await checkEndpoint({ ANTHROPIC_BASE_URL: "https://litellm.example", ANTHROPIC_AUTH_TOKEN: "x" }, s.sink, async () => {
    throw Object.assign(new Error("fetch failed"), { cause: { code: "ENOTFOUND" } });
  });
  assert.equal(code, 1);
  assert.match((s.e[0] as any).hint, /ENOTFOUND/);
  assert.equal((s.e[0] as any).reachable, false);
});

test("ohne Adresse", async () => {
  const s = collector();
  assert.equal(await checkEndpoint({}, s.sink, async () => ({ status: 200 })), 1);
  assert.match((s.e[0] as any).hint, /neither eine Endpoint-Address still ein Anthropic-Key/);
});

test("direkt bei Anthropic: x-api-key und Version, kein Bearer", async () => {
  const s = collector();
  const called: Array<{ url: string; headers: Record<string, string> }> = [];
  const code = await checkEndpoint({ ANTHROPIC_API_KEY: "sk-ant-geheim" }, s.sink, async (url, init) => {
    called.push({ url, headers: init.headers });
    return { status: 200 };
  });
  assert.equal(code, 0);
  assert.equal(called[0].url, "https://api.anthropic.com/v1/models");
  assert.equal(called[0].headers["x-api-key"], "sk-ant-geheim");
  assert.equal(called[0].headers["anthropic-version"], "2023-06-01");
  assert.equal(called[0].headers.authorization, undefined);
  assert.match((s.e[0] as any).hint, /Anthropic answers/);
  assert.ok(!JSON.stringify(s.e).includes("geheim"), "der Key steht nie in der Ausgabe");
});

test("direkt: abgelehnter Key und nicht erreichbar", async () => {
  const s = collector();
  assert.equal(await checkEndpoint({ ANTHROPIC_API_KEY: "x" }, s.sink, async () => ({ status: 401 })), 1);
  assert.equal((s.e[0] as any).key_ok, false);
  const t = collector();
  await checkEndpoint({ ANTHROPIC_API_KEY: "x" }, t.sink, async () => {
    throw Object.assign(new Error("fetch failed"), { cause: { code: "ENOTFOUND" } });
  });
  assert.match((t.e[0] as any).hint, /Anthropic not \(ENOTFOUND\)|Anthropic not/);
});

test("mit Adresse gewinnt der Endpunkt, auch wenn ein API-Key in der Umgebung steht", async () => {
  const s = collector();
  const urls: string[] = [];
  await checkEndpoint({ ANTHROPIC_BASE_URL: "https://litellm.example", ANTHROPIC_AUTH_TOKEN: "t", ANTHROPIC_API_KEY: "k" }, s.sink, async (url, init) => {
    urls.push(url);
    assert.ok(init.headers.authorization?.startsWith("Bearer "));
    return { status: 200 };
  });
  assert.deepEqual(urls, ["https://litellm.example/v1/models"]);
});
