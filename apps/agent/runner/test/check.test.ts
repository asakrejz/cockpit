import assert from "node:assert/strict";
import { test } from "node:test";
import { checkEndpoint } from "../src/check.ts";
import { setLanguage } from "../src/texts.ts";
import { collector } from "./helpers.ts";

test("reachable with a valid key", async () => {
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
  assert.deepEqual(s.e[0], { t: "check", reachable: true, key_ok: true, hint: "The endpoint answers from the container." });
  assert.ok(!JSON.stringify(s.e).includes("geheim"), "der Key steht nie in der Ausgabe");
});

test("key rejected", async () => {
  const s = collector();
  const code = await checkEndpoint({ ANTHROPIC_BASE_URL: "https://litellm.example", ANTHROPIC_AUTH_TOKEN: "x" }, s.sink, async () => ({ status: 401 }));
  assert.equal(code, 1);
  assert.equal((s.e[0] as any).reachable, true);
  assert.equal((s.e[0] as any).key_ok, false);
});

test("not reachable names the reason", async () => {
  const s = collector();
  const code = await checkEndpoint({ ANTHROPIC_BASE_URL: "https://litellm.example", ANTHROPIC_AUTH_TOKEN: "x" }, s.sink, async () => {
    throw Object.assign(new Error("fetch failed"), { cause: { code: "ENOTFOUND" } });
  });
  assert.equal(code, 1);
  assert.equal((s.e[0] as any).hint, "The container cannot reach the endpoint (ENOTFOUND).");
  assert.equal((s.e[0] as any).reachable, false);
});

test("without an address", async () => {
  const s = collector();
  assert.equal(await checkEndpoint({}, s.sink, async () => ({ status: 200 })), 1);
  assert.match((s.e[0] as any).hint, /neither an endpoint address nor an Anthropic key/);
});

test("directly at Anthropic: x-api-key and version, no Bearer", async () => {
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
  assert.match((s.e[0] as any).hint, /^Anthropic answers from the container\.$/);
  assert.ok(!JSON.stringify(s.e).includes("geheim"), "der Key steht nie in der Ausgabe");
});

test("direct: rejected key and not reachable", async () => {
  const s = collector();
  assert.equal(await checkEndpoint({ ANTHROPIC_API_KEY: "x" }, s.sink, async () => ({ status: 401 })), 1);
  assert.equal((s.e[0] as any).key_ok, false);
  const t = collector();
  await checkEndpoint({ ANTHROPIC_API_KEY: "x" }, t.sink, async () => {
    throw Object.assign(new Error("fetch failed"), { cause: { code: "ENOTFOUND" } });
  });
  assert.equal((t.e[0] as any).hint, "The container cannot reach Anthropic (ENOTFOUND).");
});

test("with an address the endpoint wins, even if an API key is in the environment", async () => {
  const s = collector();
  const urls: string[] = [];
  await checkEndpoint({ ANTHROPIC_BASE_URL: "https://litellm.example", ANTHROPIC_AUTH_TOKEN: "t", ANTHROPIC_API_KEY: "k" }, s.sink, async (url, init) => {
    urls.push(url);
    assert.ok(init.headers.authorization?.startsWith("Bearer "));
    return { status: 200 };
  });
  assert.deepEqual(urls, ["https://litellm.example/v1/models"]);
});

test("provider codex asks the OpenAI endpoint with the bearer key", async () => {
  const s = collector();
  const called: Array<{ url: string; headers: Record<string, string> }> = [];
  const code = await checkEndpoint(
    { COCKPIT_PROVIDER: "codex", OPENAI_BASE_URL: "https://proxy.example/v1/", OPENAI_API_KEY: "geheim", ANTHROPIC_API_KEY: "anders" },
    s.sink,
    async (url, init) => {
      called.push({ url, headers: init.headers });
      return { status: 200 };
    },
  );
  assert.equal(code, 0);
  assert.deepEqual(called, [{ url: "https://proxy.example/v1/models", headers: { authorization: "Bearer geheim" } }]);
  assert.ok(!JSON.stringify(s.e).includes("geheim"));
});

test("provider codex directly at OpenAI, without a key and with a rejected key", async () => {
  const s = collector();
  const urls: string[] = [];
  assert.equal(await checkEndpoint({ COCKPIT_PROVIDER: "codex", OPENAI_API_KEY: "k" }, s.sink, async (u) => (urls.push(u), { status: 401 })), 1);
  assert.deepEqual(urls, ["https://api.openai.com/v1/models"]);
  assert.equal((s.e[0] as any).hint, "OpenAI answers from the container but refuses the key (status 401).");
  const t = collector();
  assert.equal(await checkEndpoint({ COCKPIT_PROVIDER: "codex" }, t.sink, async () => ({ status: 200 })), 1);
  assert.match((t.e[0] as any).hint, /OPENAI_API_KEY/);
});

test("the real fetch is used by default (a closed local port is reported, no endless recursion)", async () => {
  const s = collector();
  assert.equal(await checkEndpoint({ ANTHROPIC_BASE_URL: "http://127.0.0.1:1", ANTHROPIC_AUTH_TOKEN: "x" }, s.sink), 1);
  assert.equal((s.e[0] as any).reachable, false);
});

test("the hint follows the language of the job", async () => {
  setLanguage("de");
  try {
    const s = collector();
    await checkEndpoint({ ANTHROPIC_BASE_URL: "https://litellm.example", ANTHROPIC_AUTH_TOKEN: "x" }, s.sink, async () => ({ status: 200 }));
    assert.equal((s.e[0] as any).hint, "Der Endpunkt antwortet aus dem Container.");
    const t = collector();
    await checkEndpoint({ ANTHROPIC_BASE_URL: "https://litellm.example", ANTHROPIC_AUTH_TOKEN: "x" }, t.sink, async () => ({ status: 403 }));
    assert.equal((t.e[0] as any).hint, "Der Endpunkt antwortet aus dem Container, lehnt den Key aber ab (Status 403).");
    const u = collector();
    await checkEndpoint({ ANTHROPIC_BASE_URL: "https://litellm.example", ANTHROPIC_AUTH_TOKEN: "x" }, u.sink, async () => {
      throw Object.assign(new Error("fetch failed"), { cause: { code: "ENOTFOUND" } });
    });
    assert.equal((u.e[0] as any).hint, "Der Container erreicht den Endpunkt nicht (ENOTFOUND).");
    const v = collector();
    await checkEndpoint({ COCKPIT_PROVIDER: "codex" }, v.sink, async () => ({ status: 200 }));
    assert.equal((v.e[0] as any).hint, "Im Container ist kein Key (OPENAI_API_KEY) eingestellt.");
  } finally {
    setLanguage("en");
  }
});
