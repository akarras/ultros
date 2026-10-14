#!/usr/bin/env node
/* eslint-disable no-console */
/**
 * List flow E2E. Requires the server to be built with `--features test-auth`.
 *
 * Exercises the /list/{id} page:
 *   1. Owner creates a list via the API.
 *   2. Owner adds an item via the UI search-and-add panel.
 *   3. Owner adds a recipe via the inline recipe composer.
 *   4. Owner marks an item acquired via the Owned editor.
 *   5. Owner renames the list inline and creates an invite through Access.
 *   6. Reader redeems the invite and sees the list with read-only chrome
 *      (no composer or Access; More options present for leaving).
 *   7. Owner deletes the list and lands back on /list.
 *
 * Env:
 *   BASE_URL     default http://127.0.0.1:8080
 *   HEADLESS     "false" to watch, anything else runs headless
 *   TIMEOUT_MS   default 30000
 */

"use strict";

const USERS = {
  owner: { id: 990000000001, username: "ListFlowOwner" },
  reader: { id: 990000000002, username: "ListFlowReader" },
};

async function login(page, baseUrl, user) {
  const url = new URL("/test/login", baseUrl);
  url.searchParams.set("user_id", String(user.id));
  url.searchParams.set("username", user.username);
  url.searchParams.set("redirect", "/list");
  const resp = await page.goto(url.toString(), { waitUntil: "domcontentloaded" });
  if (!resp || resp.status() >= 400) {
    throw new Error(`test login failed for ${user.username}: ${resp ? resp.status() : -1}`);
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
      try {
        parsed = text ? JSON.parse(text) : null;
      } catch {
        parsed = text;
      }
      return { status: r.status, body: parsed };
    },
    { method, path, body },
  );
}

function fail(failures, msg) {
  console.error(`  X ${msg}`);
  failures.push(msg);
}

function pass(msg) {
  console.log(`  + ${msg}`);
}

async function waitFor(page, selector, timeout) {
  return page.waitForSelector(selector, { timeout, visible: true });
}

// Wait until the WASM client has hydrated AND the view_caps Effect has
// updated permission-gated affordances. The More-options button is always
// rendered, so wait for it first. Then wait for an owner-only button to
// appear (proves view_caps fired with can_admin=true).
async function waitForHydration(page, timeout) {
  await page.waitForFunction(
    () => !!document.querySelector('[data-testid="list-settings-btn"]'),
    { timeout },
  );
  await page.waitForFunction(
    () => !!document.querySelector('[data-testid="inline-list-add"]'),
    { timeout },
  );
}

async function clickByText(page, selector, text) {
  // Click via evaluate so we trigger the listener directly even when the
  // element is inside a Tooltip wrapper (which may intercept synthetic
  // pointer events).
  return page.evaluate(
    (sel, t) => {
      const norm = (s) => (s || "").replace(/\s+/g, " ").trim();
      const target = norm(t);
      const el = Array.from(document.querySelectorAll(sel)).find((e) =>
        norm(e.innerText).includes(target),
      );
      if (!el) return false;
      el.click();
      return true;
    },
    selector,
    text,
  );
}

