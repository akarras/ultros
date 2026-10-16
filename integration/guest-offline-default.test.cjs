"use strict";

const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");
const { test } = require("node:test");

const source = fs.readFileSync(path.join(__dirname, "../ultros/static/guest-offline.mjs"), "utf8")
  .replaceAll("export ", "");

function browser(pathname) {
  const registrations = [];
  const listeners = new Map();
  const location = { pathname, origin: "https://ultros.test" };
  const worker = {
    scriptURL: "https://ultros.test/service-worker.js",
    state: "activated",
    postMessage(message, [port]) {
      assert.equal(message.type, "ULTROS_PREPARE_GUEST_OFFLINE");
      queueMicrotask(() => port.receive({ data: { ready: true } }));
    },
  };
  const window = {
    isSecureContext: true,
    addEventListener: (name, callback) => listeners.set(name, callback),
    dispatchEvent() {},
  };
  const start = vm.runInNewContext(source + "\n;prepare_guest_offline;", {
    window, location, URL, setTimeout, clearTimeout,
    CustomEvent: class {},
    MessageChannel: class {
      constructor() {
        this.port1 = { close() {} };
        this.port2 = { receive: event => this.port1.onmessage(event) };
      }
    },
    navigator: { serviceWorker: {
      controller: worker,
      ready: Promise.resolve(),
      register: async url => { registrations.push(url); return { active: worker }; },
    } },
    performance: { getEntriesByType: () => [
      { name: "/pkg/build/ultros.js" }, { name: "/pkg/build/ultros.wasm" },
    ] },
    document: {
      // There is deliberately no LABS cookie or query override.
      cookie: "",
      querySelectorAll: () => [],
      querySelector: () => ({ href: "https://ultros.test/pkg/build/ultros.css" }),
    },
  });
  return { registrations, listeners, location, window, start: () => start("/data/en.rkyv", "en") };
}

for (const pathname of ["/list", "/list/", "/list/device/device:existing"]) {
  test(`offline preparation works by default on ${pathname}`, async () => {
    const state = browser(pathname);
    state.start();
    await new Promise(resolve => setImmediate(resolve));
    assert.deepEqual(state.registrations, ["/service-worker.js"]);
    assert.equal(state.window.__ULTROS_GUEST_OFFLINE_READY__, true);
  });
}

test("account pages do not prepare a guest cache; later device navigation does", async () => {
  const state = browser("/list/42");
  state.start();
  assert.deepEqual(state.registrations, []);
  state.location.pathname = "/list/device/device:existing";
  state.listeners.get("ultros:guest-list-opened")();
  await new Promise(resolve => setImmediate(resolve));
  assert.deepEqual(state.registrations, ["/service-worker.js"]);
  assert.equal(state.window.__ULTROS_GUEST_OFFLINE_READY__, true);
});
