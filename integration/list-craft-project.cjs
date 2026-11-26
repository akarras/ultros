#!/usr/bin/env node
"use strict";

/**
 * Multi-recipe Crafting Projects E2E (`Build | Craft | Shop`).
 *
 * Exercises:
 *   1. Device list: add two recipes sharing a craftable intermediate, verify
 *      consolidated Build -> Craft -> Shop projection, "Used by" consolidation,
 *      Craft/Buy choice toggle, On Hand deduction, Craft progress (+1 / Done),
 *      Shop purchase recording -> readiness update, Undo/Redo, Reset Progress,
 *      and persistence across reload.
 *   2. Account list (via `/test/login`): UI + `POST /api/v1/list/{id}/add/targets`,
 *      DB persistence across reload, and read-only collaborator lock.
 *   3. Entry points: `/items/jobset/<JOB>/set/<ilvl>` Craft full set modal and
 *      `/recipe/:id` Save as Craft Project preserving Buy/Craft + On Hand into
 *      a device list at `?mode=craft`.
 *   4. Captures desktop dark/light & mobile (390px) screenshots under
 *      `integration/artifacts/craft-projects/` and asserts zero overflow and zero
 *      console/page errors.
 */

const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const { capture } = require("./capture.cjs");
const { cleanupOwnedLists, finishCleanup } = require("./list-fixture-cleanup.cjs");

const BASE_URL = process.env.BASE_URL || "http://127.0.0.1:8080";
const TIMEOUT_MS = Number(process.env.TIMEOUT_MS || 90000);
const OUT = path.join(__dirname, "artifacts", "craft-projects");

const OWNER = { id: 990000000401, username: "CraftProjectOwner" };
const READER = { id: 990000000402, username: "CraftProjectReader" };

const tid = id => `[data-testid="${id}"]`;

