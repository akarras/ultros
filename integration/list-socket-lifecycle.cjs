"use strict";

// BASE_URL=<isolated test-auth server> node integration/list-socket-lifecycle.cjs
// Native tests count retained receivers/permission calls; this checks real wire
// delivery and navigation using the same authenticated socket throughout.
const assert = require("node:assert/strict");
const puppeteer = require("puppeteer");

async function run() {
  const base = process.env.BASE_URL || "http://127.0.0.1:8080";
  const browser = await puppeteer.launch({ headless: true });
  const lists = [];
  let owner;
  async function api(method, path, body) {
    const response = await owner.evaluate(async ({ method, path, body }) => {
      const result = await fetch(path, {
        method, headers: body === undefined ? {} : { "Content-Type": "application/json" },
        body: body === undefined ? undefined : JSON.stringify(body),
      });
      const text = await result.text();
      return { status: result.status, body: text ? JSON.parse(text) : null };
    }, { method, path, body });
    assert.equal(response.status, 200, `${method} ${path}: ${JSON.stringify(response.body)}`);
    return response.body;
  }
  try {
    const context = await browser.createBrowserContext();
    owner = await context.newPage();
    await owner.setRequestInterception(true);
    owner.on("request", request => /googlesyndication|doubleclick|googletagmanager/.test(request.url())
      ? request.abort() : request.continue());
    const seed = Date.now();
    const response = await owner.goto(`${base}/test/login?user_id=990000000872&username=SocketLifecycleQA&redirect=/welcome`, { waitUntil: "domcontentloaded", timeout: 60000 });
    assert(response?.ok(), "test-auth login succeeded");
    const worlds = await api("GET", "/api/v1/world_data");
    const world = worlds.regions[0].datacenters[0].worlds[0].id;
    for (const suffix of ["target", "control"]) {
      const name = `Socket lifecycle ${seed} ${suffix}`;
      await api("POST", "/api/v1/list/create", { name, wdr_filter: { World: world } });
      const entries = await api("GET", "/api/v1/list");
      const id = entries.find(entry => entry.list.name === name)?.list.id;
      assert(id, "created fixture list");
      lists.push(id);
    }
    const [target, control] = lists;
    await owner.evaluate(async () => {
      const url = new URL("/api/v1/realtime/events", location.href);
      url.protocol = location.protocol === "https:" ? "wss:" : "ws:";
      const socket = new WebSocket(url);
      window.lifecycle = { socket, messages: [] };
      socket.addEventListener("message", event => window.lifecycle.messages.push(JSON.parse(event.data)));
      await new Promise((resolve, reject) => {
        socket.addEventListener("open", resolve, { once: true });
        socket.addEventListener("error", reject, { once: true });
      });
    });
    async function sendAndWait(message, reply, id) {
      const start = await owner.evaluate(message => {
        const start = window.lifecycle.messages.length;
        window.lifecycle.socket.send(JSON.stringify(message));
        return start;
      }, message);
      await owner.waitForFunction(({ start, reply, id }) => window.lifecycle.messages.slice(start)
        .some(message => message[reply]?.subscription_id === id), { timeout: 30000 }, { start, reply, id });
    }
    async function subscribe(id, list) {
      await sendAndWait({ SubscribeListDoc: { subscription_id: id, list_id: list, version: "" } }, "ListDocSubscribed", id);
    }
    async function unsubscribe(id) {
      await sendAndWait({ Unsubscribe: { subscription_id: id } }, "Unsubscribed", id);
    }
    async function addItem(id) {
      await api("POST", `/api/v1/list/${id}/add/item`, { id: 0, list_id: id, item_id: 2, hq: null, quantity: 1, acquired: 0 });
    }
    async function delivery(expectedTarget) {
      const start = await owner.evaluate(() => window.lifecycle.messages.length);
      await addItem(target);
      await addItem(control);
      await owner.waitForFunction(start => window.lifecycle.messages.slice(start)
        .some(message => message.SubscriptionEvent?.subscription_id === 21 && message.SubscriptionEvent.event.ListDocUpdate), {}, start);
      // Allow queued duplicate authorization work to surface. Bounded retention
      // and exact authorization counts are additionally asserted natively.
      await new Promise(resolve => setTimeout(resolve, 500));
      const events = await owner.evaluate(start => window.lifecycle.messages.slice(start)
        .map(message => message.SubscriptionEvent).filter(event => event?.event.ListDocUpdate), start);
      assert.equal(events.filter(event => event.subscription_id === 11).length, expectedTarget, JSON.stringify(events));
      assert.equal(events.filter(event => event.subscription_id === 21).length, 1, JSON.stringify(events));
    }
    await subscribe(21, control);
    for (let index = 0; index < 40; index++) await subscribe(11, target);
    await delivery(1);
    console.log("[ok] forty same-ID handshakes deliver one update");
    await unsubscribe(11);
    await delivery(0);
    for (let id = 100; id < 180; id++) {
      await subscribe(id, target);
      await unsubscribe(id);
    }
    await subscribe(11, target);
    await delivery(1);
    console.log("[ok] unsubscribe and eighty navigation subscriptions leave no duplicate delivery");
    // Reuse the ID for a different list, then switch back. The old list must
    // not reactivate merely because its former ID is live again.
    await subscribe(11, control);
    const start = await owner.evaluate(() => window.lifecycle.messages.length);
    await addItem(target);
    await addItem(control);
    await owner.waitForFunction(start => window.lifecycle.messages.slice(start)
      .some(message => message.SubscriptionEvent?.subscription_id === 11 && message.SubscriptionEvent.event.ListDocUpdate), {}, start);
    await new Promise(resolve => setTimeout(resolve, 500));
    const updates = await owner.evaluate(start => window.lifecycle.messages.slice(start)
      .filter(message => message.SubscriptionEvent?.subscription_id === 11)
      .map(message => message.SubscriptionEvent.event.ListDocUpdate).filter(Boolean), start);
    assert.equal(updates.length, 1);
    assert.equal(updates[0].list_id, control);
    console.log("[ok] ID reuse relays only the replacement list");
    await sendAndWait({ SubscribeListDoc: { subscription_id: 11, list_id: -1472, version: "" } }, "SubscriptionEvent", 11);
    const failure = await owner.evaluate(() => window.lifecycle.messages
      .filter(message => message.SubscriptionEvent?.subscription_id === 11).at(-1));
    assert(failure.SubscriptionEvent.event.Error, "replacement handshake fails for a nonexistent list");
    await delivery(0);
    console.log("[ok] failed replacement retires the prior list relay");
    await owner.evaluate(() => window.lifecycle.socket.close());
  } finally {
    if (owner) for (const id of lists.reverse()) {
      await api("DELETE", `/api/v1/list/${id}/delete`).catch(error => console.error("fixture cleanup", error.message));
    }
    await browser.close();
  }
}

run().catch(error => { console.error(error); process.exitCode = 1; });
