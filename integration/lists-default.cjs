#!/usr/bin/env node
"use strict";

// Requires a fresh test-auth server. Verifies Lists 2.0 is the default in
// SSR, hydration and client navigation, including old Labs bookmarks.
const assert = require("node:assert/strict");
const puppeteer = require("puppeteer");
const copy = require("../ultros-frontend/ultros-i18n/locales/en.json");

async function main() {
  const base = process.env.BASE_URL || "http://127.0.0.1:8080";
  const timeout = Number(process.env.TIMEOUT_MS || 30000);
  const browser = await puppeteer.launch({ headless: true, args: ["--no-sandbox"] });
  const page = await browser.newPage();
  await page.setCacheEnabled(false);
  page.setDefaultNavigationTimeout(Math.max(timeout, 60000));
  // These navigations are intentional and this fixture has no player edits.
  page.on("dialog", (dialog) => dialog.type() === "beforeunload" ? dialog.accept() : dialog.dismiss());
  page.setDefaultTimeout(timeout);
  await page.setViewport({ width: 1280, height: 900 });
  await page.setCookie({ name: "HIDE_ADS", value: "true", url: base, path: "/" });
  await page.evaluateOnNewDocument(() => {
    window.__labsHydrated = false;
    window.addEventListener("ultros:hydrated", () => { window.__labsHydrated = true; });
  });
  const errors = [];
  page.on("pageerror", (error) => errors.push(error.message));
  const api = async (method, path, body) => page.evaluate(async ({ method, path, body }) => {
    const response = await fetch(path, {
      method,
      headers: body === undefined ? {} : { "Content-Type": "application/json" },
      body: body === undefined ? undefined : JSON.stringify(body),
    });
    const text = await response.text();
    if (!response.ok) throw new Error(`${method} ${path}: ${response.status} ${text}`);
    return text ? JSON.parse(text) : null;
  }, { method, path, body });
  let listId;
  try {
    const login = new URL("/test/login", base);
    login.search = new URLSearchParams({
      user_id: "990000001353", username: "LabsGateOwner", redirect: "/list?lang=en",
    }).toString();
    const response = await page.goto(login.toString(), { waitUntil: "domcontentloaded" });
    assert(response.ok(), "test-auth login must succeed");
    await page.waitForFunction(() => window.__labsHydrated);
    const worlds = await api("GET", "/api/v1/world_data");
    const world = worlds.regions[0].datacenters[0].worlds[0].id;
    const name = `Labs gate ${Date.now()}`;
    await api("POST", "/api/v1/list/create", { name, wdr_filter: { World: world } });
    listId = (await api("GET", "/api/v1/list")).find((entry) => entry.list.name === name).list.id;
    const path = `/list/${listId}?lang=en`;
    const marker = '[data-testid="list-view-sync"]';

    async function waitForAccountSave() {
      await page.waitForSelector('[data-testid="account-list-save-state"]');
      await page.waitForFunction(expected => document.querySelector('[data-testid="account-list-save-state"]')
        ?.textContent.includes(expected), {}, copy.lists_online_local_saved);
    }

    async function loadAndCheck(suffix) {
      const response = await page.goto(new URL(path + suffix, base).toString(), { waitUntil: "domcontentloaded" });
      assert(response.ok(), "list SSR must succeed");
      const html = await response.text();
      assert.equal(html.includes('data-testid="list-view-sync"'), true, "SSR serves Lists 2.0");
      await page.waitForFunction(() => window.__labsHydrated);
      await page.waitForSelector('[data-testid="list-settings-btn"]');
      assert.equal(await page.$(marker) !== null, true, "hydration keeps Lists 2.0");
      // Account persistence is asynchronous. Reloading before its first save
      // intentionally triggers the unsaved-work guard rather than navigating.
      await page.waitForSelector('[data-testid="list-cart"]');
      assert.equal(await page.$('[data-testid="list-build-workspace"]'), null);
      await waitForAccountSave();
    }

    await loadAndCheck("");
    await page.setCookie({ name: "LABS", value: "analyzer-recipe", url: base, path: "/" });
    await loadAndCheck("");
    await page.setCookie({ name: "LABS", value: "lists-sync", url: base, path: "/" });
    await loadAndCheck("");
    await page.deleteCookie({ name: "LABS", url: base, path: "/" });
    await loadAndCheck("&labs=lists-sync&cart=legacy");
    assert(!(await page.cookies()).some((cookie) => cookie.name === "LABS"), "URL override does not persist a cookie");
    await loadAndCheck("");
    console.log("[ok] absent, retired, valid cookie and URL-only override agree in SSR and hydration");

    await page.goto(new URL("/settings?lang=en", base).toString(), { waitUntil: "domcontentloaded" });
    await page.waitForFunction(() => window.__labsHydrated);
    assert.equal(await page.$('[data-testid="labs-settings"]'), null, "retired Labs settings are absent");
    const documentToken = `labs-${Date.now()}`;
    await page.evaluate((token) => { window.__labsDocumentToken = token; }, documentToken);
    async function clickAppLink(pathname) {
      await page.waitForFunction((pathname) => Array.from(document.querySelectorAll("a[href]"))
        .some((link) => new URL(link.href).pathname === pathname), {}, pathname);
      await page.evaluate((pathname) => Array.from(document.querySelectorAll("a[href]"))
        .find((link) => new URL(link.href).pathname === pathname).click(), pathname);
      await page.waitForFunction((pathname) => location.pathname === pathname, {}, pathname);
    }
    await clickAppLink("/list");
    await clickAppLink(`/list/${listId}`);
    await page.waitForSelector(marker);
    await waitForAccountSave();
    assert.equal(await page.evaluate(() => window.__labsDocumentToken), documentToken,
      "list navigation uses the current hydrated document");
    console.log("[ok] default account editor survives SSR, hydration, old bookmarks and client navigation");
    assert.deepEqual(errors, [], "no browser errors during navigation and hydration");
  } catch (error) {
    console.error("Default list state:", await page.evaluate(() => ({
      url: location.href,
      status: document.querySelector('[data-testid="account-list-save-state"]')?.textContent,
      text: document.body.innerText.slice(0, 2500),
    })), errors);
    throw error;
  } finally {
    try {
      if (listId !== undefined) await api("DELETE", `/api/v1/list/${listId}/delete`);
    } finally {
      await browser.close();
    }
  }
}

main().catch((error) => { console.error(error); process.exitCode = 1; });
