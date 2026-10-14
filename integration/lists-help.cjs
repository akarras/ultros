#!/usr/bin/env node
"use strict";

// Fresh anonymous Settings -> device-list journey. No test-auth required.
// BASE_URL=http://127.0.0.1:8080 node integration/lists-help.cjs
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const { capture } = require("./capture.cjs");

async function main() {
  const { default: puppeteer } = await import("puppeteer");
  const base = process.env.BASE_URL || "http://127.0.0.1:8080";
  const artifacts = path.join(__dirname, "artifacts", "lists-help");
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
      assert.equal(await page.$(id("labs-settings")), null, `${lang}: retired Labs settings are absent`);
    }
    assert(!(await page.cookies()).some(cookie => cookie.name === "LABS"));
    const token = `preview-${Date.now()}`;
    await page.evaluate(token => { window.__previewHelpDocument = token; }, token);
    await page.evaluate(() => Array.from(document.querySelectorAll("a[href]"))
      .find(link => new URL(link.href).pathname === "/list").click());
    await page.waitForSelector(id("list-new"));
    assert.equal(await page.evaluate(() => window.__previewHelpDocument), token,
      "Settings -> Lists opens device lists in the same hydrated document");
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
    await visibleText(en.list_shop_handoff_intro);
    await page.waitForSelector(id("shop-cart-summary"), { visible: true });
    await capture(page, { path: path.join(artifacts, "device-shop-help.png"), fullPage: true });
    await page.click(id("guest-build-mode"));
    await page.waitForSelector(needed, { visible: true });
    await saved();
    await load("/settings?lang=en");
    await load(`${devicePath}?lang=en`);
    await page.waitForSelector(needed, { visible: true });
    assert.equal(await page.$eval(needed, input => input.value), "1", "device list remains available after returning from Settings");
    assert.deepEqual(accountWrites, [], "anonymous device editing makes no account-list writes");
    assert.deepEqual(errors, [], "no uncaught browser errors");
    console.log("PASS: no Labs settings, anonymous SPA/create/edit/Undo/Build/Shop/help/reload journey");
  } catch (error) {
    console.error("Device list state:", await page.evaluate(() => ({
      url: location.href,
      dialog: document.querySelector('[role="dialog"]')?.innerText,
      text: document.body.innerText.slice(0, 2500),
    })), errors);
    throw error;
  } finally {
    await browser.close();
  }
}

main().catch(error => { console.error(error); process.exitCode = 1; });
