#!/usr/bin/env node
/* eslint-disable no-console */
/**
 * Verifies the "Bulk Edit" toggle button at the top of the list items table
 * on /list/{id}. This is the most plausible target for "edit list button
 * immediately toggles back to viewing mode" because:
 *   - The handler is `edit_list_mode.update(|u| *u = !*u)` (literally a toggle)
 *   - The outer reactive closure on this page re-runs every second via
 *     clock_tick, so a misbehavior under re-render is plausible
 *
 *   1. Click Bulk Edit — checkbox column + Select-All controls appear
 *   2. After a settle delay, those controls are still visible
 *   3. Click again — they disappear
 *
 * Requires server built with `--features test-auth`.
 */

"use strict";

const USER = { id: 990000000030, username: "BulkEditUser" };

async function login(page, baseUrl, user) {
  const url = new URL("/test/login", baseUrl);
  url.searchParams.set("user_id", String(user.id));
  url.searchParams.set("username", user.username);
  url.searchParams.set("redirect", "/list");
  const resp = await page.goto(url.toString(), { waitUntil: "domcontentloaded" });
  if (!resp || resp.status() >= 400) {
    throw new Error(`test login failed: ${resp ? resp.status() : -1}`);
  }
}

async function api(page, method, path, body) {
  return page.evaluate(
    async ({ method, path, body }) => {
      const r = await fetch(path, {
        method,
        credentials: "include",
        headers: body === undefined ? {} : { "Content-Type": "application/json" },
        body: body === undefined ? undefined : JSON.stringify(body),
      });
      const text = await r.text();
      let parsed = null;
      try { parsed = text ? JSON.parse(text) : null; } catch { parsed = text; }
      return { status: r.status, body: parsed };
    },
    { method, path, body },
  );
}

function fail(failures, msg) { console.error(`  X ${msg}`); failures.push(msg); }
function pass(msg) { console.log(`  + ${msg}`); }

// Edit mode shows a "Select all" button. The button stays in the DOM but its
// container has class="hidden" applied via `class:hidden=move || !edit_list_mode()`.
// `offsetParent === null` is the classic test for "hidden by CSS".
async function inEditMode(page) {
  return page.evaluate(() => {
    const btn = Array.from(document.querySelectorAll("button")).find(
      (b) => /select\s*all/i.test((b.innerText || "").trim()),
    );
    if (!btn) return false;
    return btn.offsetParent !== null;
  });
}

async function clickBulkEdit(page) {
  return page.evaluate(() => {
    const btn = Array.from(document.querySelectorAll("button")).find(
      (b) => /bulk\s*edit/i.test((b.innerText || "").trim()),
    );
    if (!btn) return false;
    btn.click();
    return true;
  });
}

