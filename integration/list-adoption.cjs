#!/usr/bin/env node
"use strict";

// Requires a freshly built test-auth server. Uses an anonymous browser first,
// then authenticates in the same context so device storage is never replaced.
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const { capture } = require("./capture.cjs");
const owner = { id: 990000000811, username: "DeviceAdoptionOwner" };
const other = { id: 990000000812, username: "DeviceAdoptionOther" };
const tid = id => `[data-testid="${id}"]`;

async function main() {
  const { default: puppeteer } = await import("puppeteer");
  const base = process.env.BASE_URL || "http://127.0.0.1:8080";
  const timeout = Number(process.env.TIMEOUT_MS || 60000);
  const browser = await puppeteer.launch({ headless: process.env.HEADLESS !== "false", args: ["--no-sandbox"] });
  // Puppeteer supplies a fresh disposable profile. A regular context preserves
  // normal CacheStorage limits for the large unoptimized development WASM.
  const context = browser.defaultBrowserContext();
  const page = await context.newPage();
  const diagnostics = [];
  page.on("pageerror", error => diagnostics.push(`pageerror: ${error.stack || error.message}`));
  page.on("console", message => {
    if (message.type() === "error") diagnostics.push(`console.error: ${message.text()}`);
  });
  page.on("requestfailed", request => diagnostics.push(`requestfailed: ${request.url()} ${request.failure()?.errorText}`));
  page.on("response", response => {
    if (response.status() >= 400) diagnostics.push(`HTTP ${response.status()}: ${response.url()}`);
  });
  page.setDefaultTimeout(timeout);
  await page.setViewport({ width: 1280, height: 1000 });
  await page.setCookie(
    { name: "LABS", value: "lists-sync", url: base, path: "/" },
    { name: "HIDE_ADS", value: "true", url: base, path: "/" },
    { name: "i18n_pref_locale", value: "en", url: base, path: "/" },
  );
  await page.evaluateOnNewDocument(() => {
    window.__adoptionHydrated = false;
    window.addEventListener("ultros:hydrated", () => { window.__adoptionHydrated = true; });
  });
  const created = [];
  async function load(route) {
    const destination = new URL(route, base).href;
    const response = destination === page.url()
      ? await page.reload({ waitUntil: "domcontentloaded" })
      : await page.goto(destination, { waitUntil: "domcontentloaded" });
    assert(response?.ok(), `navigation ${route}: ${response?.status()}`);
    await page.waitForFunction(() => window.__adoptionHydrated);
  }
  async function login(user, redirect) {
    const url = new URL("/test/login", base);
    url.searchParams.set("user_id", user.id);
    url.searchParams.set("username", user.username);
    url.searchParams.set("redirect", redirect);
    await load(url.href);
  }
  async function api(method, route, body) {
    return page.evaluate(async ({ method, route, body }) => {
      const response = await fetch(route, {
        method, credentials: "include",
        headers: body === undefined ? {} : { "Content-Type": "application/json" },
        body: body === undefined ? undefined : JSON.stringify(body),
      });
      const text = await response.text();
      let data;
      try { data = JSON.parse(text); } catch { data = text; }
      return { status: response.status, body: data };
    }, { method, route, body });
  }
  async function replace(selector, value) {
    await page.waitForSelector(selector, { visible: true });
    await page.click(selector, { clickCount: 3 });
    await page.keyboard.press("Backspace");
    await page.type(selector, String(value));
  }
  const waitValue = (selector, value) => page.waitForFunction(
    (selector, value) => document.querySelector(selector)?.value === value, {}, selector, String(value));
  const saved = () => page.waitForFunction(() =>
    document.querySelector('[data-testid="device-list-status"]')?.textContent === "Saved on this device");
  async function openStorage() {
    if (await page.$(tid("device-list-storage-details"))) {
      await page.$eval(tid("device-list-storage-details"), details => { details.open = true; });
    }
  }
  const needed = 'input[aria-label="Needed for Bronze Ingot"]';
  const owned = 'input[aria-label="Owned for Bronze Ingot"]';
  const target = 'input[aria-label="Target price for Bronze Ingot"]';
  try {
    await load("/list?lang=en");
    const name = `Adoption regression ${Date.now()}`;
    await replace(tid("device-list-name"), name);
    await page.click(tid("device-list-create"));
    await page.waitForFunction(() => location.pathname.startsWith("/list/device/"));
    const devicePath = new URL(page.url()).pathname + "?labs=lists-sync";
    const deviceId = new URL(page.url()).pathname.split("/").at(-1);
    await saved();
    await replace('input[aria-label="Quantity to add"]', 3);
    await replace('input[aria-label="Add an item"]', "Bronze Ingot");
    await page.waitForSelector('button[aria-label="Add Bronze Ingot"]');
    await page.keyboard.press("Enter");
    await waitValue(needed, 3);
    await page.keyboard.press("Escape");
    await replace(owned, 1);
    await page.keyboard.press("Enter");
    await replace(target, 125);
    await page.keyboard.press("Enter");
    await saved();
    assert.equal((await context.cookies()).some(cookie => cookie.name === "discord_auth"), false);
    await login(owner, devicePath);
    await waitValue(needed, 3);
    await waitValue(owned, 1);
    await waitValue(target, 125);
    console.log("[step] authenticated device list retained its items");
    const section = 'section[aria-label="Add device list to account"]';
    await openStorage();
    await page.waitForSelector(tid("device-list-adopt"));
    // Pick a real scope through the same combobox a player uses.
    await page.$eval(`${section} input[role="combobox"]`, input => input.focus());
    const world = process.env.WORLD || "Gilgamesh";
    await page.keyboard.type(world);
    await page.waitForFunction(world => Array.from(document.querySelectorAll('[role="option"]')).some(
      option => option.textContent.includes(world)), {}, world);
    await page.keyboard.press("Enter");
    await page.keyboard.press("Tab");
    await page.waitForFunction(() => !document.querySelector('[data-testid="device-list-adopt"]').disabled);
    console.log("[step] adoption scope selected");

    // Hold the first request so an edit made during transfer exercises the
    // acknowledged-revision warning rather than only the happy path.
    await page.setRequestInterception(true);
    let release;
    const held = new Promise(resolve => { release = resolve; });
    let intercepted;
    const requestSeen = new Promise(resolve => { intercepted = resolve; });
    const holdFirstAdoption = request => {
      if (new URL(request.url()).pathname === "/api/v1/list/adopt" && request.method() === "POST") {
        intercepted(JSON.parse(request.postData()));
        held.then(() => request.continue()).catch(() => {});
      } else request.continue().catch(() => {});
    };
    page.on("request", holdFirstAdoption);
    await page.click(tid("device-list-adopt"));
    let requestTimer;
    const payload = await Promise.race([requestSeen,
      new Promise((_, reject) => { requestTimer = setTimeout(() => reject(new Error("adoption request missing")), timeout); })])
      .finally(() => clearTimeout(requestTimer));
    const duplicate = await api("POST", "/api/v1/list/create", { name, wdr_filter: payload.wdr_filter });
    assert.equal(duplicate.status, 200);
    const before = await api("GET", "/api/v1/list");
    const sameName = before.body.filter(entry => entry.list.name === name);
    assert.equal(sameName.length, 1);
    const existingId = sameName[0].list.id;
    created.push(existingId);
    await replace(needed, 7);
    await page.keyboard.press("Enter");
    await waitValue(needed, 7);
    await saved();
    const responsePromise = page.waitForResponse(response =>
      new URL(response.url()).pathname === "/api/v1/list/adopt" && response.request().method() === "POST");
    release();
    const response = await responsePromise;
    assert.equal(response.status(), 200);
    const receipt = await response.json();
    // The held transfer is complete. Stop interception before navigation:
    // Puppeteer interception can interfere with service-worker document loads.
    await page.setRequestInterception(false);
    page.off("request", holdFirstAdoption);
    created.push(receipt.list_id);
    assert.notEqual(receipt.list_id, existingId, "same names must not silently merge");
    assert.equal(receipt.owner, owner.id);
    assert.equal(receipt.device_list_id, deviceId);
    await page.waitForFunction(() => document.body.innerText.includes("Newer edits remain in this device copy"));
    await waitValue(needed, 7);

    const account = await api("GET", `/api/v1/list/${receipt.list_id}/listings`);
    assert.equal(account.status, 200);
    assert.equal(account.body[1].length, 1);
    const row = account.body[1][0][0];
    assert.equal(row.item_id, payload.items[0].item_id);
    assert.equal(row.quantity, 3);
    assert.equal(row.acquired, 1);
    assert.equal(row.target_price, 125);
    assert.equal(row.hq, payload.items[0].hq);
    const retry = await api("POST", "/api/v1/list/adopt", payload);
    assert.equal(retry.status, 200);
    assert.deepEqual(retry.body, receipt, "identical retry recovers original receipt");
    const newer = structuredClone(payload);
    newer.source_revision += "-newer";
    newer.items[0].quantity = 99;
    const retriedNewer = await api("POST", "/api/v1/list/adopt", newer);
    assert.equal(retriedNewer.status, 200);
    assert.deepEqual(retriedNewer.body, receipt, "retry never claims a newer revision was imported");
    const after = await api("GET", "/api/v1/list");
    assert.equal(after.body.filter(entry => entry.list.name === name).length, 2);
    await load(devicePath);
    await waitValue(needed, 7);
    await openStorage();
    await page.waitForFunction(() => document.body.innerText.includes("Newer edits remain in this device copy"));
    console.log("[ok] guest items survive login; adoption preserves projection, separates matching names, retains edits and retries safely");

    await login(other, devicePath);
    await waitValue(needed, 7);
    const wrongOwner = await api("POST", "/api/v1/list/adopt", payload);
    assert(wrongOwner.status >= 400 && wrongOwner.status < 500,
      `stale destination account should be rejected, got ${wrongOwner.status}`);
    const otherLists = await api("GET", "/api/v1/list");
    assert.equal(otherLists.body.some(entry => entry.list.name === name), false);
    assert.equal(await page.$(`${section} a[href="/list/${receipt.list_id}?labs=lists-sync"]`), null,
      "previous account's adoption receipt is not presented after account switching");
    console.log("[ok] account switching cannot send a stale adoption payload to another account");

    await login(owner, "/list?labs=lists-sync");
    const batchLists = [];
    for (const quantity of [2, 4]) {
      const batchName = `${name} batch ${quantity}`;
      await replace(tid("device-list-name"), batchName);
      await page.click(tid("device-list-create"));
      await page.waitForFunction(() => location.pathname.startsWith("/list/device/"));
      const id = new URL(page.url()).pathname.split("/").at(-1);
      await saved();
      await replace('input[aria-label="Quantity to add"]', quantity);
      await replace('input[aria-label="Add an item"]', "Bronze Ingot");
      await page.waitForSelector('button[aria-label="Add Bronze Ingot"]');
      await page.keyboard.press("Enter");
      await waitValue(needed, quantity);
      await page.keyboard.press("Escape");
      await saved();
      batchLists.push({ id, name: batchName, quantity });
      await load("/list?labs=lists-sync");
    }
    const batchSection = tid("device-lists-adoption");
    await page.waitForSelector(batchSection);
    await page.click(`${batchSection} summary`);
    await page.waitForFunction((selector, username) =>
      document.querySelector(selector)?.innerText.includes(`Add to ${username}'s account`),
    {}, batchSection, owner.username);
    const adoptedCheckbox = `${batchSection} input[aria-label="Add ${name} to account"]`;
    assert.equal(await page.$eval(adoptedCheckbox, input => input.disabled), true,
      "the directory disables a list already acknowledged for this account");
    for (const list of batchLists) {
      const checkbox = `${batchSection} input[aria-label="Add ${list.name} to account"]`;
      await page.waitForSelector(checkbox, { visible: true });
      assert.equal(await page.$eval(checkbox, input => input.disabled), false);
      await page.click(checkbox);
    }
    await page.$eval(`${batchSection} input[role="combobox"]`, input => input.focus());
    await page.keyboard.type(world);
    await page.waitForFunction(world => Array.from(document.querySelectorAll('[role="option"]')).some(
      option => option.textContent.includes(world)), {}, world);
    await page.keyboard.press("Enter");
    await page.keyboard.press("Tab");
    await page.waitForFunction(() => {
      const button = document.querySelector('[data-testid="device-lists-adopt-selected"]');
      return !button?.disabled && button.textContent.includes("Add 2 selected lists");
    });
    const batchRequests = [];
    const recordBatch = request => {
      if (new URL(request.url()).pathname === "/api/v1/list/adopt" && request.method() === "POST") {
        batchRequests.push(JSON.parse(request.postData()));
      }
    };
    page.on("request", recordBatch);
    await page.click(tid("device-lists-adopt-selected"));
    await page.waitForFunction((ids, ownerId) => ids.every(id =>
      localStorage.getItem(`ultros:device-adoption:v1:${ownerId}:${id}:receipt`)),
    {}, batchLists.map(list => list.id), owner.id);
    await page.waitForFunction(() => {
      const button = document.querySelector('[data-testid="device-lists-adopt-selected"]');
      return button?.disabled && button.textContent.includes("Add 0 selected lists");
    });
    page.off("request", recordBatch);
    assert.equal(batchRequests.length, 2, "one batch action dispatches exactly the two selected lists");
    assert(batchRequests.every(request => request.expected_owner === owner.id),
      "each transfer binds the same explicitly displayed account");
    assert.deepEqual(batchRequests.map(request => request.device_list_id).sort(), batchLists.map(list => list.id).sort());
    for (const list of batchLists) {
      const receipt = await page.evaluate(({ id, ownerId }) =>
        JSON.parse(localStorage.getItem(`ultros:device-adoption:v1:${ownerId}:${id}:receipt`)),
      { id: list.id, ownerId: owner.id });
      created.push(receipt.list_id);
      const imported = await api("GET", `/api/v1/list/${receipt.list_id}/listings`);
      assert.equal(imported.status, 200);
      assert.equal(imported.body[1].length, 1);
      assert.equal(imported.body[1][0][0].quantity, list.quantity);
      assert.equal(imported.body[1][0][0].item_id, payload.items[0].item_id);
      const checkbox = `${batchSection} input[aria-label="Add ${list.name} to account"]`;
      assert.equal(await page.$eval(checkbox, input => input.disabled && !input.checked), true);
      assert(await page.$(`${batchSection} a[href="/list/${receipt.list_id}?labs=lists-sync"]`),
        "each imported list exposes its own account destination");
    }
    const batchAccount = await api("GET", "/api/v1/list");
    for (const list of batchLists) {
      assert.equal(batchAccount.body.filter(entry => entry.list.name === list.name).length, 1);
      await load(`/list/device/${list.id}?labs=lists-sync`);
      await waitValue(needed, list.quantity);
      assert.equal(await page.$eval('input[aria-label="List name"]', input => input.value), list.name,
        "batch adoption retains each intact local source");
    }
    console.log("[ok] two selected device lists transfer in one account-bound action and retain independent local copies");
  } catch (error) {
    const artifacts = path.join(__dirname, "artifacts", "list-adoption");
    fs.mkdirSync(artifacts, { recursive: true });
    await capture(page, { path: path.join(artifacts, "failure.png"), fullPage: true }).catch(() => {});
    console.error("Adoption page:", page.url(), await page.$eval("body", body => body.innerText).catch(() => "unavailable"));
    console.error("Browser diagnostics:", diagnostics);
    throw error;
  } finally {
    if (created.length) {
      await login(owner, "/list").catch(() => {});
      for (const id of created) await api("DELETE", `/api/v1/list/${id}/delete`).catch(() => {});
    }
    await browser.close();
  }
}
main().catch(error => { console.error(error); process.exitCode = 1; });
