#!/usr/bin/env node
"use strict";

// Local (device-only) list cards on /list get the same pencil as account
// lists: rename in place and delete from a Danger Zone, without opening the
// list. Run against a built server:
//   BASE_URL=http://127.0.0.1:8080 npm --prefix integration run test:device-list-card-edit
// No account, database row or market data is needed. Covers the pencil
// appearing on a local card, Cancel restoring view mode, a rename landing in
// the card and surviving a reload, and delete-with-confirmation removing the
// card and its storage record.
const assert = require("node:assert/strict");

const testId = id => `[data-testid="${id}"]`;

async function main() {
  const { default: puppeteer } = await import("puppeteer");
  const base = process.env.BASE_URL || "http://127.0.0.1:8080";
  const timeout = Number(process.env.TIMEOUT_MS || 60000);
  const browser = await puppeteer.launch({
    headless: process.env.HEADLESS !== "false", args: ["--no-sandbox"],
  });
  const page = await browser.defaultBrowserContext().newPage();
  page.setDefaultTimeout(timeout);
  await page.setViewport({ width: 1280, height: 900 });
  await page.setCookie(
    { name: "LABS", value: "lists-sync", url: base, path: "/" },
    { name: "HIDE_ADS", value: "true", url: base, path: "/" },
    { name: "i18n_pref_locale", value: "en", url: base, path: "/" },
  );
  await page.evaluateOnNewDocument(() => {
    window.__cardHydrated = false;
    window.addEventListener("ultros:hydrated", () => { window.__cardHydrated = true; });
  });
  const errors = [];
  page.on("pageerror", error => errors.push(error.message));

  async function load(route) {
    const response = await page.goto(new URL(route, base).href, { waitUntil: "domcontentloaded" });
    assert(response?.ok(), `navigation ${route}: ${response?.status()}`);
    await page.waitForFunction(() => window.__cardHydrated);
  }
  async function replace(selector, value) {
    await page.waitForSelector(selector, { visible: true });
    await page.click(selector);
    await page.$eval(selector, input => input.select());
    await page.keyboard.press("Backspace");
    await page.type(selector, String(value));
  }
  // The card whose heading link carries `name`, or null once it is gone.
  const cardWith = name => page.evaluateHandle(name => {
    const cards = [...document.querySelectorAll('[data-testid="list-card"]')];
    return cards.find(card => [...card.querySelectorAll("a")].some(a => a.textContent.trim() === name)) ?? null;
  }, name);
  async function waitCard(name) {
    await page.waitForFunction(name =>
      [...document.querySelectorAll('[data-testid="list-card"]')].some(card =>
        [...card.querySelectorAll("a")].some(a => a.textContent.trim() === name)),
    {}, name);
    return cardWith(name);
  }
  const waitNoCard = name => page.waitForFunction(name =>
    ![...document.querySelectorAll('[data-testid="list-card"]')].some(card =>
      [...card.querySelectorAll("a")].some(a => a.textContent.trim() === name)),
  {}, name);
  const within = (card, id) => card.$(testId(id));
  async function createList(name) {
    await page.click(testId("list-new"));
    await replace(testId("device-list-name"), name);
    await page.click(testId("device-list-create"));
    await page.waitForFunction(() => location.pathname.startsWith("/list/device/"));
    await page.waitForFunction(selector =>
      document.querySelector(selector)?.textContent.includes("Saved on this device"),
    {}, testId("device-list-status"));
  }

  try {
    await load("/list?lang=en");
    await page.waitForSelector(testId("list-new"));
    const stamp = Date.now();
    const original = `Card edit ${stamp}`;
    const renamed = `Card renamed ${stamp}`;
    await createList(original);
    console.log("[step] device list created");

    await load("/list?lang=en");
    let card = await waitCard(original);
    assert(await within(card, "list-card-make-online"), "card is a local card (Make online)");
    const pencil = await within(card, "list-card-edit");
    assert(pencil, "local card shows the edit pencil");
    console.log("[ok] local card has the edit pencil");

    await pencil.click();
    await page.waitForSelector(testId("list-card-name"), { visible: true });
    assert.equal(
      await page.$eval(testId("list-card-name"), input => input.value), original,
      "edit form starts from the current name",
    );
    await page.click(testId("list-card-cancel"));
    await page.waitForFunction(selector => !document.querySelector(selector), {}, testId("list-card-name"));
    card = await waitCard(original);
    assert(await within(card, "list-card-edit"), "Cancel returns to view mode with the pencil");
    console.log("[ok] Cancel restores view mode");

    await (await within(card, "list-card-edit")).click();
    await replace(testId("list-card-name"), renamed);
    await page.click(testId("list-card-save"));
    await waitCard(renamed);
    await waitNoCard(original);
    assert.equal(await page.$(testId("list-card-name")), null, "save leaves edit mode");
    console.log("[ok] rename from the card updates the card");

    await load("/list?lang=en");
    card = await waitCard(renamed);
    await waitNoCard(original);
    console.log("[ok] rename persisted across a reload");

    await (await within(card, "list-card-edit")).click();
    await page.waitForSelector(testId("list-card-delete"), { visible: true });
    await page.click(testId("list-card-delete"));
    await page.waitForSelector(testId("list-card-confirm-delete"), { visible: true });
    await page.click(testId("list-card-confirm-delete"));
    await waitNoCard(renamed);
    await page.waitForFunction(selector => !document.querySelector(selector), {}, testId("list-card-confirm-delete"));
    console.log("[ok] delete from the card removes it");

    await load("/list?lang=en");
    await page.waitForSelector(testId("list-new"));
    await waitNoCard(renamed);
    await waitNoCard(original);
    console.log("[ok] deleted list stays gone after a reload");

    assert.deepEqual(errors, [], "no page errors");
    console.log("[pass] device-list-card-edit");
  } finally {
    await browser.close();
  }
}

main().catch(error => {
  console.error(error);
  process.exit(1);
});
