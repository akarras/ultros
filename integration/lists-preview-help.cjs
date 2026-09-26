#!/usr/bin/env node
"use strict";

// Anonymous default Lists journey, including retired Labs links/cookies. No test-auth required.
// BASE_URL=http://127.0.0.1:8080 node integration/lists-preview-help.cjs
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const { capture } = require("./capture.cjs");

async function main() {
  const { default: puppeteer } = await import("puppeteer");
  const base = process.env.BASE_URL || "http://127.0.0.1:8080";
  const artifacts = path.join(__dirname, "artifacts", "lists-preview-help");
  fs.mkdirSync(artifacts, { recursive: true });
  const copy = lang => JSON.parse(fs.readFileSync(path.join(__dirname,
    "../ultros-frontend/ultros-i18n/locales", `${lang}.json`), "utf8"));
  const en = copy("en");
  const browser = await puppeteer.launch({ headless: true, args: ["--no-sandbox"] });
  const page = await browser.newPage();
  page.setDefaultTimeout(Number(process.env.TIMEOUT_MS || 60000));
  await page.setViewport({ width: 1280, height: 900 });
  await page.setCookie({ name: "HIDE_ADS", value: "true", url: base, path: "/" });
  await page.evaluateOnNewDocument(() => {
    window.__previewHelpHydrated = false;
    window.addEventListener("ultros:hydrated", () => { window.__previewHelpHydrated = true; });
  });
  const errors = [];
  const accountWrites = [];
  page.on("pageerror", error => errors.push(error.message));
  page.on("request", request => {
    if (request.method() !== "GET" && /^\/api\/v1\/list(?:\/|$)/.test(new URL(request.url()).pathname)) {
      accountWrites.push(`${request.method()} ${request.url()}`);
    }
  });
  const id = value => `[data-testid="${value}"]`;
  async function load(route) {
    const response = await page.goto(new URL(route, base).href, { waitUntil: "domcontentloaded" });
    assert(response?.ok(), `${route}: ${response?.status()}`);
    await page.waitForFunction(() => window.__previewHelpHydrated);
    return response;
  }
  async function visibleText(value) {
    await page.waitForFunction(value => document.body.innerText.includes(value), {}, value);
  }
  async function replace(selector, value) {
    await page.waitForSelector(selector, { visible: true });
    await page.click(selector);
    await page.$eval(selector, input => input.select());
    await page.keyboard.press("Backspace");
    await page.type(selector, String(value));
  }
  async function saved() {
    await page.waitForFunction(() => document.querySelector('[data-testid="device-list-status"]')
      ?.textContent.includes("Saved on this device"));
  }
  try {
    for (const lang of ["de", "fr", "ja", "cn", "ko", "tc", "en"]) {
      await load(`/settings?lang=${lang}`);
      assert.equal(await page.$(id("labs-settings")), null, `${lang}: retired Labs controls are absent`);
    }
    assert(!(await page.cookies()).some(cookie => cookie.name === "LABS"));
    const token = `preview-${Date.now()}`;
    await page.evaluate(token => { window.__previewHelpDocument = token; }, token);
    await page.evaluate(() => Array.from(document.querySelectorAll("a[href]"))
      .find(link => new URL(link.href).pathname === "/list").click());
    await page.waitForSelector(id("list-new"));
    assert.equal(await page.evaluate(() => window.__previewHelpDocument), token,
      "Settings -> Lists uses the hydrated document without opt-in");
    await capture(page, { path: path.join(artifacts, "device-directory-help.png"), fullPage: true });
    await page.click(id("list-new"));
    await replace(id("device-list-name"), `Preview help ${Date.now()}`);
    await page.click(id("device-list-create"));
    await page.waitForFunction(() => location.pathname.startsWith("/list/device/"));
    const devicePath = new URL(page.url()).pathname;
    await saved();
    const needed = 'input[aria-label="Needed for Bronze Ingot"]';
    await replace('input[aria-label="Add an item"]', "Bronze Ingot");
    await page.waitForSelector('button[aria-label="Add Bronze Ingot"]');
    await page.keyboard.press("Enter");
    await page.waitForSelector(needed);
    await replace(needed, 4);
    await page.keyboard.press("Enter");
    await saved();
    await page.click(id("list-undo"));
    await page.waitForFunction(selector => document.querySelector(selector)?.value === "1", {}, needed);
    await saved();
    await page.click(id("device-list-storage-toggle"));
    await visibleText(en.guest_workspace_backup_warning);
    await capture(page, { path: path.join(artifacts, "device-build-undo-help.png"), fullPage: true });
    await page.click('button[aria-label="Close modal"]');
    await page.click(id("guest-shop-mode"));
    await visibleText(en.guest_workspace_shop_intro);
    await page.waitForSelector(id("shop-cart-summary"), { visible: true });
    await capture(page, { path: path.join(artifacts, "device-shop-help.png"), fullPage: true });
    await page.click(id("guest-build-mode"));
    await page.waitForSelector(needed, { visible: true });
    await saved();
    await load(`${devicePath}?lang=en`);
    await page.waitForSelector(needed);
    assert.equal(await page.$eval(needed, input => input.value), "1", "plain device URL retains edits");
    await page.setCookie({ name: "LABS", value: "lists-sync", url: base, path: "/" });
    await load(`${devicePath}?labs=lists-sync&lang=en`);
    await page.waitForSelector(needed);
    assert.equal(await page.$eval(needed, input => input.value), "1", "old preview links still open the same document");
    await page.setViewport({ width: 390, height: 844 });
    await capture(page, { path: path.join(artifacts, "default-device-mobile.png"), fullPage: true });
    assert.deepEqual(accountWrites, [], "anonymous preview makes no account-list writes");
    assert.deepEqual(errors, [], "no uncaught browser errors");
    console.log("PASS: default anonymous create/edit/Undo/Build/Shop/reload, retired Labs compatibility, seven-locale settings");
  } finally {
    await browser.close();
  }
}

main().catch(error => { console.error(error); process.exitCode = 1; });
