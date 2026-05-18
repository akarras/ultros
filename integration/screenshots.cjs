#!/usr/bin/env node
/* eslint-disable no-console */
/**
 * Capture screenshots of the refreshed list-view UI in interesting states.
 * Run against a server built with --features test-auth.
 *
 *   1. Empty owner list                       — empty-state
 *   2. List with mixed items + mark acquired  — populated + completed row
 *   3. Read-only viewer of same list          — write affordances hidden
 *   4. Owner settings drawer open             — full drawer
 *
 * Writes to integration/artifacts/.
 */

"use strict";

const fs = require("fs");
const path = require("path");
const puppeteer = require("puppeteer");

const BASE_URL = process.env.BASE_URL || "http://127.0.0.1:60703";
const OUT = path.resolve(__dirname, "artifacts");
fs.mkdirSync(OUT, { recursive: true });

const OWNER = { id: 990000000010, username: "ScreenshotOwner" };
const READER = { id: 990000000011, username: "ScreenshotReader" };

async function login(page, user) {
  const url = new URL("/test/login", BASE_URL);
  url.searchParams.set("user_id", String(user.id));
  url.searchParams.set("username", user.username);
  url.searchParams.set("redirect", "/");
  await page.goto(url.toString(), { waitUntil: "domcontentloaded" });
}

async function api(page, method, path_, body) {
  return page.evaluate(
    async ({ method, path_, body }) => {
      const r = await fetch(path_, {
        method,
        credentials: "include",
        headers: body === undefined ? {} : { "Content-Type": "application/json" },
        body: body === undefined ? undefined : JSON.stringify(body),
      });
      const text = await r.text();
      try {
        return { status: r.status, body: text ? JSON.parse(text) : null };
      } catch {
        return { status: r.status, body: text };
      }
    },
    { method, path_, body },
  );
}

async function snap(page, name) {
  const file = path.join(OUT, `list-refresh-${name}.png`);
  await page.screenshot({ path: file, fullPage: true });
  console.log("[ok]", file);
}

async function waitForHydration(page) {
  await page.waitForFunction(
    () => !!document.querySelector('[data-testid="list-settings-btn"]'),
    { timeout: 30000 },
  );
  // Wait for the view_caps Effect to fire (owner-gated Add Item button).
  await page.waitForFunction(
    () =>
      Array.from(document.querySelectorAll(".list-toolbar button")).some((b) =>
        (b.innerText || "").includes("Add Item"),
      ),
    { timeout: 15000 },
  );
  // Brief settle for any async layout.
  await new Promise((r) => setTimeout(r, 500));
}

(async () => {
  const browser = await puppeteer.launch({
    headless: "new",
    args: ["--no-sandbox", "--disable-setuid-sandbox"],
  });
  try {
    const ownerCtx = await browser.createBrowserContext();
    const readerCtx = await browser.createBrowserContext();
    const owner = await ownerCtx.newPage();
    const reader = await readerCtx.newPage();
    for (const p of [owner, reader]) {
      await p.setViewport({ width: 1280, height: 900, deviceScaleFactor: 1 });
      p.setDefaultTimeout(60000);
    }

    await login(owner, OWNER);
    await login(reader, READER);

    // --- Create the list, leave it empty for first shot.
    const worldData = await api(owner, "GET", "/api/v1/world_data");
    const worldId = worldData.body.regions[0].datacenters[0].worlds[0].id;
    const name = `Screenshot Sample ${Date.now()}`;
    await api(owner, "POST", "/api/v1/list/create", {
      name,
      wdr_filter: { World: worldId },
    });
    const ownerLists = await api(owner, "GET", "/api/v1/list");
    const listId = ownerLists.body.find((e) => e.list.name === name).list.id;
    console.log("[step] created list", listId);

    // --- Shot 1: empty list, owner view.
    await owner.goto(`${BASE_URL}/list/${listId}`, { waitUntil: "domcontentloaded" });
    await waitForHydration(owner);
    await snap(owner, "01-empty-owner");

    // --- Populate via API: a few items so we can show the table + progress + completed row.
    // Item IDs: 5 Earth Shard, 4 Wind Shard, 19 Maple Log, 46010 Ceremonial Shamshir (HQ marketable)
    const items = [
      { item_id: 5, quantity: 99, hq: null },
      { item_id: 4, quantity: 50, hq: null, acquired: 50 }, // fully acquired
      { item_id: 19, quantity: 12, hq: null, acquired: 4 },
      { item_id: 46010, quantity: 1, hq: true },
    ];
    for (const it of items) {
      await api(owner, "POST", `/api/v1/list/${listId}/add/item`, {
        id: 0,
        list_id: listId,
        hq: it.hq,
        quantity: it.quantity,
        acquired: it.acquired ?? null,
        item_id: it.item_id,
      });
    }

    // --- Shot 2: populated list, owner view.
    await owner.reload({ waitUntil: "domcontentloaded" });
    await waitForHydration(owner);
    // Wait for at least one row + progress text.
    await owner
      .waitForFunction(
        () => /units acquired/.test(document.body.innerText) &&
          document.querySelectorAll("tbody tr").length >= 4,
        { timeout: 30000 },
      )
      .catch(() => {});
    await new Promise((r) => setTimeout(r, 800));
    await snap(owner, "02-populated-owner");

    // --- Shot 3: owner settings drawer open.
    await owner.evaluate(() => {
      const b = document.querySelector('[data-testid="list-settings-btn"]');
      b && b.click();
    });
    await owner.waitForSelector('[data-testid="list-settings-drawer"]', {
      visible: true,
      timeout: 10000,
    });
    // Wait for sharing section to render its Suspense.
    await owner
      .waitForFunction(
        () => {
          const sec = document.querySelector('[data-testid="list-settings-sharing"]');
          return (
            sec &&
            Array.from(sec.querySelectorAll("button")).some((b) =>
              /copy/i.test((b.innerText || "").trim()),
            )
          );
        },
        { timeout: 10000 },
      )
      .catch(() => {});
    await new Promise((r) => setTimeout(r, 500));
    await snap(owner, "03-settings-drawer");

    // Close drawer.
    await owner.keyboard.press("Escape");
    await owner
      .waitForFunction(
        () => !document.querySelector('[data-testid="list-settings-drawer"]'),
        { timeout: 5000 },
      )
      .catch(() => {});

    // --- Shot 4: read-only viewer.
    // Share Read with reader.
    await api(owner, "POST", `/api/v1/list/${listId}/share/user`, {
      user_id: READER.id,
      permission: "Read",
    });
    await reader.goto(`${BASE_URL}/list/${listId}`, { waitUntil: "domcontentloaded" });
    await reader.waitForFunction(() => !!document.querySelector("h1"), { timeout: 30000 });
    await new Promise((r) => setTimeout(r, 2000));
    await snap(reader, "04-read-only-reader");

    // --- Shot 5 (bonus): the top-level /list page with the refreshed share modal.
    await owner.goto(`${BASE_URL}/list`, { waitUntil: "domcontentloaded" });
    await owner.waitForFunction(
      () => !!document.querySelector("h1"),
      { timeout: 30000 },
    );
    await new Promise((r) => setTimeout(r, 1500));
    await snap(owner, "05-lists-page-owner");

    // --- Cleanup.
    await api(owner, "DELETE", `/api/v1/list/${listId}/delete`);
  } finally {
    await browser.close();
  }
})().catch((e) => {
  console.error(e);
  process.exit(1);
});
