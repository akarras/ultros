"use strict";
const test = require("node:test");
const assert = require("node:assert/strict");
const { pathToFileURL } = require("node:url");
const path = require("node:path");

// An unsupported browser still reports the preparation result. That event
// proves the public entry point reached preparation without a Labs cookie;
// the service-worker suite separately verifies cache contents and isolation.
test("offline preparation follows default Lists routes without Labs", async () => {
  const names = ["window", "location", "navigator", "CustomEvent"];
  const originals = new Map(names.map(name => [name, Object.getOwnPropertyDescriptor(globalThis, name)]));
  try {
    for (const [pathname, expected] of [["/list", 1], ["/list/", 1], ["/list/device/device:abc", 1], ["/list/42", 0], ["/settings", 0]]) {
      const events = [];
      globalThis.window = {
        isSecureContext: false,
        addEventListener() {},
        dispatchEvent(event) { events.push(event); },
      };
      globalThis.location = { pathname };
      Object.defineProperty(globalThis, "navigator", { configurable: true, value: {} });
      globalThis.CustomEvent = class { constructor(type, init) { this.type = type; this.detail = init.detail; } };
      const url = pathToFileURL(path.join(__dirname, "../ultros/static/guest-offline.mjs"));
      url.searchParams.set("case", pathname);
      const { prepare_guest_offline } = await import(url.href);
      prepare_guest_offline("/pkg/catalog.bin", "en");
      await new Promise(resolve => setImmediate(resolve));
      assert.equal(events.length, expected, pathname);
      if (expected) {
        assert.equal(events[0].type, "ultros:guest-offline-ready");
        assert.equal(events[0].detail.ready, false);
      }
    }
  } finally {
    for (const [name, descriptor] of originals) {
      if (descriptor) Object.defineProperty(globalThis, name, descriptor);
      else delete globalThis[name];
    }
  }
});