async function main() {
  const puppeteer = require("puppeteer");
  const BASE_URL = process.env.BASE_URL || "http://127.0.0.1:8080";
  const TIMEOUT_MS = Number(process.env.TIMEOUT_MS || 30000);
  const headless = process.env.HEADLESS !== "false";

  const browser = await puppeteer.launch({
    headless,
    args: ["--no-sandbox", "--disable-setuid-sandbox"],
  });

  const failures = [];
  let listId = null;

  try {
    // Use separate browser contexts so each user keeps their own cookie jar.
    // The default context is shared across pages — the second /test/login
    // would overwrite the first's discord_auth cookie.
    const ownerContext = await browser.createBrowserContext();
    const readerContext = await browser.createBrowserContext();
    const ownerPage = await ownerContext.newPage();
    const readerPage = await readerContext.newPage();
    for (const p of [ownerPage, readerPage]) {
      p.setDefaultTimeout(TIMEOUT_MS);
      // Force desktop viewport so the list-toolbar buttons render in their
      // full labeled form (the layout collapses below ~lg breakpoint).
      await p.setViewport({ width: 1280, height: 900, deviceScaleFactor: 1 });
    }

    // ===== Owner setup =====
    console.log("[step] owner + reader login");
    await login(ownerPage, BASE_URL, USERS.owner);
    await login(readerPage, BASE_URL, USERS.reader);

    // Create a list via the API (faster + reliable).
    const worldData = await api(ownerPage, "GET", "/api/v1/world_data");
    if (worldData.status !== 200) {
      fail(failures, `world_data expected 200, got ${worldData.status}`);
      throw new Error("cannot continue");
    }
    const worldId = worldData.body.regions[0].datacenters[0].worlds[0].id;
    const name = `ListFlow E2E ${Date.now()}`;
    const create = await api(ownerPage, "POST", "/api/v1/list/create", {
      name,
      wdr_filter: { World: worldId },
    });
    if (create.status !== 200) {
      fail(failures, `create list expected 200, got ${create.status}`);
      throw new Error("cannot continue");
    }
    const ownerLists = await api(ownerPage, "GET", "/api/v1/list");
    const owned = ownerLists.body.find((e) => e.list.name === name);
    if (!owned) {
      fail(failures, "created list not returned to owner");
      throw new Error("cannot continue");
    }
    listId = owned.list.id;
    pass(`created list ${listId} via API`);

    // ===== Step 1: Add an item via the UI =====
    console.log("[step] owner adds an item via the UI");
    const listUrl = new URL(`/list/${listId}`, BASE_URL).toString();
    await ownerPage.goto(listUrl, { waitUntil: "domcontentloaded" });
    await waitForHydration(ownerPage, TIMEOUT_MS);

    await ownerPage.waitForSelector('[data-testid="list-view-sync"]');
    {
      await ownerPage.type('input[aria-label="Add an item"]', "Maple Log");
      await ownerPage.waitForSelector('button[aria-label="Add Maple Log"]');
      await ownerPage.keyboard.press("Enter");
      await ownerPage.waitForSelector('input[aria-label="Needed for Maple Log"]');
      await ownerPage.waitForFunction(async id => {
        const response = await fetch(`/api/v1/list/${id}/listings`);
        return response.ok && (await response.json())[1].length >= 1;
      }, { timeout: TIMEOUT_MS }, listId);
      await ownerPage.keyboard.press("Escape");
      pass("added item through the permanent inline composer and synchronized it");
    }

    // ===== Step 2: Add a recipe via the current UI =====
    console.log("[step] owner adds a recipe");
    // Lists 2.0: recipes come from the composer's Items / Recipes toggle.
    const recipeToggle = ['[data-testid="inline-list-add"] button', "Recipes"];
    if (!(await clickByText(ownerPage, ...recipeToggle))) {
      fail(failures, "Add Recipe button not found");
    } else {
      try {
        {
          const composer = '[data-testid="inline-recipe-add"]';
          await ownerPage.waitForSelector(composer);
          await ownerPage.type(`${composer} input[aria-label="Search recipes"]`, "Bronze Ingot");
          // Each result opens a preview before items are added to the cart.
          await ownerPage.waitForFunction(selector => !!document.querySelector(`${selector} button[aria-label="Preview Bronze Ingot"]`), {}, composer);
          // Catalog names overlap (e.g. Standard Treated Bronze Ingot), so
          // select the exact ordinary recipe used by this fixture.
          await ownerPage.evaluate(selector => {
            const button = document.querySelector(`${selector} button[aria-label="Preview Bronze Ingot"]`);
            if (!button) throw new Error("Exact Bronze Ingot recipe is missing");
            button.click();
          }, composer);
          await ownerPage.waitForSelector(`${composer} [aria-label="Recipe preview"] li`);
          await ownerPage.waitForFunction(selector => {
            const text = document.querySelector(`${selector} [aria-label="Recipe preview"]`)?.textContent || "";
            return text.includes("2 × Copper Ore") && text.includes("1 × Tin Ore") && text.includes("Shard");
          }, {}, composer);
          await ownerPage.select(`${composer} [aria-label="Recipe items to add"]`, "finished");
          await ownerPage.waitForFunction(selector =>
            document.querySelector(`${selector} [aria-label="Recipe preview"]`)?.textContent === "1 × Bronze Ingot",
          {}, composer);
          await ownerPage.select(`${composer} [aria-label="Recipe items to add"]`, "ingredients");
          await ownerPage.waitForFunction(selector =>
            document.querySelectorAll(`${selector} [aria-label="Recipe preview"] li`).length === 3,
          {}, composer);
          await clickByText(ownerPage, `${composer} button`, "Add previewed items");
          await ownerPage.waitForFunction(async id => {
            const response = await fetch(`/api/v1/list/${id}/listings`);
            return response.ok && (await response.json())[1].length > 1;
          }, { timeout: TIMEOUT_MS }, listId);
          await clickByText(ownerPage, '[data-testid="inline-list-add"] button', "Items");
          pass("previewed and added recipe ingredients inline");
        }
      } catch (e) {
        console.error("Recipe preview state:", await ownerPage.$eval('[data-testid="inline-recipe-add"]', el => el.outerHTML).catch(() => "missing composer"));
        fail(failures, `inline recipe interaction failed: ${e.message || e}`);
      }
    }

    // ===== Step 2b: Auto-mark purchases modal =====
    console.log("[step] owner opens the auto-mark purchases modal");
    // Lists 2.0 keeps the button in the shell's More-options menu.
    {
      await ownerPage.click('[data-testid="list-settings-btn"]');
      await waitFor(ownerPage, '[data-testid="list-auto-mark-btn"]', 10000);
    }
    const autoMarkClicked = await ownerPage.evaluate(() => {
      const b = document.querySelector('[data-testid="list-auto-mark-btn"]');
      if (!b) return false;
      b.click();
      return true;
    });
    if (!autoMarkClicked) {
      fail(failures, "auto-mark toolbar button not found");
    } else {
      const modalShown = await ownerPage
        .waitForFunction(
          () => {
            // Topmost dialog: under Lists 2.0 the More-options menu sits beneath it.
            const dialog = [...document.querySelectorAll('[role="dialog"]')].at(-1);
            return !!dialog && !!dialog.querySelector('input[placeholder]');
          },
          { timeout: 10000 },
        )
        .then(() => true)
        .catch(() => false);
      if (!modalShown) {
        fail(failures, "auto-mark modal did not open");
      } else {
        const hasNameInput = await ownerPage.evaluate(() => {
          const dialog = [...document.querySelectorAll('[role="dialog"]')].at(-1);
          return (
            !!dialog &&
            !!Array.from(dialog.querySelectorAll("input[placeholder]")).find((i) =>
              /character name/i.test(i.placeholder),
            )
          );
        });
        if (!hasNameInput) {
          fail(failures, "auto-mark modal missing character-name input");
        } else {
          pass("auto-mark modal opens with character-name input");
        }
        await ownerPage.keyboard.press("Escape");
        // Under Lists 2.0 the More-options menu is still open underneath.
        {
          await new Promise((r) => setTimeout(r, 300));
          await ownerPage.keyboard.press("Escape");
        }
        await ownerPage
          .waitForFunction(() => !document.querySelector('[role="dialog"]'), {
            timeout: 5000,
          })
          .catch(() => {});
      }
    }

    // ===== Step 3: Mark an item acquired via the Owned editor =====
    console.log("[step] owner marks an item acquired");
    {
      const neededInput = 'input[aria-label="Needed for Maple Log"]';
      const ownedInput = 'input[aria-label="Owned for Maple Log"]';
      const qualitySelect = 'select[aria-label="Quality for Maple Log"]';
      const nextNeeded = await ownerPage.$eval(neededInput, element => Number(element.value) + 1);
      const editedItem = await ownerPage.$eval(neededInput, element => Number(element.closest('[data-item-id]').dataset.itemId));
      await ownerPage.$eval(qualitySelect, element => { window.__qualityBeforeCommit = element; });
      await ownerPage.locator(neededInput).fill(String(nextNeeded));
      await ownerPage.keyboard.press("Tab");
      await ownerPage.waitForFunction(async ({ id, quantity, itemId }) => {
        const response = await fetch(`/api/v1/list/${id}/listings`);
        return response.ok && (await response.json())[1].some(([item]) => item.item_id === itemId && item.quantity === quantity);
      }, { timeout: TIMEOUT_MS }, { id: listId, quantity: nextNeeded, itemId: editedItem });
      if (!await ownerPage.$eval(qualitySelect, element =>
        element === window.__qualityBeforeCommit && document.activeElement === element)) {
        fail(failures, "Tab after Needed commit must retain the Quality control and focus through server acknowledgement");
      } else {
        pass("Tab commits Needed without replacing the next editor or dropping focus");
      }
      // Owned lives behind the compact cart's details toggle.
      await ownerPage.click('button[aria-label="Details for Maple Log"]');
      await ownerPage.waitForSelector(ownedInput, { visible: true });
      await ownerPage.locator(ownedInput).fill("1");
      await ownerPage.keyboard.press("Enter");
      await ownerPage.waitForFunction(async id => {
        const response = await fetch(`/api/v1/list/${id}/listings`);
        return response.ok && (await response.json())[1].some(([item]) => item.acquired >= 1);
      }, { timeout: TIMEOUT_MS }, listId);
      pass("inline Owned edit records acquisition in the synchronized account list");
    }

    // ===== Step 4: rename + invite =====
    console.log("[step] owner renames inline and opens Access");
    {
      const newName = `${name} (renamed)`;
      const nameInput = '[data-testid="list-name-input"]';
      await ownerPage.waitForFunction(
        selector => { const input = document.querySelector(selector); return !!input && !input.readOnly; },
        { timeout: 10000 },
        nameInput,
      );
      await ownerPage.locator(nameInput).fill(newName);
      await ownerPage.keyboard.press("Enter");
      await ownerPage
        .waitForFunction(
          () => (document.title || "").includes("(renamed)"),
          { timeout: 10000 },
        )
        .catch(() => {});
      const title = await ownerPage.title();
      if (!title.includes("(renamed)")) {
        fail(failures, `expected the document title to include '(renamed)', got '${title}'`);
      } else {
        pass("renamed list inline");
      }
      await ownerPage.click('[data-testid="list-access-btn"]');
      await waitFor(ownerPage, '[data-testid="list-invite-create"]', 10000);
      await ownerPage.click('[data-testid="list-invite-create"]');
      await new Promise((r) => setTimeout(r, 2000));
      const invitesResp = await api(ownerPage, "GET", `/api/v1/list/${listId}/invites`);
      if (
        invitesResp.status !== 200 ||
        !Array.isArray(invitesResp.body) ||
        invitesResp.body.length === 0
      ) {
        fail(
          failures,
          `expected at least 1 invite, got ${invitesResp.status} body=${JSON.stringify(invitesResp.body)}`,
        );
      } else {
        pass(`created invite via Access (${invitesResp.body.length} invite(s))`);
        const inviteId = invitesResp.body[invitesResp.body.length - 1].id;
        const redeem = await api(readerPage, "POST", `/api/v1/invite/${inviteId}/use`);
        if (redeem.status !== 200 || redeem.body !== listId) {
          fail(
            failures,
            `invite redeem expected 200 + listId ${listId}, got ${redeem.status} body=${redeem.body}`,
          );
        } else {
          pass("reader redeemed invite");
          await readerPage.goto(`${BASE_URL}/list/${listId}`, { waitUntil: "domcontentloaded" });
          await readerPage.waitForSelector('[data-testid="list-name-input"]', { timeout: TIMEOUT_MS });
          await new Promise((r) => setTimeout(r, 1500));
          const viewer = await readerPage.evaluate(() => ({
            hasAddItem: !!document.querySelector('[data-testid="inline-list-add"]'),
            hasMenu: !!document.querySelector('[data-testid="list-settings-btn"]'),
            hasAccess: !!document.querySelector('[data-testid="list-access-btn"]'),
            nameReadOnly: !!document.querySelector('[data-testid="list-name-input"]')?.readOnly,
          }));
          if (viewer.hasAddItem) fail(failures, "read-only viewer should NOT see the composer"); else pass("read-only viewer hides the composer");
          if (!viewer.hasMenu) fail(failures, "read-only viewer should see the More menu (for Leave)"); else pass("read-only viewer sees the More menu");
          if (viewer.hasAccess) fail(failures, "read-only viewer should NOT see Access"); else pass("read-only viewer hides Access");
          if (!viewer.nameReadOnly) fail(failures, "read-only viewer's name input must be read-only"); else pass("read-only viewer cannot rename");
        }
      }
      await ownerPage.keyboard.press("Escape");
      await ownerPage
        .waitForFunction(() => !document.querySelector('[role="dialog"]'), { timeout: 5000 })
        .catch(() => {});
    }

    // ===== Step 6: Delete the list =====
    console.log("[step] owner deletes the list");
    const settingsClicked2 = await ownerPage.evaluate(() => {
      const b = document.querySelector('[data-testid="list-settings-btn"]');
      if (!b) return false;
      b.click();
      return true;
    });
    if (settingsClicked2) {
      // The More menu opens a separate deletion confirmation.
      await waitFor(ownerPage, '[data-testid="list-danger-zone"]', 10000);
      // Click in-page by selector rather than through an element handle: the
      // drawer is re-rendered whenever the list resource changes (a market
      // update or a list broadcast), and a handle taken a moment earlier is
      // then "not clickable or not an Element".
      const clickDelete = (selector = '[data-testid="list-delete-btn"]') =>
        ownerPage.evaluate((selector) => {
          const b = document.querySelector(selector);
          if (!b) return false;
          b.click();
          return true;
        }, selector);
      const deleteBtn = await clickDelete();
      if (!deleteBtn) {
        fail(failures, "delete button not found");
      } else {
        // first click: confirm prompt
        // Wait for the button to re-render into its confirm state rather
        // than sleeping — the dev-build WASM can take >500ms to apply it.
        await ownerPage
          .waitForFunction(
            () => !!document.querySelector('[data-testid="list-confirm-delete"]'),
            { timeout: 10000 },
          )
          .catch(() => {});
        const deleteBtn2 = await clickDelete('[data-testid="list-confirm-delete"]');
        if (!deleteBtn2) {
          fail(failures, "delete confirm button not found");
        } else {
          await ownerPage
            .waitForFunction(() => window.location.pathname === "/list", { timeout: 15000 })
            .catch(() => {});
          if (new URL(ownerPage.url()).pathname === "/list") {
            pass("owner returned to /list after delete");
          } else {
            fail(failures, `expected path /list, got ${ownerPage.url()}`);
          }
          const checkResp = await api(ownerPage, "GET", "/api/v1/list");
          const stillThere = (checkResp.body || []).find((e) => e.list.id === listId);
          if (stillThere) {
            fail(failures, `list ${listId} still exists after delete`);
          } else {
            pass(`list ${listId} no longer in API list`);
          }
        }
      }
    }

    for (const p of [ownerPage, readerPage]) await p.close();
  } finally {
    await browser.close();
  }

  if (failures.length) {
    console.error(`[fail] ${failures.length} list-flow assertion(s) failed`);
    process.exit(1);
  }
  console.log("[ok] list flow passed");
}

main().catch((err) => {
  console.error("[error]", err && err.stack ? err.stack : err);
  process.exit(1);
});
