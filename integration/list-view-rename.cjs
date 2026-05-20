#!/usr/bin/env node
/* eslint-disable no-console */
/**
 * Verifies the inline "rename" pencil on /list/{id}:
 *   1. Click the pencil (data-testid="list-rename-btn")
 *   2. The inline rename input mounts (data-testid="list-rename-input")
 *   3. The input is still present after a settle delay
 *      (regression for "toggles back to viewing mode immediately")
 *
 * Requires server built with `--features test-auth`.
 *
 * Env: BASE_URL, HEADLESS, TIMEOUT_MS, POST_CLICK_SETTLE_MS (default 1500)
 */

"use strict";

const USER = { id: 990000000020, username: "ListViewRenameUser" };

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

async function main() {
  const puppeteer = require("puppeteer");
  const BASE_URL = process.env.BASE_URL || "http://127.0.0.1:8080";
  const TIMEOUT_MS = Number(process.env.TIMEOUT_MS || 30000);
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

    console.log("[step] login + create list");
    await login(page, BASE_URL, USER);

    const worldData = await api(page, "GET", "/api/v1/world_data");
    if (worldData.status !== 200) throw new Error(`world_data ${worldData.status}`);
    const worldId = worldData.body.regions[0].datacenters[0].worlds[0].id;
    const listName = `RenamePencil E2E ${Date.now()}`;
    await api(page, "POST", "/api/v1/list/create", {
      name: listName,
      wdr_filter: { World: worldId },
    });
    const all = await api(page, "GET", "/api/v1/list");
    const ourList = (all.body || []).find((e) => e.list.name === listName);
    if (!ourList) throw new Error("created list not in GET /api/v1/list");
    const listId = ourList.list.id;
    pass(`created list "${listName}" (id=${listId})`);

    console.log("[step] open /list/" + listId);
    await page.goto(new URL(`/list/${listId}`, BASE_URL).toString(), {
      waitUntil: "domcontentloaded",
    });

    // The owner-only rename button appears only after hydration + view_caps Effect.
    console.log("[step] wait for rename pencil to mount");
    await page.waitForFunction(
      () => !!document.querySelector('[data-testid="list-rename-btn"]'),
      { timeout: TIMEOUT_MS },
    );
    await new Promise((r) => setTimeout(r, 1500));
    pass("rename pencil visible");

    // Sanity: input not yet present.
    const before = await page.evaluate(() => ({
      input: !!document.querySelector('[data-testid="list-rename-input"]'),
      pencil: !!document.querySelector('[data-testid="list-rename-btn"]'),
    }));
    if (!before.pencil || before.input) {
      fail(failures, `before click: bad initial state ${JSON.stringify(before)}`);
    } else {
      pass("before click: pencil visible, input absent");
    }

    // ----- Click the pencil -----
    console.log("[step] click rename pencil");
    await page.evaluate(() => {
      window.__renameMutations = [];
      const obs = new MutationObserver((records) => {
        for (const r of records) {
          window.__renameMutations.push({
            t: Date.now(),
            type: r.type,
            target: (r.target && r.target.tagName) || "?",
            attr: r.attributeName,
            inputPresent: !!document.querySelector('[data-testid="list-rename-input"]'),
            pencilPresent: !!document.querySelector('[data-testid="list-rename-btn"]'),
          });
        }
      });
      obs.observe(document.body, { subtree: true, childList: true, attributes: true });
      window.__renameObserver = obs;
    });
    await page.click('[data-testid="list-rename-btn"]');

    // ----- Poll: did the input appear, even briefly? -----
    let firstSawInputAt = null;
    let lastSawInputAt = null;
    const pollStart = Date.now();
    while (Date.now() - pollStart < SETTLE_MS + 500) {
      const seen = await page.evaluate(() => ({
        input: !!document.querySelector('[data-testid="list-rename-input"]'),
        pencil: !!document.querySelector('[data-testid="list-rename-btn"]'),
        now: Date.now(),
      }));
      if (seen.input) {
        if (firstSawInputAt === null) firstSawInputAt = seen.now;
        lastSawInputAt = seen.now;
      }
      await new Promise((r) => setTimeout(r, 50));
    }
    console.log(`  [debug] first saw input: +${firstSawInputAt === null ? "never" : firstSawInputAt - pollStart}ms, last: +${lastSawInputAt === null ? "never" : lastSawInputAt - pollStart}ms`);

    // ----- Final state check -----
    const after = await page.evaluate(() => ({
      input: !!document.querySelector('[data-testid="list-rename-input"]'),
      pencil: !!document.querySelector('[data-testid="list-rename-btn"]'),
      inputValue: document.querySelector('[data-testid="list-rename-input"]')?.value || null,
    }));
    console.log(`  [debug] final state: ${JSON.stringify(after)}`);

    if (firstSawInputAt === null) {
      fail(failures, "rename input never appeared after click");
    } else {
      pass(`rename input appeared (+${firstSawInputAt - pollStart}ms)`);
    }
    if (!after.input) {
      fail(failures, "after settle: rename input is GONE (toggle-back regression)");
      // Dump mutation log to understand what happened
      const muts = await page.evaluate(() => window.__renameMutations || []);
      const filtered = muts.filter((m) => m.type === "childList" || m.attr === "data-testid");
      console.error(`  ?  relevant mutations: ${filtered.length}`);
      for (const m of filtered.slice(0, 30)) {
        console.error(`      +${m.t - pollStart}ms: ${m.type} <${m.target}> attr=${m.attr} input=${m.inputPresent} pencil=${m.pencilPresent}`);
      }
    } else {
      pass(`after settle (~${SETTLE_MS}ms): rename input still mounted`);
    }
    if (after.pencil) {
      fail(failures, "after settle: pencil still visible (rename mode not engaged)");
    } else {
      pass("after settle: pencil hidden");
    }

    // Cleanup
    await api(page, "DELETE", `/api/v1/list/${listId}/delete`).catch(() => {});
    await page.close();
  } finally {
    await browser.close();
  }

  if (failures.length) {
    console.error(`[fail] ${failures.length} list-view rename assertion(s) failed`);
    process.exit(1);
  }
  console.log("[ok] list view rename pencil persists");
}

main().catch((err) => {
  console.error("[error]", err && err.stack ? err.stack : err);
  process.exit(1);
});
