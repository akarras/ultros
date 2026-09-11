#!/usr/bin/env node
"use strict";

// Keyboard undo/redo on device lists (issue #1429). Run against a built
// server: BASE_URL=http://127.0.0.1:8080 npm --prefix integration run test:list-undo-keyboard
// No account, database row or market data is needed; the catalog must be
// installed. Covers a direct load and a client-side navigation between two
// device lists, committed add/edit/delete undo and redo, persistence after
// undo, the delete-confirmation guard, and that switching lists leaves
// exactly one listener behind (one Ctrl+Z reverts exactly one step).
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const { capture } = require("./capture.cjs");

const testId = id => `[data-testid="${id}"]`;
const NEEDED = 'input[aria-label="Needed for Bronze Ingot"]';
const SEARCH = 'input[aria-label="Add an item"]';

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
    window.__undoHydrated = false;
    window.addEventListener("ultros:hydrated", () => { window.__undoHydrated = true; });
  });
  const errors = [];
  page.on("pageerror", error => errors.push(error.message));

  async function load(route) {
    const response = await page.goto(new URL(route, base).href, { waitUntil: "domcontentloaded" });
    assert(response?.ok(), `navigation ${route}: ${response?.status()}`);
    await page.waitForFunction(() => window.__undoHydrated);
  }
  async function replace(selector, value) {
    await page.waitForSelector(selector, { visible: true });
    // Triple-click never selects a number input, and a right-aligned cell
    // puts the caret before the digits; select explicitly instead.
    await page.click(selector);
    await page.$eval(selector, input => input.select());
    await page.keyboard.press("Backspace");
    await page.type(selector, String(value));
  }
  async function saved() {
    await page.waitForFunction(selector =>
      document.querySelector(selector)?.textContent.includes("Saved on this device"),
    {}, testId("device-list-status"));
  }
  const waitValue = (selector, expected) => page.waitForFunction(
    (selector, expected) => document.querySelector(selector)?.value === expected,
    {}, selector, String(expected));
  const waitGone = selector => page.waitForFunction(
    selector => !document.querySelector(selector), {}, selector);
  const neededValue = () => page.$eval(NEEDED, input => input.value).catch(() => null);
  // Blur whatever editor has focus so the shortcut targets the document,
  // which is the contract this PR ships; the editor-focus rules are #1430.
  const focusDocument = () => page.evaluate(() => { document.activeElement?.blur(); });
  async function chord(...keys) {
    const modifiers = keys.slice(0, -1);
    for (const key of modifiers) await page.keyboard.down(key);
    await page.keyboard.press(keys[keys.length - 1]);
    for (const key of modifiers.reverse()) await page.keyboard.up(key);
  }
  const undo = () => chord("Control", "KeyZ");
  const redoY = () => chord("Control", "KeyY");
  const redoShiftZ = () => chord("Control", "Shift", "KeyZ");
  async function createList(name) {
    await replace(testId("device-list-name"), name);
    await page.click(testId("device-list-create"));
    await page.waitForFunction(() => location.pathname.startsWith("/list/device/"));
    await saved();
    return page.url();
  }
  async function addBronzeIngot(quantity) {
    await replace('input[aria-label="Quantity to add"]', quantity);
    await replace(SEARCH, "Bronze Ingot");
    await page.waitForSelector('button[aria-label="Add Bronze Ingot"]');
    await page.keyboard.press("Enter");
    await page.keyboard.press("Escape");
    await focusDocument();
  }

  try {
    await load("/list?lang=en");
    await page.waitForSelector(testId("device-list-create"));
    const listA = await createList(`Undo direct ${Date.now()}`);
    await load(listA);
    console.log("[step] device list opened by direct load");

    await addBronzeIngot(3);
    await waitValue(NEEDED, 3);
    await saved();
    await undo();
    await waitGone(NEEDED);
    console.log("[ok] Ctrl+Z undoes a committed add");
    await redoY();
    await waitValue(NEEDED, 3);
    console.log("[ok] Ctrl+Y redoes it");
    await undo();
    await waitGone(NEEDED);
    await redoShiftZ();
    await waitValue(NEEDED, 3);
    console.log("[ok] Ctrl+Shift+Z redoes it");

    await replace(NEEDED, 8);
    await page.keyboard.press("Enter");
    await waitValue(NEEDED, 8);
    await saved();
    await undo();
    await waitValue(NEEDED, 3);
    await redoY();
    await waitValue(NEEDED, 8);
    console.log("[ok] a committed cell edit undoes and redoes");

    const removeButton = await page.$('[data-item-id] button[aria-label="Remove Bronze Ingot"]');
    assert(removeButton, "the row's Remove button exists");
    await removeButton.click();
    await waitGone(NEEDED);
    await focusDocument();
    await undo();
    await waitValue(NEEDED, 8);
    console.log("[ok] a committed delete undoes");
    await saved();
    await load(listA);
    await waitValue(NEEDED, 8);
    console.log("[ok] the undone state is what persisted");

    await page.click(testId("device-list-storage-toggle"));
    await page.click(testId("device-list-delete"));
    await page.waitForSelector(testId("device-list-confirm-delete"));
    await focusDocument();
    await undo();
    await new Promise(resolve => setTimeout(resolve, 500));
    assert.equal(await neededValue(), "8", "Ctrl+Z is ignored while the delete confirmation is open");
    await page.$eval(testId("device-list-confirm-delete"), button =>
      button.parentElement.querySelector("button.btn-secondary").click());
    await waitGone(testId("device-list-confirm-delete"));
    console.log("[ok] the delete confirmation guards the shortcut");

    console.log("[step] client-side navigation to a second device list");
    const token = `undo-nav-${Date.now()}`;
    await page.evaluate(token => { window.__undoDocument = token; }, token);
    await page.$eval('a[href="/list?labs=lists-sync"]', link => link.click());
    await page.waitForSelector(testId("device-list-create"));
    const listB = await createList(`Undo navigated ${Date.now()}`);
    assert.equal(await page.evaluate(() => window.__undoDocument), token,
      "the second list opened without a document reload");
    await addBronzeIngot(3);
    await waitValue(NEEDED, 3);
    await addBronzeIngot(3);
    await waitValue(NEEDED, 6);
    await saved();
    await undo();
    await waitValue(NEEDED, 3);
    await new Promise(resolve => setTimeout(resolve, 500));
    assert.equal(await neededValue(), "3",
      "one Ctrl+Z reverts exactly one step: a second listener would have reverted both adds");
    console.log("[ok] shortcuts reach the navigated-to list through a single listener");
    await saved();

    // ---- #1430: undo while an editor has focus ----
    // Contract in docs/superpowers/specs/2026-09-10-list-undo-keyboard-and-editing-design.md:
    // an editor with a draft keeps native text undo; a clean editor, a
    // select, a checkbox and focus outside the grid all reach document undo.
    console.log("[step] editor-focus rules on the navigated-to list");
    // The compact cart (#1434) renders rows as `li[data-item-id]` with
    // per-row accessible names and puts Quality right after Needed.
    const QUALITY = '[data-item-id] select[aria-label="Quality for Bronze Ingot"]';
    const CHECKBOX = '[data-item-id] input[type="checkbox"]';
    const FEEDBACK = `${testId("inline-list-add")} [role="status"]`;
    const committed = () => page.$eval(NEEDED, input => input.getAttribute("data-committed"));
    const focusedLabel = () => page.evaluate(() => document.activeElement?.getAttribute("aria-label") ?? null);
    const settle = () => new Promise(resolve => setTimeout(resolve, 400));

    await page.click(SEARCH);
    await page.keyboard.press("Escape");
    await undo();
    await waitGone(NEEDED);
    console.log("[ok] Ctrl+Z in the empty catalog search undoes the document");
    await redoY();
    await waitValue(NEEDED, 3);
    await page.type(SEARCH, "Bronze");
    await undo();
    await settle();
    assert.equal(await neededValue(), "3", "Ctrl+Z inside a search draft leaves the document alone");
    assert.equal(await focusedLabel(), "Add an item", "focus stays in the search draft");
    await page.keyboard.press("Escape");
    console.log("[ok] a catalog search draft keeps native text undo");

    await replace(NEEDED, 5);
    await page.keyboard.press("Tab");
    await waitValue(NEEDED, 5);
    assert.equal(await focusedLabel(), "Quality for Bronze Ingot", "Tab moved focus into the next control");
    await undo();
    await waitValue(NEEDED, 3);
    console.log("[ok] Ctrl+Z in the next clean cell undoes the Tab-committed edit");
    await redoY();
    await waitValue(NEEDED, 5);

    await replace(NEEDED, 44);
    await undo();
    await settle();
    assert.equal(await committed(), "5", "Ctrl+Z inside a numeric draft leaves the document alone");
    assert.equal(await focusedLabel(), "Needed for Bronze Ingot", "focus stays in the drafting cell");
    await page.keyboard.press("Escape");
    await waitValue(NEEDED, 5);
    await undo();
    await waitValue(NEEDED, 3);
    console.log("[ok] a numeric draft keeps native text undo; Escape discards it and Ctrl+Z then reaches the document");
    await redoY();
    await waitValue(NEEDED, 5);

    await page.select(QUALITY, "hq");
    await page.waitForFunction(selector => document.querySelector(selector)?.value === "hq", {}, QUALITY);
    await page.focus(QUALITY);
    await undo();
    await page.waitForFunction(selector => document.querySelector(selector)?.value === "any", {}, QUALITY);
    console.log("[ok] Ctrl+Z in the focused quality select undoes the quality change");
    await page.click(CHECKBOX);
    assert.equal(await page.$eval(CHECKBOX, box => box.checked), true);
    await redoY();
    await page.waitForFunction(selector => document.querySelector(selector)?.value === "hq", {}, QUALITY);
    await page.click(CHECKBOX);
    console.log("[ok] Ctrl+Y with a focused checkbox redoes the document");
    await page.focus(QUALITY);
    await undo();
    await page.waitForFunction(selector => document.querySelector(selector)?.value === "any", {}, QUALITY);

    await replace(NEEDED, 7);
    await page.keyboard.press("Enter");
    await waitValue(NEEDED, 7);
    await replace(NEEDED, 9);
    await page.keyboard.press("Enter");
    await waitValue(NEEDED, 9);
    await focusDocument();
    await undo();
    await waitValue(NEEDED, 7);
    await undo();
    await waitValue(NEEDED, 5);
    console.log("[ok] two immediate commits undo one at a time");
    await redoY();
    await waitValue(NEEDED, 7);
    await redoY();
    await waitValue(NEEDED, 9);
    await page.waitForFunction(selector => document.querySelector(selector)?.disabled === true, {}, testId("list-redo"));
    assert.equal(await page.$eval(testId("list-undo"), button => button.disabled), false, "Undo stays available");
    await redoY();
    await page.waitForFunction(selector => document.querySelector(selector)?.textContent.includes("Nothing to redo"), {}, FEEDBACK);
    console.log("[ok] an exhausted redo stack disables the button and the shortcut explains itself");
    await saved();

    await page.evaluate(token => { window.__undoDocument = token; }, token);
    await page.$eval('a[href="/list?labs=lists-sync"]', link => link.click());
    await page.waitForSelector(testId("device-list-create"));
    await page.$eval(`a[href^="${new URL(listA).pathname}"]`, link => link.click());
    await waitValue(NEEDED, 8);
    assert.equal(await page.evaluate(() => window.__undoDocument), token,
      "the first list reopened without a document reload");
    await undo();
    await new Promise(resolve => setTimeout(resolve, 500));
    assert.equal(await neededValue(), "8",
      "a freshly reopened document has nothing to undo; the previous list's history does not leak");
    await load(listB);
    await waitValue(NEEDED, 9);
    console.log("[ok] leaving a list disposes its listener and its history");

    assert.deepEqual(errors, [], "no uncaught browser errors");
  } catch (error) {
    const artifacts = path.join(__dirname, "artifacts", "list-undo-keyboard");
    fs.mkdirSync(artifacts, { recursive: true });
    await capture(page, { path: path.join(artifacts, "failure.png"), fullPage: true }).catch(() => {});
    console.error("Page:", page.url(), await page.$eval("body", body => body.innerText).catch(() => "unavailable"));
    console.error("Errors:", errors);
    throw error;
  } finally {
    await browser.close();
  }
}

main().catch(error => { console.error(error); process.exitCode = 1; });
