import { strict as assert } from "node:assert";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import vm from "node:vm";
import ts from "typescript";

function setup() {
  const source = readFileSync(new URL("../src/lib/trustpilot.ts", import.meta.url), "utf8");
  const code = ts.transpileModule(source, {
    compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 },
  }).outputText;
  const scripts = [];
  const window = {};
  const document = {
    createElement: () => {
      const handlers = {};
      return {
        addEventListener: (name, handler) => { handlers[name] = handler; },
        dispatch: (name) => handlers[name](),
        remove() { scripts.splice(scripts.indexOf(this), 1); },
      };
    },
    head: { appendChild: (script) => scripts.push(script) },
  };
  const context = { exports: {}, window, document };
  vm.runInNewContext(code, context);
  return { api: context.exports, scripts, window };
}

test("Trustpilot lädt und registriert bei doppeltem React-Effekt nur einmal", async () => {
  const { api, scripts, window } = setup();
  const first = api.registerTrustpilot();
  const second = api.registerTrustpilot();
  assert.equal(scripts.length, 1);
  assert.equal(window.tp.q.length, 1);
  assert.deepEqual(Array.from(window.tp.q[0]), ["register", "sHQ1P2OxAlG0YV4f"]);
  scripts[0].dispatch("load");
  await Promise.all([first, second]);
  await api.registerTrustpilot();
  assert.equal(scripts.length, 1);
  assert.equal(window.tp.q.length, 1);
});

test("TrustBox überlebt SPA-Wechsel und vermeidet doppelte Iframes", async () => {
  const { api, scripts, window } = setup();
  const calls = [];
  const element = () => ({ isConnected: true, iframe: false, querySelector() { return this.iframe; } });
  const first = element();
  const pending = [api.loadTrustpilotWidget(first), api.loadTrustpilotWidget(first)];
  assert.equal(scripts.length, 1);
  window.Trustpilot = { loadFromElement: (node) => { calls.push(node); node.iframe = true; } };
  scripts[0].dispatch("load");
  await Promise.all(pending);
  assert.equal(calls.length, 1);
  const next = element();
  await api.loadTrustpilotWidget(next);
  assert.deepEqual(calls, [first, next]);
  const detached = element();
  detached.isConnected = false;
  await api.loadTrustpilotWidget(detached);
  assert.equal(calls.length, 2);
  assert.equal(scripts.length, 1);
});

test("Ein fehlgeschlagenes Skript kann beim nächsten Aufruf erneut laden", async () => {
  const { api, scripts, window } = setup();
  const first = api.registerTrustpilot();
  const rejected = assert.rejects(first, /Trustpilot konnte nicht geladen werden/);
  scripts[0].dispatch("error");
  await rejected;
  assert.equal(scripts.length, 0);
  const retry = api.registerTrustpilot();
  assert.equal(scripts.length, 1);
  assert.equal(window.tp.q.length, 1);
  scripts[0].dispatch("load");
  await retry;
});
