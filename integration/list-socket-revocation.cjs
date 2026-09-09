"use strict";

// Run against an isolated test-auth server. Exercise the actual websocket and
// permission lookup, including group-derived access, without relying on UI state.
const assert = require("node:assert/strict");
const puppeteer = require("puppeteer");

async function run() {
  const base = process.env.BASE_URL || "http://127.0.0.1:8080";
  const browser = await puppeteer.launch({ headless: true, args: ["--no-sandbox"] });
  const contexts = [];
  const lists = [];
  const groups = [];
  let owner;
  async function api(page, method, path, body) {
    const response = await page.evaluate(async ({ method, path, body }) => {
      const result = await fetch(path, {
        method,
        headers: body === undefined ? {} : { "Content-Type": "application/json" },
        body: body === undefined ? undefined : JSON.stringify(body),
      });
      const text = await result.text();
      return { status: result.status, body: text ? JSON.parse(text) : null };
    }, { method, path, body });
    assert.equal(response.status, 200, `${method} ${path}: ${JSON.stringify(response.body)}`);
    return response.body;
  }
  try {
    const seed = Date.now();
    const readerId = seed * 1000 + 2;
    async function actor(id, username) {
      const context = await browser.createBrowserContext();
      contexts.push(context);
      const page = await context.newPage();
      page.setDefaultTimeout(30000);
      const url = new URL("/test/login", base);
      url.searchParams.set("user_id", id);
      url.searchParams.set("username", username);
      url.searchParams.set("redirect", "/welcome?lang=en");
      const response = await page.goto(url.href, { waitUntil: "domcontentloaded" });
      assert(response && response.ok(), "test-auth login succeeded");
      return page;
    }
    owner = await actor(seed * 1000 + 1, "SocketGateOwner");
    const reader = await actor(readerId, "SocketGateReader");
    const worldData = await api(owner, "GET", "/api/v1/world_data");
    const world = worldData.regions[0].datacenters[0].worlds[0].id;
    async function createList(suffix) {
      const name = `Socket gate ${seed} ${suffix}`;
      await api(owner, "POST", "/api/v1/list/create", { name, wdr_filter: { World: world } });
      const entries = await api(owner, "GET", "/api/v1/list");
      const id = entries.find(entry => entry.list.name === name)?.list.id;
      assert(id, "created list exists");
      lists.push(id);
      return id;
    }
    async function shareUser(id) {
      await api(owner, "POST", `/api/v1/list/${id}/share/user`, {
        user_id: readerId, permission: "Read",
      });
    }
    async function addItem(id) {
      await api(owner, "POST", `/api/v1/list/${id}/add/item`, {
        id: 0, list_id: id, item_id: 2, hq: null, quantity: 1, acquired: 0,
      });
    }
    for (const mode of ["direct", "group-share", "group-member"]) {
      const target = await createList(`${mode} target`);
      const control = await createList(`${mode} control`);
      await shareUser(control);
      let group;
      if (mode === "direct") {
        await shareUser(target);
      } else {
        group = (await api(owner, "POST", "/api/v1/group/create", { name: `Socket ${seed} ${mode}` })).id;
        groups.push(group);
        await api(owner, "POST", `/api/v1/group/${group}/member/add/${readerId}`);
        await api(owner, "POST", `/api/v1/list/${target}/share/group`, { group_id: group, permission: "Read" });
      }
      await reader.evaluate(async ({ target, control }) => {
        window.socketGate?.socket.close();
        const url = new URL("/api/v1/realtime/events", location.href);
        url.protocol = location.protocol === "https:" ? "wss:" : "ws:";
        const socket = new WebSocket(url);
        const messages = [];
        window.socketGate = { socket, messages };
        socket.addEventListener("message", event => messages.push(JSON.parse(event.data)));
        await new Promise((resolve, reject) => {
          socket.addEventListener("open", resolve, { once: true });
          socket.addEventListener("error", reject, { once: true });
        });
        for (const [id, list] of [[11, target], [21, control]]) {
          socket.send(JSON.stringify({ SubscribeListDoc: { subscription_id: id, list_id: list, version: "" } }));
          socket.send(JSON.stringify({ SubscribeList: { subscription_id: id + 1, list_id: list } }));
        }
      }, { target, control });
      await reader.waitForFunction(() => [11, 21].every(id => window.socketGate.messages.some(
        message => message.ListDocSubscribed?.subscription_id === id,
      )) && [12, 22].every(id => window.socketGate.messages.some(
        message => message.Subscribed?.subscription_id === id,
      )));
      const start = await reader.evaluate(() => window.socketGate.messages.length);
      if (mode === "direct") {
        await api(owner, "DELETE", `/api/v1/list/${target}/share/user/${readerId}`);
      } else if (mode === "group-share") {
        await api(owner, "DELETE", `/api/v1/list/${target}/share/group/${group}`);
      } else {
        await api(owner, "DELETE", `/api/v1/group/${group}/member/remove/${readerId}`);
      }
      await addItem(target);
      await addItem(control);
      await reader.waitForFunction(start => {
        const events = window.socketGate.messages.slice(start).map(m => m.SubscriptionEvent).filter(Boolean);
        return [11, 12].every(id => events.some(e => e.subscription_id === id && e.event.Error))
          && events.some(e => e.subscription_id === 21 && e.event.ListDocUpdate)
          && events.some(e => e.subscription_id === 22 && e.event.ListUpdate);
      }, {}, start);
      // A second edit proves revoked streams stay inactive while another list
      // on the same socket continues to deliver events.
      const second = await reader.evaluate(() => window.socketGate.messages.length);
      await addItem(target);
      await addItem(control);
      await reader.waitForFunction(second => window.socketGate.messages.slice(second).some(
        m => m.SubscriptionEvent?.subscription_id === 21 && m.SubscriptionEvent.event.ListDocUpdate,
      ), {}, second);
      const events = await reader.evaluate(start => window.socketGate.messages.slice(start), start);
      for (const message of events) {
        const event = message.SubscriptionEvent;
        if (event && [11, 12].includes(event.subscription_id)) {
          assert(event.event.Error?.message.includes("forbidden"), `${mode}: revoked subscription leaked ${JSON.stringify(event)}`);
        }
      }
      console.log(`[ok] ${mode} revocation stops document and legacy payloads; control subscriptions stay live`);
    }
  } finally {
    if (owner) {
      for (const id of lists.reverse()) {
        await api(owner, "DELETE", `/api/v1/list/${id}/delete`).catch(error => console.error("fixture cleanup", error.message));
      }
      for (const id of groups.reverse()) {
        await api(owner, "DELETE", `/api/v1/group/${id}`).catch(error => console.error("fixture cleanup", error.message));
      }
    }
    for (const context of contexts) await context.close();
    await browser.close();
  }
}

run().catch(error => { console.error(error); process.exitCode = 1; });
