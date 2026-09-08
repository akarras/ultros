#!/usr/bin/env node
"use strict";

// Requires a fresh test-auth server. Exercises the real cookie, SSR branch,
// hydration, and Settings switch; it creates and deletes its own empty list.
const assert = require("node:assert/strict");
const puppeteer = require("puppeteer");

async function main() {
  const base = process.env.BASE_URL || "http://127.0.0.1:8080";
  const timeout = Number(process.env.TIMEOUT_MS || 30000);
  const browser = await puppeteer.launch({ headless: true, args: ["--no-sandbox"] });
  const page = await browser.newPage();
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

    async function loadAndCheck(suffix, expected) {
      const response = await page.goto(new URL(path + suffix, base).toString(), { waitUntil: "domcontentloaded" });
      assert(response.ok(), "list SSR must succeed");
      const html = await response.text();
      assert.equal(html.includes('data-testid="list-view-sync"'), expected, "SSR chooses the expected Labs branch");
      await page.waitForFunction(() => window.__labsHydrated);
      await page.waitForSelector('[data-testid="list-settings-btn"]');
      assert.equal(await page.$(marker) !== null, expected, "hydration preserves the SSR branch");
    }

    await loadAndCheck("", false);
    await page.setCookie({ name: "LABS", value: "analyzer-recipe", url: base, path: "/" });
    await loadAndCheck("", false);
    await page.setCookie({ name: "LABS", value: "lists-sync", url: base, path: "/" });
    await loadAndCheck("", true);
    await page.deleteCookie({ name: "LABS", url: base, path: "/" });
    await loadAndCheck("&labs=lists-sync", true);
    assert(!(await page.cookies()).some((cookie) => cookie.name === "LABS"), "URL override does not persist a cookie");
    await loadAndCheck("", false);
    console.log("[ok] absent, retired, valid cookie and URL-only override agree in SSR and hydration");

    await page.goto(new URL("/settings?lang=en", base).toString(), { waitUntil: "domcontentloaded" });
    await page.waitForFunction(() => window.__labsHydrated);
    const switchSelector = '[data-testid="lab-lists-sync"] input[role="switch"]';
    const isChecked = () => page.$eval(switchSelector, (input) => input.checked);
    assert.equal(await isChecked(), false);
    await page.$eval(switchSelector, (input) => input.click());
    await page.waitForFunction(() => document.cookie.includes("LABS=lists-sync"));
    await page.reload({ waitUntil: "domcontentloaded" });
    await page.waitForFunction(() => window.__labsHydrated);
    assert.equal(await isChecked(), true, "enabling survives reload");
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
    assert.equal(await page.evaluate(() => window.__labsDocumentToken), documentToken,
      "list navigation uses the current hydrated document");
    console.log("[ok] Settings cookie selects the Labs branch during client-side list navigation");
    await loadAndCheck("", true);

    await page.goto(new URL("/settings?lang=en", base).toString(), { waitUntil: "domcontentloaded" });
    await page.waitForFunction(() => window.__labsHydrated);
    await page.$eval(switchSelector, (input) => input.click());
    await page.waitForFunction(() => !document.cookie.split(";").some((part) => part.trim().startsWith("LABS=")));
    await page.reload({ waitUntil: "domcontentloaded" });
    await page.waitForFunction(() => window.__labsHydrated);
    assert.equal(await isChecked(), false, "disabling survives reload");
    await loadAndCheck("", false);
    console.log("[ok] Settings enables and removes the cookie across reloads");
    assert.deepEqual(errors, [], "no browser errors during Labs navigation and hydration");
  } finally {
    try {
      if (listId !== undefined) await api("DELETE", `/api/v1/list/${listId}/delete`);
    } finally {
      await browser.close();
    }
  }
}

main().catch((error) => { console.error(error); process.exitCode = 1; });
