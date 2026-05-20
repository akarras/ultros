#!/usr/bin/env node
/* eslint-disable no-console */
/**
 * Verifies the ListCard edit pencil on /list (the "list menu"):
 *   1. Click the pencil — the inline edit form (input + Save/Cancel) appears
 *   2. After a settle delay the form is still mounted (regression check for
 *      "edit list button immediately toggles back to viewing mode")
 *   3. Cancel restores view mode
 *
 * Requires server built with `--features test-auth`.
 *
 * Env:
 *   BASE_URL              default http://127.0.0.1:8080
 *   HEADLESS              "false" to watch; otherwise puppeteer's "new" mode
 *   TIMEOUT_MS            default 30000
 *   HYDRATION_WAIT_MS     default 4000 — dev WASM takes a few seconds to wire
 *                         up listeners; without this delay clicks no-op
 *   POST_CLICK_SETTLE_MS  default 1500 — how long to wait before re-checking
 *                         that the form didn't snap back to view mode
 */

"use strict";

const USER = { id: 990000000010, username: "ListCardEditUser" };

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

// Locate the card by either:
//   - innerText contains the list name (view mode — name is in <a>), OR
//   - any descendant <input> has value === name (edit mode — name moves to
//     input.value, which is NOT part of innerText).
async function inspectCard(page, listName) {
  return page.evaluate((name) => {
    const cards = Array.from(document.querySelectorAll(".panel.rounded-xl"));
    const card = cards.find((c) => {
      if ((c.innerText || "").includes(name)) return true;
      return Array.from(c.querySelectorAll("input")).some(
        (i) => (i.value || "") === name,
      );
    });
    if (!card) return { found: false };

    const inputs = Array.from(card.querySelectorAll("input"));
    const matchingInput = inputs.find((i) => (i.value || "") === name);
    const buttons = Array.from(card.querySelectorAll("button"));
    const hasSave = buttons.some((b) => /save/i.test((b.innerText || "").trim()));
    const hasCancel = buttons.some((b) => /cancel/i.test((b.innerText || "").trim()));
    const pencilBtn = buttons.find((b) =>
      /edit\s*list/i.test(b.getAttribute("aria-label") || ""),
    );
    const nameLink = Array.from(card.querySelectorAll("a")).find(
      (a) => (a.innerText || "").trim() === name,
    );
    return {
      found: true,
      editFormVisible: !!matchingInput && hasSave && hasCancel,
      viewModeVisible: !!nameLink && !!pencilBtn,
    };
  }, listName);
}

