import assert from "node:assert/strict";
import { test } from "node:test";
import { pruefeProxy } from "../src/pruefung.ts";
import { sammler } from "./hilfen.ts";

test("erreichbar mit gültigem Key", async () => {
  const s = sammler();
  const aufgerufen: string[] = [];
  const code = await pruefeProxy(
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
  assert.deepEqual(s.e[0], { t: "pruefung", erreichbar: true, key_ok: true, hinweis: "Der Proxy antwortet aus dem Container." });
  assert.ok(!JSON.stringify(s.e).includes("geheim"), "der Key steht nie in der Ausgabe");
});

test("Key abgelehnt", async () => {
  const s = sammler();
  const code = await pruefeProxy({ ANTHROPIC_BASE_URL: "https://litellm.example", ANTHROPIC_AUTH_TOKEN: "x" }, s.senke, async () => ({ status: 401 }));
  assert.equal(code, 1);
  assert.equal((s.e[0] as any).erreichbar, true);
  assert.equal((s.e[0] as any).key_ok, false);
});

test("nicht erreichbar nennt den Grund", async () => {
  const s = sammler();
  const code = await pruefeProxy({ ANTHROPIC_BASE_URL: "https://litellm.example", ANTHROPIC_AUTH_TOKEN: "x" }, s.senke, async () => {
    throw Object.assign(new Error("fetch failed"), { cause: { code: "ENOTFOUND" } });
  });
  assert.equal(code, 1);
  assert.match((s.e[0] as any).hinweis, /ENOTFOUND/);
  assert.equal((s.e[0] as any).erreichbar, false);
});

test("ohne Adresse", async () => {
  const s = sammler();
  assert.equal(await pruefeProxy({}, s.senke, async () => ({ status: 200 })), 1);
  assert.match((s.e[0] as any).hinweis, /keine Proxy-Adresse/);
});