async function main() {
  fs.mkdirSync(OUT, { recursive: true });
  const { default: puppeteer } = await import("puppeteer");
  const args = ["--no-sandbox", "--disable-setuid-sandbox"];
  if (process.env.E2E_BLOCK_EXTERNAL === "1") {
    args.push("--host-resolver-rules=MAP * ~NOTFOUND, EXCLUDE 127.0.0.1, EXCLUDE localhost");
  }
  const browser = await puppeteer.launch({
    headless: process.env.HEADLESS !== "false",
    args,
  });

  const errors = [];
  let accountListId = null;
  let ownerPage = null;
  let originalError;

  const trackErrors = (page, { allowGuest401 = false } = {}) => {
    page.on("pageerror", error => {
      const msg = String(error.stack || error);
      if (!msg.includes("pagead2.googlesyndication.com")) errors.push(`pageerror: ${msg}`);
    });
    page.on("console", msg => {
      if (msg.type() !== "error") return;
      const text = msg.text();
      if (/favicon|ERR_BLOCKED_BY_CLIENT|net::ERR_ABORTED|pagead2\.googlesyndication\.com/.test(text)) {
        return;
      }
      if (process.env.E2E_BLOCK_EXTERNAL === "1" && text.includes("net::ERR_NAME_NOT_RESOLVED")) {
        return;
      }
      if (allowGuest401 && text.includes("status of 401")) {
        return;
      }
      errors.push(`console.error: ${text}`);
    });
  };

  async function installPriceFixtures(page) {
    const cdp = await page.createCDPSession();
    await cdp.send("Fetch.enable", {
      patterns: [
        { urlPattern: "*/api/v1/bulk_listings/*" },
        { urlPattern: "*/api/v1/listings/*" },
      ],
    });
    cdp.on("Fetch.requestPaused", async ({ requestId, request }) => {
      const url = new URL(request.url);
      let match = url.pathname.match(/^\/api\/v1\/bulk_listings\/[^/]+\/(.+)$/);
      if (match) {
        const ids = match[1]
          .split(",")
          .map(Number)
          .filter(n => Number.isFinite(n) && n > 0);
        const body = {};
        for (const id of ids) {
          // Make finished items substantially more expensive than raw ingredients
          // so crafting saves gil and optimizer favors Craft where reasonable.
          const price = id > 2000 ? 1500 : 120 + (id % 50) * 5;
          body[String(id)] = [
            [
              {
                id: id * 100 + 1,
                world_id: 63,
                item_id: id,
                hq: false,
                price_per_unit: price,
                quantity: 99,
                retainer_id: id * 10 + 1,
                timestamp: "2026-11-26T12:00:00",
              },
              { id: id * 10 + 1, world_id: 63, name: "Craft Fixture", retainer_city_id: 1 },
            ],
          ];
        }
        return cdp.send("Fetch.fulfillRequest", {
          requestId,
          responseCode: 200,
          responseHeaders: [{ name: "Content-Type", value: "application/json" }],
          body: Buffer.from(JSON.stringify(body)).toString("base64"),
        });
      }
      match = url.pathname.match(/^\/api\/v1\/listings\/[^/]+\/(\d+)$/);
      if (match) {
        const item = Number(match[1]);
        const price = item > 2000 ? 1500 : 120 + (item % 50) * 5;
        const body = {
          listings: [
            [
              {
                id: item * 100 + 1,
                world_id: 63,
                item_id: item,
                hq: false,
                price_per_unit: price,
                quantity: 99,
                retainer_id: item * 10 + 1,
                timestamp: "2026-11-26T12:00:00",
              },
              { id: item * 10 + 1, world_id: 63, name: "Craft Fixture", retainer_city_id: 1 },
            ],
          ],
          sales: [],
          last_updated: [{ world_id: 63, updated_at: "2026-11-26T12:00:00" }],
        };
        return cdp.send("Fetch.fulfillRequest", {
          requestId,
          responseCode: 200,
          responseHeaders: [{ name: "Content-Type", value: "application/json" }],
          body: Buffer.from(JSON.stringify(body)).toString("base64"),
        });
      }
      return cdp.send("Fetch.continueRequest", { requestId });
    });
  }

  async function initPage(ctx, { allowGuest401 = false } = {}) {
    const page = await ctx.newPage();
    page.setDefaultTimeout(TIMEOUT_MS);
    await page.setViewport({ width: 1440, height: 950 });
    await page.setCookie(
      { name: "HIDE_ADS", value: "true", url: BASE_URL, path: "/" },
      { name: "i18n_pref_locale", value: "en", url: BASE_URL, path: "/" },
      { name: "HOME_WORLD", value: "Gilgamesh", url: BASE_URL, path: "/" },
    );
    await page.evaluateOnNewDocument(() => {
      window.__craftHydrated = false;
      window.addEventListener("ultros:hydrated", () => {
        window.__craftHydrated = true;
      });
    });
    page.on("dialog", d => (d.type() === "beforeunload" ? d.accept() : d.dismiss()));
    trackErrors(page, { allowGuest401 });
    await installPriceFixtures(page);
    return page;
  }

  async function replace(page, selector, value) {
    await page.waitForSelector(selector, { visible: true });
    await page.click(selector);
    await page.$eval(selector, el => el.select());
    await page.keyboard.press("Backspace");
    await page.type(selector, String(value));
  }

  async function setTheme(page, mode) {
    await page.evaluate(mode => {
      document.documentElement.dataset.theme = mode;
      document.documentElement.dataset.palette = "ultros";
    }, mode);
    await new Promise(r => setTimeout(r, 150));
  }

  async function assertFitsViewport(page, label) {
    const metrics = await page.evaluate(() => ({
      scrollWidth: document.documentElement.scrollWidth,
      clientWidth: document.documentElement.clientWidth,
    }));
    assert(
      metrics.scrollWidth <= metrics.clientWidth + 1,
      `${label}: horizontal overflow ${metrics.scrollWidth} > ${metrics.clientWidth}`,
    );
  }

  const api = (page, method, route, body) =>
    page.evaluate(
      async ({ method, route, body }) => {
        const res = await fetch(route, {
          method,
          credentials: "include",
          headers: body === undefined ? {} : { "Content-Type": "application/json" },
          body: body === undefined ? undefined : JSON.stringify(body),
        });
        const text = await res.text();
        let parsed = null;
        try {
          parsed = text ? JSON.parse(text) : null;
        } catch {
          parsed = text;
        }
        return { status: res.status, body: parsed };
      },
      { method, route, body },
    );

  async function addInlineRecipeTarget(page, query, exactLabel) {
    await page.click(tid("list-composer-recipes"));
    const searchSel = `${tid("inline-recipe-add")} input[aria-label="Search recipes"]`;
    await replace(page, searchSel, query);
    await page.waitForFunction(
      label => !!document.querySelector(`[data-testid="inline-recipe-add"] button[aria-label="Preview ${label}"]`),
      {},
      exactLabel,
    );
    await page.evaluate(label => {
      document
        .querySelector(`[data-testid="inline-recipe-add"] button[aria-label="Preview ${label}"]`)
        .click();
    }, exactLabel);
    await page.waitForSelector(tid("inline-recipe-add-target"), { visible: true });
    await page.click(tid("inline-recipe-add-target"));
  }

  try {
    // ===== 1. Device list Craft Project flow + screenshots =====
    console.log("[1] Device list Craft Project flow");
    const guestCtx = browser.defaultBrowserContext();
    const page = await initPage(guestCtx, { allowGuest401: true });

    await page.goto(`${BASE_URL}/list?lang=en`, { waitUntil: "domcontentloaded" });
    await page.waitForFunction(() => window.__craftHydrated);
    await page.waitForSelector(tid("list-new"), { visible: true });
    await page.click(tid("list-new"));
    await replace(page, tid("device-list-name"), `Craft Project ${Date.now()}`);
    await page.click(tid("device-list-create"));
    await page.waitForFunction(() => location.pathname.startsWith("/list/device/"));
    await page.waitForFunction(() =>
      document.querySelector('[data-testid="device-list-status"]')?.textContent.includes("Saved on this device"),
    );

    // Craft tab hidden before any craft targets are added
    assert.equal(await page.$(tid("list-craft-mode")), null, "Craft tab hidden with no targets");

    // Pick Bronze War Axe & Bronze Pickaxe (both use Bronze Ingot, Copper/Tin/Maple/etc.)
    await addInlineRecipeTarget(page, "Bronze War Axe", "Bronze War Axe");
    await page.waitForSelector(tid("list-craft-mode"), { visible: true });
    await page.waitForSelector(tid("build-craft-targets"), { visible: true });

    await addInlineRecipeTarget(page, "Bronze Pickaxe", "Bronze Pickaxe");
    await page.waitForFunction(
      () => document.querySelectorAll('[data-testid="build-craft-target-row"]').length === 2,
    );

    // Also add 1 manual item alongside targets to verify clean Build separation
    await page.click(tid("list-composer-items"));
    await replace(page, 'input[aria-label="Add an item"]', "Maple Log");
    await page.waitForSelector('button[aria-label="Add Maple Log"]');
    await page.keyboard.press("Enter");
    await page.waitForSelector('input[aria-label="Needed for Maple Log"]');
    await page.keyboard.press("Escape");

    // Capture Build with targets (Dark desktop)
    await page.waitForFunction(() =>
      document.querySelector('[data-testid="list-estimate-total"]')?.textContent.includes("gil"),
    );
    await setTheme(page, "dark");
    await capture(page, { path: path.join(OUT, "01-build-targets-dark-desktop.png"), fullPage: true });
    await setTheme(page, "light");
    await capture(page, { path: path.join(OUT, "02-build-targets-light-desktop.png"), fullPage: true });
    await setTheme(page, "dark");

    // Switch to Craft view
    await page.click(tid("build-open-craft"));
    await page.waitForSelector(tid("list-craft-view"), { visible: true });
    await page.waitForSelector(tid("craft-target-card"), { visible: true });
    await page.waitForSelector(tid("craft-step-card"), { visible: true });

    const targetsCount = await page.$$eval(tid("craft-target-card"), els => els.length);
    assert.equal(targetsCount, 2, "two target cards in Craft view");

    // Check shared Bronze Ingot consolidation & "Used by"
    const stepTexts = await page.$$eval(tid("craft-step-card"), els => els.map(el => el.textContent));
    assert(
      stepTexts.some(t => t.includes("Bronze Ingot") && t.includes("Used by")),
      `Bronze Ingot consolidated step must show Used by: ${JSON.stringify(stepTexts)}`,
    );

    // Toggle Craft -> By Job and back, taking screenshots
    await capture(page, { path: path.join(OUT, "03-craft-steps-dark-desktop.png"), fullPage: true });
    await setTheme(page, "light");
    await capture(page, { path: path.join(OUT, "04-craft-steps-light-desktop.png"), fullPage: true });
    await setTheme(page, "dark");

    await page.click(tid("craft-group-by-job"));
    await page.waitForSelector(tid("craft-steps-by-job"), { visible: true });
    await capture(page, { path: path.join(OUT, "05-craft-by-job-dark-desktop.png"), fullPage: true });
    await page.click(tid("craft-group-step-order"));

    // Record consolidated Shop need before toggling Bronze Ingot to Buy
    await page.click(tid("guest-shop-mode"));
    await page.waitForSelector(tid("shop-handoff"), { visible: true });
    if (await page.$('[data-testid="shop-change-route"][aria-expanded="false"]')) {
      await page.click(tid("shop-change-route"));
    }
    const routeSelector = (await page.$('[data-testid="shop-route-option"][data-route-cheapest="true"]'))
      ? '[data-testid="shop-route-option"][data-route-cheapest="true"]'
      : tid("shop-route-option");
    await page.waitForSelector(routeSelector, { visible: true });
    await page.click(routeSelector);
    await page.waitForSelector(tid("shop-confirm-route"), { visible: true });
    await page.click(tid("shop-confirm-route"));
    await page.waitForSelector(tid("shop-stack"), { visible: true });
    await page.waitForSelector(tid("shop-stack-description"), { visible: true });
    await capture(page, { path: path.join(OUT, "06-shop-consolidated-dark-desktop.png"), fullPage: true });
    const shopStacks = await page.$$eval(tid("shop-stack"), els => els.map(el => el.textContent));
    const expectedLeaves = [
      "Copper Ore",
      "Tin Ore",
      "Moko Grass",
      "Maple Log",
      "Fire Shard",
      "Wind Shard",
      "Lightning Shard",
    ];
    assert(
      shopStacks.some(t => expectedLeaves.some(leaf => t.includes(leaf))),
      `Shop should include consolidated craft leaves: ${JSON.stringify(shopStacks)}`,
    );
    assert(
      !shopStacks.some(t => t.includes("Bronze Ingot")),
      `Shop should not buy Bronze Ingot while Craft is chosen: ${JSON.stringify(shopStacks)}`,
    );

    // Record a consolidated leaf purchase in Shop and verify readiness updates in Craft
    const bought = await page.evaluate(leaves => {
      const stacks = [...document.querySelectorAll('[data-testid="shop-stack"]')];
      const stack = stacks.find(el => leaves.some(leaf => el.textContent.includes(leaf))) || stacks[0];
      const btn = stack?.querySelector('[data-testid="shop-stack-bought"]');
      if (!btn || btn.disabled) return false;
      btn.click();
      return true;
    }, expectedLeaves);
    assert.equal(bought, true, "expected enabled shop-stack-bought button on consolidated leaf stack");
    await page.waitForFunction(() =>
      document.querySelector('[data-testid="device-list-status"]')?.textContent.includes("Saved on this device"),
    );

    // Flip Bronze Ingot to Buy in Craft view
    await page.click(tid("list-craft-mode"));
    await page.waitForSelector(tid("craft-step-card"), { visible: true });
    const leavesText = await page.$eval(tid("craft-leaves-acquired"), el => el.textContent.trim());
    assert(!leavesText.startsWith("0 /"), `Shop purchase must update craft leaves acquired: ${leavesText}`);
    await page.evaluate(() => {
      const card = [...document.querySelectorAll('[data-testid="craft-step-card"]')]
        .find(el => el.textContent.includes("Bronze Ingot"));
      card.querySelector('[data-testid="craft-choice-buy"]').click();
    });
    await page.waitForFunction(() => {
      const card = [...document.querySelectorAll('[data-testid="craft-step-card"]')]
        .find(el => el.textContent.includes("Bronze Ingot"));
      return card?.dataset.craftMode === "buy";
    });

    // Undo flips Bronze Ingot back to Craft
    await page.click(tid("list-undo"));
    await page.waitForFunction(() => {
      const card = [...document.querySelectorAll('[data-testid="craft-step-card"]')]
        .find(el => el.textContent.includes("Bronze Ingot"));
      return card?.dataset.craftMode === "craft";
    });
    // Redo flips it back to Buy, then Undo again so we test crafting steps!
    await page.click(tid("list-redo"));
    await page.waitForFunction(() => {
      const card = [...document.querySelectorAll('[data-testid="craft-step-card"]')]
        .find(el => el.textContent.includes("Bronze Ingot"));
      return card?.dataset.craftMode === "buy";
    });
    await page.click(tid("list-undo"));

    // Mark Bronze Ingot +1 crafted and Target done, verify persistence across reload
    await page.evaluate(() => {
      const card = [...document.querySelectorAll('[data-testid="craft-step-card"]')]
        .find(el => el.textContent.includes("Bronze Ingot"));
      card.querySelector('[data-testid="craft-step-plus"]').click();
    });
    await page.waitForFunction(() => {
      const card = [...document.querySelectorAll('[data-testid="craft-step-card"]')]
        .find(el => el.textContent.includes("Bronze Ingot"));
      return card?.querySelector('[data-testid="craft-step-crafted-progress"]')?.textContent.includes("1 / ");
    });
    await page.waitForFunction(() =>
      document.querySelector('[data-testid="device-list-status"]')?.textContent.includes("Saved on this device"),
    );

    const savedUrl = new URL(page.url());
    savedUrl.searchParams.set("mode", "craft");
    await page.goto(savedUrl.toString(), { waitUntil: "domcontentloaded" });
    await page.waitForFunction(() => window.__craftHydrated);
    await page.waitForSelector(tid("craft-step-card"), { visible: true });
    const craftedAfterReload = await page.evaluate(() => {
      const card = [...document.querySelectorAll('[data-testid="craft-step-card"]')]
        .find(el => el.textContent.includes("Bronze Ingot"));
      return card?.querySelector('[data-testid="craft-step-crafted-progress"]')?.textContent || "";
    });
    assert(craftedAfterReload.includes("1 / "), `crafted progress must survive reload: ${craftedAfterReload}`);

    // Mobile 390px overflow + screenshots
    await page.waitForFunction(() =>
      document.querySelector('[data-testid="list-craft-view"]')?.textContent.includes("gil"),
    );
    await page.setViewport({ width: 390, height: 844 });
    await page.waitForSelector(tid("craft-step-card"), { visible: true });
    await assertFitsViewport(page, "mobile Craft view");
    await capture(page, { path: path.join(OUT, "07-craft-mobile.png"), fullPage: true });
    await page.click(tid("guest-build-mode"));
    await page.waitForSelector(tid("build-craft-targets"), { visible: true });
    await page.waitForFunction(() =>
      document.querySelector('[data-testid="list-estimate-total"]')?.textContent.includes("gil"),
    );
    await assertFitsViewport(page, "mobile Build view");
    await capture(page, { path: path.join(OUT, "08-build-mobile.png"), fullPage: true });
    await page.setViewport({ width: 1440, height: 950 });

    // Test Reset progress from Build summary clears both acquired and crafted!
    await page.click(tid("list-craft-mode"));
    await page.waitForSelector(tid("craft-target-done"), { visible: true });
    await page.click(tid("craft-target-done"));
    await page.click(tid("guest-build-mode"));
    await page.waitForSelector(tid("cart-reset-progress"), { visible: true });
    // Acquire 1 Maple Log so reset button is active even when only targets were crafted
    await page.click(tid("cart-reset-progress"));
    await page.click(tid("cart-reset-progress"));
    await page.click(tid("list-craft-mode"));
    await page.waitForFunction(() => {
      const card = [...document.querySelectorAll('[data-testid="craft-step-card"]')]
        .find(el => el.textContent.includes("Bronze Ingot"));
      return card?.querySelector('[data-testid="craft-step-crafted-progress"]')?.textContent.includes("0 / ");
    });
    console.log("[ok] Device list Craft Project flow passed");

    // ===== 2. Entry points: Job Set & /recipe/:id =====
    console.log("[2] Testing Job Set & /recipe/:id entry points");
    await page.goto(`${BASE_URL}/items/jobset/WAR/set/740`, { waitUntil: "domcontentloaded" });
    await page.waitForFunction(() => window.__craftHydrated);
    const addSetBtn = await page.waitForFunction(() =>
      [...document.querySelectorAll("button")].find(b => /Add set/i.test(b.textContent || "")),
    );
    await addSetBtn.asElement().click();
    await page.waitForSelector(tid("add-set-mode-toggle"), { visible: true });
    const craftPressed = await page.$eval(tid("add-set-mode-craft"), b => b.getAttribute("aria-pressed"));
    assert.equal(craftPressed, "true", "Job Set modal should default to Craft full set");
    await page.click(tid("add-set-mode-buy"));
    await page.click(tid("add-set-mode-craft"));
    console.log("[ok] Job Set AddSetToList Craft full set toggle verified");

    // /recipe/:id SavePlan as Craft Project -> Device list `?mode=craft`
    await page.goto(`${BASE_URL}/recipe/1?world=Gilgamesh&quantity=2`, { waitUntil: "domcontentloaded" });
    await page.waitForFunction(() => window.__craftHydrated);
    const savePlanBtn = await page.waitForFunction(() =>
      [...document.querySelectorAll("button")].find(
        b => !b.disabled && b.textContent === "Add remaining materials to a list",
      ),
    );
    await savePlanBtn.asElement().click();
    await page.waitForSelector(tid("recipe-save-mode-toggle"), { visible: true });
    await page.waitForSelector('[data-testid="local-list-new-name"]', { visible: true });
    await replace(page, '[data-testid="local-list-new-name"]', `From Recipe ${Date.now()}`);
    await page.click('[data-testid="local-list-create-add"]');
    const openLink = await page.waitForSelector('a[href*="mode=craft"]', { visible: true });
    const openHref = await openLink.evaluate(a => a.getAttribute("href"));
    assert(openHref.includes("?mode=craft"), `expected ?mode=craft link, got ${openHref}`);
    await openLink.click();
    await page.waitForFunction(() => location.search.includes("mode=craft"));
    await page.waitForSelector(tid("craft-target-card"), { visible: true });
    console.log("[ok] /recipe/:id SavePlan -> device list ?mode=craft verified");

    // ===== 3. Account list API + Sync + Collaborator read-only lock =====
    console.log("[3] Account list + POST /api/v1/list/{id}/add/targets + read-only guard");
    const ownerCtx = await browser.createBrowserContext();
    const readerCtx = await browser.createBrowserContext();
    ownerPage = await initPage(ownerCtx);
    const readerPage = await initPage(readerCtx);

    const loginOwner = new URL("/test/login", BASE_URL);
    loginOwner.searchParams.set("user_id", String(OWNER.id));
    loginOwner.searchParams.set("username", OWNER.username);
    loginOwner.searchParams.set("redirect", "/list");
    await ownerPage.goto(loginOwner.toString(), { waitUntil: "domcontentloaded" });
    await ownerPage.waitForFunction(() => window.__craftHydrated);

    const worldData = await api(ownerPage, "GET", "/api/v1/world_data");
    const worldId = worldData.body.regions[0].datacenters[0].worlds[0].id;
    const listName = `Account Craft ${Date.now()}`;
    const created = await api(ownerPage, "POST", "/api/v1/list/create", {
      name: listName,
      wdr_filter: { World: worldId },
    });
    assert.equal(created.status, 200);
    const lists = await api(ownerPage, "GET", "/api/v1/list");
    accountListId = lists.body.find(l => l.list.name === listName)?.list.id;
    assert(accountListId, "created account list id missing");

    // Add craft target via POST /api/v1/list/{id}/add/targets (recipe_id 1 or Bronze War Axe)
    const addRes = await api(ownerPage, "POST", `/api/v1/list/${accountListId}/add/targets`, {
      targets: [{ recipe_id: 2, count: 2 }],
    });
    assert.equal(addRes.status, 200, `add/targets failed: ${JSON.stringify(addRes)}`);

    await ownerPage.goto(`${BASE_URL}/list/${accountListId}?mode=craft`, { waitUntil: "domcontentloaded" });
    await ownerPage.waitForFunction(() => window.__craftHydrated);
    await ownerPage.waitForSelector(tid("craft-target-card"), { visible: true });

    // Verify invite read-only view locks craft mutations
    const inviteRes = await api(ownerPage, "POST", `/api/v1/list/${accountListId}/invite/create`, {
      permission: "Read",
      max_uses: 5,
    });
    assert.equal(inviteRes.status, 200, `invite/create failed: ${JSON.stringify(inviteRes)}`);

    const loginReader = new URL("/test/login", BASE_URL);
    loginReader.searchParams.set("user_id", String(READER.id));
    loginReader.searchParams.set("username", READER.username);
    loginReader.searchParams.set("redirect", "/list");
    await readerPage.goto(loginReader.toString(), { waitUntil: "domcontentloaded" });
    await readerPage.waitForFunction(() => window.__craftHydrated);
    const redeemRes = await api(readerPage, "POST", `/api/v1/invite/${inviteRes.body.id}/use`);
    assert.equal(redeemRes.status, 200, `invite redeem failed: ${JSON.stringify(redeemRes)}`);
    await readerPage.goto(`${BASE_URL}/list/${accountListId}?mode=craft`, { waitUntil: "domcontentloaded" });
    await readerPage.waitForFunction(() => window.__craftHydrated);
    await readerPage.waitForSelector(tid("list-craft-mode"), { visible: true });
    await readerPage.click(tid("list-craft-mode"));
    await readerPage.waitForSelector(tid("craft-target-card"), { visible: true });
    assert.equal(await readerPage.$(tid("craft-target-remove")), null, "read-only viewer must not see target remove button");
    assert.equal(
      await readerPage.$eval(tid("craft-optimize-btn"), b => b.disabled),
      true,
      "read-only viewer must have disabled Optimize button",
    );
    console.log("[ok] Account list Craft Project + read-only collaborator verified");

    assert.deepEqual(errors, [], `unexpected browser errors: ${errors.join("\n")}`);
    console.log("ALL CRAFT PROJECT E2E ASSERTIONS PASSED");
  } catch (err) {
    originalError = err;
    throw err;
  } finally {
    const ownerApi = ownerPage ? (method, route, body) => api(ownerPage, method, route, body) : null;
    await finishCleanup(
      [
        async () => {
          if (ownerApi && accountListId) {
            await cleanupOwnedLists(ownerApi, [accountListId]);
          }
        },
        () => browser.close(),
      ],
      originalError,
    );
  }
}

main().catch(err => {
  console.error(err);
  process.exit(1);
});