async function main() {
  const puppeteer = require("puppeteer");
  const BASE_URL = process.env.BASE_URL || "http://127.0.0.1:8080";
  const TIMEOUT_MS = Number(process.env.TIMEOUT_MS || 30000);
  const HYDRATION_WAIT_MS = Number(process.env.HYDRATION_WAIT_MS || 4000);
  const SETTLE_MS = Number(process.env.POST_CLICK_SETTLE_MS || 1500);
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

    // ----- Login + create a list via API -----
    console.log("[step] login + create list");
    await login(page, BASE_URL, USER);

    const worldData = await api(page, "GET", "/api/v1/world_data");
    if (worldData.status !== 200) throw new Error(`world_data ${worldData.status}`);
    const worldId = worldData.body.regions[0].datacenters[0].worlds[0].id;
    const listName = `EditPencil E2E ${Date.now()}`;
    const created = await api(page, "POST", "/api/v1/list/create", {
      name: listName,
      wdr_filter: { World: worldId },
    });
    if (created.status !== 200) throw new Error(`create list ${created.status}`);
    const all = await api(page, "GET", "/api/v1/list");
    const ourList = (all.body || []).find((e) => e.list.name === listName);
    if (!ourList) throw new Error("created list not in GET /api/v1/list");
    const listId = ourList.list.id;
    pass(`created list "${listName}" (id=${listId})`);

    // ----- Visit /list and wait for the card to hydrate -----
    console.log("[step] open /list");
    await page.goto(new URL("/list", BASE_URL).toString(), {
      waitUntil: "domcontentloaded",
    });
    await page.waitForFunction(
      (name) => {
        const cards = Array.from(document.querySelectorAll(".panel.rounded-xl"));
        return cards.some((c) => (c.innerText || "").includes(name));
      },
      { timeout: TIMEOUT_MS },
      listName,
    );
    await page.waitForFunction(
      (name) => {
        const cards = Array.from(document.querySelectorAll(".panel.rounded-xl"));
        const card = cards.find((c) => (c.innerText || "").includes(name));
        if (!card) return false;
        return Array.from(card.querySelectorAll("button")).some(
          (b) => /edit\s*list/i.test(b.getAttribute("aria-label") || ""),
        );
      },
      { timeout: TIMEOUT_MS },
      listName,
    );
    pass("list card visible with pencil aria-label='Edit List'");

    // Dev WASM needs a few seconds after DOM hydration before click handlers
    // are wired up. Clicking too early no-ops the button silently.
    console.log(`[step] wait ${HYDRATION_WAIT_MS}ms for hydration to attach listeners`);
    await new Promise((r) => setTimeout(r, HYDRATION_WAIT_MS));
    pass("hydration grace period elapsed");

    const before = await inspectCard(page, listName);
    if (!before.found || !before.viewModeVisible || before.editFormVisible) {
      fail(failures, `before click: bad initial state ${JSON.stringify(before)}`);
      throw new Error("cannot continue");
    }
    pass("before click: in viewing mode (link + pencil)");

    // ----- Click the pencil -----
    console.log("[step] click pencil");
    const clicked = await page.evaluate((name) => {
      const cards = Array.from(document.querySelectorAll(".panel.rounded-xl"));
      const card = cards.find((c) => (c.innerText || "").includes(name));
      if (!card) return false;
      const pencil = Array.from(card.querySelectorAll("button")).find((b) =>
        /edit\s*list/i.test(b.getAttribute("aria-label") || ""),
      );
      if (!pencil) return false;
      pencil.click();
      return true;
    }, listName);
    if (!clicked) {
      fail(failures, "pencil click failed");
      throw new Error("cannot continue");
    }

    // Poll up to 3s for the input to appear — if the click is wired up at all
    // it should mount within a frame, but the timeout gives the WASM a window.
    let everSawInput = false;
    const pollStart = Date.now();
    while (Date.now() - pollStart < 3000) {
      const sawInput = await page.evaluate((name) => {
        const cards = Array.from(document.querySelectorAll(".panel.rounded-xl"));
        return cards.some((c) =>
          Array.from(c.querySelectorAll("input")).some(
            (i) => (i.value || "") === name,
          ),
        );
      }, listName);
      if (sawInput) { everSawInput = true; break; }
      await new Promise((r) => setTimeout(r, 50));
    }
    if (!everSawInput) {
      fail(failures, "rename input never appeared after click");
    }

    const immediately = await inspectCard(page, listName);
    if (!immediately.editFormVisible) {
      fail(failures, `after click: expected edit form, got ${JSON.stringify(immediately)}`);
    } else {
      pass("after click: edit form mounted (input + Save + Cancel)");
    }

    // ----- Regression check: wait, then re-verify the edit form persists -----
    console.log(`[step] wait ${SETTLE_MS}ms then re-check edit form persistence`);
    await new Promise((r) => setTimeout(r, SETTLE_MS));

    const settled = await inspectCard(page, listName);
    if (!settled.editFormVisible) {
      fail(failures, `after ${SETTLE_MS}ms: edit form vanished (toggle-back regression)`);
    } else {
      pass(`after ${SETTLE_MS}ms: edit form still mounted`);
    }
    if (settled.viewModeVisible) {
      fail(failures, `after ${SETTLE_MS}ms: viewing mode visible (toggle-back regression)`);
    } else {
      pass(`after ${SETTLE_MS}ms: viewing mode hidden`);
    }

    // ----- Cancel restores view mode -----
    console.log("[step] cancel returns to view mode");
    const cancelled = await page.evaluate((name) => {
      const cards = Array.from(document.querySelectorAll(".panel.rounded-xl"));
      const card = cards.find((c) => {
        if ((c.innerText || "").includes(name)) return true;
        return Array.from(c.querySelectorAll("input")).some(
          (i) => (i.value || "") === name,
        );
      });
      if (!card) return false;
      const cancelBtn = Array.from(card.querySelectorAll("button")).find(
        (b) => /cancel/i.test((b.innerText || "").trim()),
      );
      if (!cancelBtn) return false;
      cancelBtn.click();
      return true;
    }, listName);
    if (!cancelled) {
      fail(failures, "cancel click failed");
    } else {
      await page.waitForFunction(
        (name) => {
          const cards = Array.from(document.querySelectorAll(".panel.rounded-xl"));
          const card = cards.find((c) => (c.innerText || "").includes(name));
          if (!card) return false;
          return !!Array.from(card.querySelectorAll("a")).find(
            (a) => (a.innerText || "").trim() === name,
          );
        },
        { timeout: 5000 },
        listName,
      ).catch(() => {});
      const afterCancel = await inspectCard(page, listName);
      if (!afterCancel.viewModeVisible) {
        fail(failures, "after cancel: expected view mode to return");
      } else {
        pass("after cancel: view mode restored");
      }
    }

    // ----- Cleanup -----
    await api(page, "DELETE", `/api/v1/list/${listId}/delete`).catch(() => {});
    await page.close();
  } finally {
    await browser.close();
  }

  if (failures.length) {
    console.error(`[fail] ${failures.length} list-card edit assertion(s) failed`);
    process.exit(1);
  }
  console.log("[ok] list card edit pencil persists");
}

main().catch((err) => {
  console.error("[error]", err && err.stack ? err.stack : err);
  process.exit(1);
});