async function main() {
  const puppeteer = require("puppeteer");
  const BASE_URL = process.env.BASE_URL || "http://127.0.0.1:8080";
  const TIMEOUT_MS = Number(process.env.TIMEOUT_MS || 30000);
  const SETTLE_MS = Number(process.env.POST_CLICK_SETTLE_MS || 2000);
  const headless = process.env.HEADLESS === "false" ? false : "new";

  const browser = await puppeteer.launch({
    headless,
    args: ["--no-sandbox", "--disable-setuid-sandbox"],
  });

  const failures = [];

  try {
    const context = await browser.createBrowserContext();
    const page = await context.newPage();
    page.setDefaultTimeout(TIMEOUT_MS);
    await page.setViewport({ width: 1280, height: 900, deviceScaleFactor: 1 });

    page.on("console", (msg) => {
      if (["error", "warning"].includes(msg.type())) {
        console.log(`  [page-${msg.type()}] ${msg.text()}`);
      }
    });
    page.on("pageerror", (err) => console.log(`  [page-error] ${err.message}`));

    // ----- Setup -----
    console.log("[step] login + create list with an item");
    await login(page, BASE_URL, USER);

    const worldData = await api(page, "GET", "/api/v1/world_data");
    if (worldData.status !== 200) throw new Error(`world_data ${worldData.status}`);
    const worldId = worldData.body.regions[0].datacenters[0].worlds[0].id;
    const listName = `BulkEdit E2E ${Date.now()}`;
    await api(page, "POST", "/api/v1/list/create", {
      name: listName,
      wdr_filter: { World: worldId },
    });
    const all = await api(page, "GET", "/api/v1/list");
    const ourList = (all.body || []).find((e) => e.list.name === listName);
    if (!ourList) throw new Error("created list missing");
    const listId = ourList.list.id;
    // Add a couple of items so the table renders with rows.
    for (const itemId of [5, 6, 7]) {
      await api(page, "POST", `/api/v1/list/${listId}/add/item`, {
        item_id: itemId,
        list_id: listId,
        quantity: 1,
      }).catch(() => {});
    }
    pass(`created list "${listName}" (id=${listId})`);

    // ----- Navigate -----
    console.log("[step] open /list/" + listId);
    await page.goto(new URL(`/list/${listId}`, BASE_URL).toString(), {
      waitUntil: "domcontentloaded",
    });

    // Wait for owner-only Bulk Edit button (gated on view_caps.can_write).
    console.log("[step] wait for Bulk Edit button");
    await page.waitForFunction(
      () =>
        Array.from(document.querySelectorAll("button")).some(
          (b) => /bulk\s*edit/i.test((b.innerText || "").trim()),
        ),
      { timeout: TIMEOUT_MS },
    );
    // Hydration grace period — dev WASM needs ~2-4s to wire up handlers.
    await new Promise((r) => setTimeout(r, 3000));
    pass("Bulk Edit button visible");

    // Initial state: NOT in edit mode.
    if (await inEditMode(page)) {
      fail(failures, "before click: already in edit mode");
    } else {
      pass("before click: not in edit mode (Select All hidden)");
    }

    // ----- 1st click: enter edit mode -----
    console.log("[step] click Bulk Edit (1st)");
    if (!(await clickBulkEdit(page))) {
      fail(failures, "Bulk Edit click 1 failed");
      throw new Error("cannot continue");
    }
    // Poll briefly for edit-mode indicators
    let entered = false;
    const t0 = Date.now();
    while (Date.now() - t0 < 2000) {
      if (await inEditMode(page)) { entered = true; break; }
      await new Promise((r) => setTimeout(r, 50));
    }
    if (!entered) {
      fail(failures, "after click 1: edit mode never engaged");
    } else {
      pass("after click 1: edit mode engaged (Select All visible)");
    }

    // ----- Regression check: wait SETTLE_MS, still in edit mode? -----
    console.log(`[step] wait ${SETTLE_MS}ms then re-check edit-mode persistence`);
    await new Promise((r) => setTimeout(r, SETTLE_MS));
    if (!(await inEditMode(page))) {
      fail(failures, `after ${SETTLE_MS}ms: edit mode toggled back to viewing mode`);
    } else {
      pass(`after ${SETTLE_MS}ms: edit mode persists`);
    }

    // ----- 2nd click: leave edit mode -----
    console.log("[step] click Bulk Edit (2nd) — should leave edit mode");
    if (!(await clickBulkEdit(page))) {
      fail(failures, "Bulk Edit click 2 failed");
    } else {
      let left = false;
      const t1 = Date.now();
      while (Date.now() - t1 < 2000) {
        if (!(await inEditMode(page))) { left = true; break; }
        await new Promise((r) => setTimeout(r, 50));
      }
      if (!left) {
        fail(failures, "after click 2: still in edit mode");
      } else {
        pass("after click 2: returned to viewing mode");
      }
    }

    // Cleanup
    await api(page, "DELETE", `/api/v1/list/${listId}/delete`).catch(() => {});
    await page.close();
  } finally {
    await browser.close();
  }

  if (failures.length) {
    console.error(`[fail] ${failures.length} bulk-edit assertion(s) failed`);
    process.exit(1);
  }
  console.log("[ok] bulk edit toggle persists");
}

main().catch((err) => {
  console.error("[error]", err && err.stack ? err.stack : err);
  process.exit(1);
});
