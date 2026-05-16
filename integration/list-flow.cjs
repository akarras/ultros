#!/usr/bin/env node
/* eslint-disable no-console */
/**
 * List flow E2E. Requires the server to be built with `--features test-auth`.
 *
 * Exercises the /list/{id} page:
 *   1. Owner creates a list via the API.
 *   2. Owner adds an item via the UI search-and-add panel.
 *   3. Owner adds a recipe via the recipe modal.
 *   4. Owner marks an item acquired via the row toggle.
 *   5. Owner opens the settings drawer, renames the list, creates an invite.
 *   6. Reader redeems the invite and sees the list with read-only chrome
 *      (no Add Item; Settings present for leave; Notify present).
 *   7. Owner deletes the list and lands back on /list.
 *
 * Env:
 *   BASE_URL    default http://127.0.0.1:8080
 *   HEADLESS    "false" to watch, anything else uses puppeteer's "new" mode
 *   TIMEOUT_MS  default 30000
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

// Find a button (or any element) whose visible text matches `text`.
// Returns an ElementHandle or null.
async function findByText(page, selector, text) {
  return page.evaluateHandle(
    (sel, t) => {
      const norm = (s) => (s || "").replace(/\s+/g, " ").trim();
      const target = norm(t);
      const elems = Array.from(document.querySelectorAll(sel));
      return elems.find((el) => norm(el.innerText).includes(target)) || null;
    },
    selector,
    text,
  );
}

async function clickByText(page, selector, text) {
  const handle = await findByText(page, selector, text);
  const el = handle.asElement();
  if (!el) {
    await handle.dispose();
    return false;
  }
  await el.click();
  await handle.dispose();
  return true;
}

async function main() {
  const puppeteer = require("puppeteer");
  const BASE_URL = process.env.BASE_URL || "http://127.0.0.1:8080";
  const TIMEOUT_MS = Number(process.env.TIMEOUT_MS || 30000);
  const headless = process.env.HEADLESS === "false" ? false : "new";

  const browser = await puppeteer.launch({
    headless,
    args: ["--no-sandbox", "--disable-setuid-sandbox"],
  });

  const failures = [];
  let listId = null;

  try {
    const ownerPage = await browser.newPage();
    const readerPage = await browser.newPage();
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
    await ownerPage.waitForFunction(() => !!document.querySelector("h1"), { timeout: TIMEOUT_MS });

    if (!(await clickByText(ownerPage, ".list-toolbar button", "Add Item"))) {
      fail(failures, "Add Item button not found");
    } else {
      await waitFor(ownerPage, "input[placeholder]", 5000);
      // The search input is the most recently-rendered placeholder input.
      const inputs = await ownerPage.$$("input[placeholder]");
      const searchInput = inputs[inputs.length - 1];
      await searchInput.click({ clickCount: 3 });
      await searchInput.type("Maple Log");
      await new Promise((r) => setTimeout(r, 1000));
      // Click a row-level "Add" button.
      if (!(await clickByText(ownerPage, "button", "Add"))) {
        fail(failures, "row-level Add button not found");
      } else {
        await new Promise((r) => setTimeout(r, 1500));
        const apiRes = await api(ownerPage, "GET", `/api/v1/list/${listId}/listings`);
        const itemsLen = apiRes.body && apiRes.body[1] ? apiRes.body[1].length : 0;
        if (itemsLen < 1) {
          fail(failures, `expected >=1 item after add, api items=${itemsLen}`);
        } else {
          pass(`added item via UI (api items=${itemsLen})`);
        }
      }
    }

    // ===== Step 2: Add a recipe via the modal =====
    console.log("[step] owner adds a recipe");
    if (!(await clickByText(ownerPage, ".list-toolbar button", "Add Recipe"))) {
      fail(failures, "Add Recipe button not found");
    } else {
      try {
        await waitFor(ownerPage, "input[placeholder]", 5000);
        const inputs = await ownerPage.$$("input[placeholder]");
        // After the modal opens, the modal's search input is the most recent.
        const modalInput = inputs[inputs.length - 1];
        await modalInput.click({ clickCount: 3 });
        await modalInput.type("Bronze Ingot");
        await new Promise((r) => setTimeout(r, 1500));
        // Look for any button containing "Add" or "Ingredient" text in the modal.
        const added =
          (await clickByText(ownerPage, "button", "Add ingredients")) ||
          (await clickByText(ownerPage, "button", "Add Ingredients")) ||
          (await clickByText(ownerPage, "button", "Add to list"));
        if (!added) {
          fail(failures, "recipe add button not found");
        } else {
          await new Promise((r) => setTimeout(r, 2000));
          await ownerPage.keyboard.press("Escape");
          await new Promise((r) => setTimeout(r, 500));
          const apiRes = await api(ownerPage, "GET", `/api/v1/list/${listId}/listings`);
          const itemsLen = apiRes.body && apiRes.body[1] ? apiRes.body[1].length : 0;
          if (itemsLen <= 1) {
            fail(failures, `expected recipe to add rows, api items=${itemsLen}`);
          } else {
            pass(`added recipe via UI (api items=${itemsLen})`);
          }
        }
      } catch (e) {
        fail(failures, `recipe modal interaction failed: ${e.message || e}`);
      }
    }

    // ===== Step 3: Mark an item acquired via the row toggle =====
    console.log("[step] owner marks an item acquired");
    const markBtn = await ownerPage.$('button[aria-label="Mark acquired"]');
    if (!markBtn) {
      fail(failures, "Mark acquired button not found");
    } else {
      await markBtn.click();
      await new Promise((r) => setTimeout(r, 1500));
      const unmarkBtn = await ownerPage.$('button[aria-label="Mark unacquired"]');
      if (!unmarkBtn) {
        fail(failures, "after toggle, expected Mark unacquired aria-label");
      } else {
        pass("marked item acquired (toggle works)");
      }
      const bodyText = await ownerPage.evaluate(() => document.body.innerText);
      if (!/units acquired/.test(bodyText)) {
        fail(failures, "header progress string 'units acquired' not visible");
      } else {
        pass("header units-acquired summary visible");
      }
    }

    // ===== Step 4: Settings drawer — rename + invite =====
    console.log("[step] owner opens settings drawer");
    const settingsBtn = await ownerPage.$('[data-testid="list-settings-btn"]');
    if (!settingsBtn) {
      fail(failures, "Settings button not found");
    } else {
      await settingsBtn.click();
      await waitFor(ownerPage, '[data-testid="list-settings-drawer"]', 5000);
      pass("settings drawer opened");

      // Rename via drawer.
      const newName = `${name} (renamed)`;
      const renameInput = await ownerPage.$('[data-testid="drawer-rename-input"]');
      if (!renameInput) {
        fail(failures, "drawer rename input not found");
      } else {
        await renameInput.click({ clickCount: 3 });
        await renameInput.type(newName);
        const saveBtn = await ownerPage.$('[data-testid="drawer-save-details"]');
        if (!saveBtn) {
          fail(failures, "drawer save button not found");
        } else {
          await saveBtn.click();
          await new Promise((r) => setTimeout(r, 2000));
          const heading = await ownerPage.$eval("h1", (h) => h.textContent || "");
          if (!heading.includes("(renamed)")) {
            fail(failures, `expected heading to include '(renamed)', got '${heading}'`);
          } else {
            pass("renamed list via drawer");
          }
        }
      }

      // Create an invite via the drawer's share section.
      const inviteSection = await ownerPage.$('[data-testid="list-settings-sharing"]');
      if (!inviteSection) {
        fail(failures, "drawer sharing section not found");
      } else {
        // Find the "Copy" button inside the sharing section (creates invite + copies URL).
        const inviteBtnHandle = await ownerPage.evaluateHandle((section) => {
          const norm = (s) => (s || "").replace(/\s+/g, " ").trim();
          const buttons = Array.from(section.querySelectorAll("button"));
          return buttons.find((b) => /copy/i.test(norm(b.innerText))) || null;
        }, inviteSection);
        const inviteBtn = inviteBtnHandle.asElement();
        if (!inviteBtn) {
          fail(failures, "drawer invite-create button not found");
        } else {
          await inviteBtn.click();
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
            pass(`created invite via drawer (${invitesResp.body.length} invite(s))`);

            // ===== Step 5: Reader redeems the invite =====
            const inviteId = invitesResp.body[invitesResp.body.length - 1].id;
            const redeem = await api(readerPage, "POST", `/api/v1/invite/${inviteId}/use`);
            if (redeem.status !== 200 || redeem.body !== listId) {
              fail(
                failures,
                `invite redeem expected 200 + listId ${listId}, got ${redeem.status} body=${redeem.body}`,
              );
            } else {
              pass("reader redeemed invite");

              await readerPage.goto(`${BASE_URL}/list/${listId}`, {
                waitUntil: "domcontentloaded",
              });
              await readerPage.waitForFunction(() => !!document.querySelector("h1"), {
                timeout: TIMEOUT_MS,
              });
              await new Promise((r) => setTimeout(r, 1500));
              const visibleControls = await readerPage.evaluate(() => {
                const text = document.body.innerText;
                return {
                  hasAddItem: text.includes("Add Item"),
                  hasSettings: !!document.querySelector('[data-testid="list-settings-btn"]'),
                  hasNotify: text.includes("Notify"),
                };
              });
              if (visibleControls.hasAddItem) {
                fail(failures, "read-only viewer should NOT see Add Item");
              } else {
                pass("read-only viewer hides Add Item");
              }
              if (!visibleControls.hasSettings) {
                fail(failures, "read-only viewer should see Settings (for Leave option)");
              } else {
                pass("read-only viewer sees Settings button");
              }
              if (!visibleControls.hasNotify) {
                fail(failures, "read-only viewer should still see Notify");
              } else {
                pass("read-only viewer sees Notify");
              }
            }
          }
          await inviteBtnHandle.dispose();
        }
      }

      await ownerPage.keyboard.press("Escape");
      await new Promise((r) => setTimeout(r, 500));
    }

    // ===== Step 6: Delete the list =====
    console.log("[step] owner deletes the list");
    const settingsBtn2 = await ownerPage.$('[data-testid="list-settings-btn"]');
    if (settingsBtn2) {
      await settingsBtn2.click();
      await waitFor(ownerPage, '[data-testid="list-settings-drawer"]', 5000);
      const deleteBtn = await ownerPage.$('[data-testid="list-delete-btn"]');
      if (!deleteBtn) {
        fail(failures, "delete button not found");
      } else {
        await deleteBtn.click(); // first click: confirm prompt
        await new Promise((r) => setTimeout(r, 500));
        const deleteBtn2 = await ownerPage.$('[data-testid="list-delete-btn"]');
        if (!deleteBtn2) {
          fail(failures, "delete confirm button not found");
        } else {
          await deleteBtn2.click();
          await ownerPage
            .waitForFunction(() => window.location.pathname === "/list", { timeout: 5000 })
            .catch(() => {});
          if (ownerPage.url().endsWith("/list")) {
            pass("owner returned to /list after delete");
          } else {
            fail(failures, `expected url to end with /list, got ${ownerPage.url()}`);
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
