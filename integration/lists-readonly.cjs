"use strict";

// Run only against an isolated test-auth server started with
// ULTROS_LISTS_READONLY=true. REST fixture writes remain supported while paused.
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");

async function main() {
  const { default: puppeteer } = await import("puppeteer");
  const base = process.env.BASE_URL || "http://127.0.0.1:8080";
  const browser = await puppeteer.launch({ headless: true, args: ["--no-sandbox"] });
  const page = await browser.newPage();
  page.setDefaultTimeout(Number(process.env.TIMEOUT_MS || 60000));
  page.setDefaultNavigationTimeout(60000);
  const errors = [];
  page.on("pageerror", error => errors.push(error.message));
  await page.setCookie({ name: "HIDE_ADS", value: "true", url: base, path: "/" });
  await page.evaluateOnNewDocument(() => {
    window.__readonlyHydrated = false;
    window.__listDocSends = [];
    window.addEventListener("ultros:hydrated", () => { window.__readonlyHydrated = true; });
    const send = WebSocket.prototype.send;
    WebSocket.prototype.send = function (message) {
      if (typeof message === "string" && /"(?:SubscribeListDoc|ListDocUpdate)"/.test(message)) {
        window.__listDocSends.push(message);
      }
      return send.call(this, message);
    };
  });
  const api = (method, route, body) => page.evaluate(async ({ method, route, body }) => {
    const response = await fetch(route, {
      method, headers: body === undefined ? {} : { "Content-Type": "application/json" },
      body: body === undefined ? undefined : JSON.stringify(body),
    });
    if (!response.ok) throw new Error(`${method} ${route}: ${response.status}`);
    const text = await response.text();
    return text ? JSON.parse(text) : null;
  }, { method, route, body });
  const artifacts = path.join(__dirname, "artifacts", "lists-readonly");
  fs.mkdirSync(artifacts, { recursive: true });
  let listId;
  async function load(route) {
    const response = await page.goto(new URL(route, base).href, { waitUntil: "domcontentloaded" });
    assert(response?.ok(), `${route} SSR succeeds`);
    const html = await response.text();
    assert(html.includes('"lists_readonly":true'), "server must be started with ULTROS_LISTS_READONLY=true");
    assert(html.includes('data-testid="lists-paused"'), "SSR renders the pause banner");
    await page.waitForFunction(() => window.__readonlyHydrated);
    await page.waitForSelector('[data-testid="lists-paused"]');
    assert.equal(await page.$('[data-testid="list-cart"]'), null, "no editing workspace opens");
    assert.equal(await page.$('[data-testid="list-new"]'), null, "no list creation control is offered");
    assert.deepEqual(await page.evaluate(() => window.__listDocSends), [], "paused routes never open a document socket");
    assert.deepEqual(await page.evaluate(() => Object.keys(localStorage)
      .filter(key => key.startsWith("ultros.listdoc.v1."))), [], "paused routes never create account snapshots");
    assert.notEqual(await page.evaluate(() => window.__ULTROS_GUEST_OFFLINE_READY__), true,
      "paused routes do not prepare an editable offline generation");
    return html;
  }
  try {
    await load("/list?lang=en");
    const login = new URL("/test/login", base);
    login.search = new URLSearchParams({
      user_id: "990000001665", username: "ListsReadonlyQA", redirect: "/list?lang=en",
    }).toString();
    const response = await page.goto(login.href, { waitUntil: "domcontentloaded" });
    assert(response?.ok(), "test-auth login succeeds");
    await page.waitForFunction(() => window.__readonlyHydrated);
    const worlds = await api("GET", "/api/v1/world_data");
    const world = worlds.regions[0].datacenters[0].worlds[0].id;
    const name = `Read-only promotion QA ${Date.now()}`;
    await api("POST", "/api/v1/list/create", { name, wdr_filter: { World: world } });
    listId = (await api("GET", "/api/v1/list")).find(entry => entry.list.name === name)?.list.id;
    assert(listId, "fixture account list exists");
    await api("POST", `/api/v1/list/${listId}/add/item`, {
      id: 0, list_id: listId, item_id: 5056, hq: false, quantity: 5, acquired: 2, target_price: null,
    });
    const before = (await api("GET", `/api/v1/list/${listId}/listings`))[1].map(([row]) => row);
    for (const [size, viewport] of [
      ["desktop", { width: 1280, height: 900 }], ["mobile", { width: 390, height: 844 }],
    ]) {
      await page.setViewport(viewport);
      await load("/list?lang=en");
      await page.waitForSelector(`a[href="/list/${listId}"]`);
      await page.screenshot({ path: path.join(artifacts, `${size}-directory.png`), fullPage: true });
      const html = await load(`/list/${listId}?lang=en&labs=lists-sync&cart=legacy`);
      assert(html.includes('data-testid="list-readonly"'), "account projection is server-rendered");
      await page.waitForSelector('[data-testid="list-readonly"]');
      assert.match(await page.$eval('[data-testid="list-readonly"]', node => node.textContent), /2 \/ 5/,
        "projected purchase progress stays readable");
      await page.screenshot({ path: path.join(artifacts, `${size}-account.png`), fullPage: true });
      await load("/list/device/device:readonly-qa?lang=en");
      assert.equal(await page.$('[data-testid="device-list-status"]'), null, "device documents stay closed");
      await page.screenshot({ path: path.join(artifacts, `${size}-device.png`), fullPage: true });
    }
    console.log("[ok] paused directory, account projection and device route agree in SSR and hydration at both widths");
    await load("/list?lang=en");
    const link = `a[href="/list/${listId}"]`;
    await page.waitForSelector(link);
    await page.evaluate(() => { window.__readonlyNavigation = "same-document"; });
    await page.click(link);
    await page.waitForSelector('[data-testid="list-readonly"]');
    assert.equal(await page.evaluate(() => window.__readonlyNavigation), "same-document",
      "the paused account route is also reached by client navigation");
    assert.deepEqual(await page.evaluate(() => window.__listDocSends), [], "client navigation never opens document sync");
    // A stale editable tab must be refused by the server even without reloading.
    const replies = await page.evaluate(async listId => {
      const url = new URL("/api/v1/realtime/events", location.href);
      url.protocol = location.protocol === "https:" ? "wss:" : "ws:";
      const socket = new WebSocket(url);
      return new Promise((resolve, reject) => {
        const messages = [];
        const timer = setTimeout(() => { socket.close(); reject(new Error("paused sync probe timed out")); }, 30000);
        socket.onopen = () => {
          socket.send(JSON.stringify({ SubscribeListDoc: { subscription_id: 91, list_id: listId, version: "" } }));
          socket.send(JSON.stringify({ ListDocUpdate: { list_id: listId, update: btoa("not-a-document") } }));
        };
        socket.onerror = () => { clearTimeout(timer); socket.close(); reject(new Error("sync socket failed")); };
        socket.onmessage = event => {
          const message = JSON.parse(event.data);
          // Every socket receives this greeting before any command replies.
          if (message === "SocketConnected") return;
          messages.push(message);
          if (messages.length === 2) { clearTimeout(timer); socket.close(); resolve(messages); }
        };
      });
    }, listId);
    assert.equal(replies[0].SubscriptionEvent?.subscription_id, 91,
      `handshake refusal is scoped to its subscription: ${JSON.stringify(replies)}`);
    assert.match(replies[0].SubscriptionEvent?.event.Error?.message || "", /temporarily read-only/);
    assert.match(replies[1].Error?.message || "", /temporarily read-only/, "updates are refused before document decoding");
    const after = (await api("GET", `/api/v1/list/${listId}/listings`))[1].map(([row]) => row);
    assert.deepEqual(after, before, "reading paused routes and rejected socket writes preserve the projection");
    assert.deepEqual(errors, [], "no hydration or browser errors while paused");
    console.log("[ok] stale clients cannot subscribe or submit document updates while editing is paused");
  } finally {
    try {
      if (listId !== undefined) await api("DELETE", `/api/v1/list/${listId}/delete`);
    } finally {
      await browser.close();
    }
  }
}

main().catch(error => { console.error(error); process.exitCode = 1; });
