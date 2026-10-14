#!/usr/bin/env node
/* eslint-disable no-console */
/**
 * Shared-list E2E. Requires the server to be built with `--features test-auth`.
 */

"use strict";

const USERS = {
  owner: { id: 880000000001, username: "SharedListOwner" },
  reader: { id: 880000000002, username: "SharedListReader" },
  invited: { id: 880000000003, username: "SharedListInvited" },
  manual: { id: 880000000004, username: "SharedListManual" },
  overLimit: { id: 880000000005, username: "SharedListOverLimit" },
  inviteWriter: { id: 880000000006, username: "SharedListInviteWriter" },
};

async function navigate(page, url) {
  await page.bringToFront();
  const response = await page.goto(url, { waitUntil: "domcontentloaded" });
  await page.waitForFunction(() => window.__sharedListHydrated);
  return response;
}

async function login(page, baseUrl, user) {
  const loginUrl = new URL("/test/login", baseUrl);
  loginUrl.searchParams.set("user_id", String(user.id));
  loginUrl.searchParams.set("username", user.username);
  // Establish each session on a same-origin JSON page. Mount the editor only
  // when that user's UI is exercised, avoiding six concurrent WASM instances.
  loginUrl.searchParams.set("redirect", "/api/v1/world_data");
  const resp = await page.goto(loginUrl.toString(), { waitUntil: "domcontentloaded" });
  if (!resp || resp.status() >= 400) {
    throw new Error(`test login failed for ${user.username}: ${resp ? resp.status() : -1}`);
  }
}

async function api(page, method, path, body) {
  return page.evaluate(
    async ({ method, path, body }) => {
      const resp = await fetch(path, {
        method,
        credentials: "include",
        headers: body === undefined ? {} : { "Content-Type": "application/json" },
        body: body === undefined ? undefined : JSON.stringify(body),
      });
      const text = await resp.text();
      let parsed = null;
      try {
        parsed = text ? JSON.parse(text) : null;
      } catch (_) {
        parsed = text;
      }
      return { status: resp.status, body: parsed, text };
    },
    { method, path, body },
  );
}

function failIf(condition, failures, message) {
  if (condition) failures.push(message);
}

async function addItem(page, listId, itemId) {
  return api(page, "POST", `/api/v1/list/${listId}/add/item`, {
    id: 0,
    item_id: itemId,
    list_id: listId,
    hq: null,
    quantity: 1,
    acquired: null,
  });
}

async function getListEntry(page, listId) {
  const lists = await api(page, "GET", "/api/v1/list");
  return Array.isArray(lists.body) ? lists.body.find((candidate) => candidate.list.id === listId) : null;
}

async function listControlState(page, timeout) {
  await page.bringToFront();
  await page.waitForSelector('[data-testid="list-settings-btn"]', { timeout }).catch(async error => {
    console.error("List controls missing:", page.url(), await page.evaluate(() => document.body.innerText));
    throw error;
  });
  await new Promise((resolve) => setTimeout(resolve, 1500));
  return page.evaluate(() => {
    return {
      hasAddItem: !!document.querySelector('[data-testid="inline-list-add"]'),
      hasSettings: !!document.querySelector('[data-testid="list-settings-btn"]'),
    };
  });
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

  try {
    const ownerCtx = await browser.createBrowserContext();
    const readerCtx = await browser.createBrowserContext();
    const invitedCtx = await browser.createBrowserContext();
    const manualCtx = await browser.createBrowserContext();
    const overLimitCtx = await browser.createBrowserContext();
    const inviteWriterCtx = await browser.createBrowserContext();

    const ownerPage = await ownerCtx.newPage();
    const readerPage = await readerCtx.newPage();
    const invitedPage = await invitedCtx.newPage();
    const manualPage = await manualCtx.newPage();
    const overLimitPage = await overLimitCtx.newPage();
    const inviteWriterPage = await inviteWriterCtx.newPage();

    for (const page of [ownerPage, readerPage, invitedPage, manualPage, overLimitPage, inviteWriterPage]) {
      page.setDefaultTimeout(TIMEOUT_MS);
      page.setDefaultNavigationTimeout(Math.max(TIMEOUT_MS, 60000));
      page.on("dialog", dialog => dialog.type() === "beforeunload" ? dialog.accept() : dialog.dismiss());
      await page.setCookie({ name: "HIDE_ADS", value: "true", url: BASE_URL, path: "/" });
      await page.evaluateOnNewDocument(() => {
        window.__sharedListHydrated = false;
        window.addEventListener("ultros:hydrated", () => { window.__sharedListHydrated = true; });
      });
      page.on("pageerror", error => console.error("Shared-list browser error:", error));
    }

    await login(ownerPage, BASE_URL, USERS.owner);
    await login(readerPage, BASE_URL, USERS.reader);
    await login(invitedPage, BASE_URL, USERS.invited);
    await login(manualPage, BASE_URL, USERS.manual);
    await login(overLimitPage, BASE_URL, USERS.overLimit);
    await login(inviteWriterPage, BASE_URL, USERS.inviteWriter);

    const worldData = await api(ownerPage, "GET", "/api/v1/world_data");
    failIf(worldData.status !== 200, failures, `world_data expected 200, got ${worldData.status}`);
    const worldId = worldData.body.regions[0].datacenters[0].worlds[0].id;
    const name = `Shared E2E ${Date.now()}`;

    const create = await api(ownerPage, "POST", "/api/v1/list/create", {
      name,
      wdr_filter: { World: worldId },
    });
    failIf(create.status !== 200, failures, `create list expected 200, got ${create.status}`);

    const ownerLists = await api(ownerPage, "GET", "/api/v1/list");
    const created = ownerLists.body.find((entry) => entry.list.name === name);
    failIf(!created, failures, "created list not returned to owner");
    if (!created) throw new Error("cannot continue without created list");
    failIf(created.permission !== "Owner", failures, `owner permission was ${created.permission}`);
    const listId = created.list.id;

    const readShare = await api(ownerPage, "POST", `/api/v1/list/${listId}/share/user`, {
      user_id: USERS.reader.id,
      permission: "Read",
    });
    failIf(readShare.status !== 200, failures, `read share expected 200, got ${readShare.status}`);

    const readerList = await getListEntry(readerPage, listId);
    failIf(!readerList, failures, "read-shared list not returned to reader");
    failIf(readerList && readerList.permission !== "Read", failures, `reader permission was ${readerList && readerList.permission}`);

    const readAdd = await addItem(readerPage, listId, 2);
    failIf(readAdd.status !== 403, failures, `read-only add expected 403, got ${readAdd.status}`);

    const writeShare = await api(ownerPage, "POST", `/api/v1/list/${listId}/share/user`, {
      user_id: USERS.reader.id,
      permission: "Write",
    });
    failIf(writeShare.status !== 200, failures, `write share expected 200, got ${writeShare.status}`);

    const writeAdd = await addItem(readerPage, listId, 2);
    failIf(writeAdd.status !== 200, failures, `write add expected 200, got ${writeAdd.status}`);

    const invite = await api(ownerPage, "POST", `/api/v1/list/${listId}/invite/create`, {
      permission: "Read",
      max_uses: 2,
    });
    failIf(invite.status !== 200, failures, `create invite expected 200, got ${invite.status}`);
    const inviteId = invite.body.id;
    failIf(!inviteId || inviteId.length < 32, failures, "invite id was missing or too short");

    // Manual invite redemption via UI
    console.log(`[step] manual user redeems invite via code input: ${inviteId}`);
    await navigate(manualPage, `${BASE_URL}/list`);
    // The invite-code input lives inside the "Redeem invite" modal since the
    // lists-index redesign — open it before typing.
    await manualPage.waitForSelector('[data-testid="list-join-open"]', { timeout: 15000 });
    await manualPage.click('[data-testid="list-join-open"]');
    await manualPage.waitForSelector('[data-testid="list-join-code"]', { timeout: 10000 });
    await manualPage.type('[data-testid="list-join-code"]', inviteId);
    await manualPage.click('[data-testid="list-join-submit"]');
    // Manual redemption closes the modal and refreshes the list index.
    // Assert the newly accessible list instead of expecting direct-link navigation.
    await manualPage.waitForFunction(
      (expected) => Array.from(document.querySelectorAll('a[href]')).some(
        a => new URL(a.href).pathname === expected,
      ),
      { timeout: 10000 },
      `/list/${listId}`,
    );

    const manualList = await getListEntry(manualPage, listId);
    failIf(!manualList || manualList.permission !== "Read", failures, "manual redemption did not grant Read access");
    await manualPage.goto(`${BASE_URL}/api/v1/world_data`);

    // UI-level invite redemption (direct link)
    console.log(`[step] invited user redeems invite via direct link UI: /list/invite/${inviteId}`);
    await navigate(invitedPage, `${BASE_URL}/list/invite/${inviteId}`);
    await invitedPage.waitForFunction(
      (expected) => window.location.pathname === expected,
      { timeout: 10000 },
      `/list/${listId}`,
    ).catch(() => {});

    const invitedUrl = invitedPage.url();
    failIf(new URL(invitedUrl).pathname !== `/list/${listId}`, failures, `invited user expected redirect to /list/${listId}, got ${invitedUrl}`);

    const readInviteList = await getListEntry(invitedPage, listId);
    failIf(!readInviteList, failures, "read-invited list not returned to invited user");
    failIf(
      readInviteList && readInviteList.permission !== "Read",
      failures,
      `read invite permission was ${readInviteList && readInviteList.permission}`,
    );

    const readInviteControls = await listControlState(invitedPage, TIMEOUT_MS);
    failIf(readInviteControls.hasAddItem, failures, "read-invited user should not see Add Item");
    failIf(!readInviteControls.hasSettings, failures, "read-invited user should see Settings button");

    const readInviteAdd = await addItem(invitedPage, listId, 3);
    failIf(readInviteAdd.status !== 403, failures, `read invite add expected 403, got ${readInviteAdd.status}`);
    await invitedPage.goto(`${BASE_URL}/api/v1/world_data`);

    // Exhausted invite path
    console.log("[step] over-limit user attempts to redeem exhausted invite via UI");
    await navigate(overLimitPage, `${BASE_URL}/list/invite/${inviteId}`);
    await overLimitPage.waitForSelector(".alert-error", { timeout: 10000 }).catch(() => {});
    const errorText = await overLimitPage.evaluate(() => {
      const el = document.querySelector(".alert-error");
      return el ? el.innerText : "";
    });
    failIf(!errorText.includes("Could not accept invite:"), failures, `over-limit user expected error message, got: "${errorText}"`);
    await overLimitPage.goto(`${BASE_URL}/api/v1/world_data`);

    // Write invite path
    console.log("[step] invite writer redeems write invite and adds an item");
    const writeInvite = await api(ownerPage, "POST", `/api/v1/list/${listId}/invite/create`, {
      permission: "Write",
      max_uses: 1,
    });
    failIf(writeInvite.status !== 200, failures, `create write invite expected 200, got ${writeInvite.status}`);
    const writeInviteId = writeInvite.body.id;
    failIf(!writeInviteId || writeInviteId.length < 32, failures, "write invite id was missing or too short");

    await navigate(inviteWriterPage, `${BASE_URL}/list/invite/${writeInviteId}`);
    await inviteWriterPage.waitForFunction(
      (expected) => window.location.pathname === expected,
      { timeout: 10000 },
      `/list/${listId}`,
    ).catch(() => {});

    const inviteWriterUrl = inviteWriterPage.url();
    failIf(
      new URL(inviteWriterUrl).pathname !== `/list/${listId}`,
      failures,
      `invite writer expected redirect to /list/${listId}, got ${inviteWriterUrl}`,
    );

    const writeInviteList = await getListEntry(inviteWriterPage, listId);
    failIf(!writeInviteList, failures, "write-invited list not returned to invite writer");
    failIf(
      writeInviteList && writeInviteList.permission !== "Write",
      failures,
      `write invite permission was ${writeInviteList && writeInviteList.permission}`,
    );

    await inviteWriterPage.waitForFunction(
      () => !!document.querySelector('[data-testid="inline-list-add"]'),
      { timeout: TIMEOUT_MS },
    );

    const writeInviteAdd = await addItem(inviteWriterPage, listId, 3);
    failIf(writeInviteAdd.status !== 200, failures, `write invite add expected 200, got ${writeInviteAdd.status}`);
    await inviteWriterPage.goto(`${BASE_URL}/api/v1/world_data`);

    // Invite deletion path
    console.log("[step] owner creates a disposable invite for deletion");
    const deleteInvite = await api(ownerPage, "POST", `/api/v1/list/${listId}/invite/create`, {
      permission: "Write",
      max_uses: 5,
    });
    failIf(deleteInvite.status !== 200, failures, `create second invite expected 200, got ${deleteInvite.status}`);
    const deleteInviteId = deleteInvite.body.id;

    console.log("[step] owner deletes invite via UI");
    await navigate(ownerPage, `${BASE_URL}/list/${listId}`);
    await ownerPage.waitForSelector('[data-testid="list-access-btn"]');
    await ownerPage.click('[data-testid="list-access-btn"]');
    await ownerPage.waitForSelector('[data-testid="list-invite-create"]', { timeout: 10000 });

    // Find the row containing our new invite ID (first 10 chars)
    const shortId = deleteInviteId.substring(0, 10);
    // The drawer appears before its sharing resource has loaded.
    await ownerPage.waitForFunction(
      id => Array.from(document.querySelectorAll('div.flex.items-center.gap-3.py-2'))
        .some(row => row.innerText.includes(`Link: ${id}`)),
      { timeout: TIMEOUT_MS },
      shortId,
    );
    const deleted = await ownerPage.evaluate((id) => {
      const rows = Array.from(document.querySelectorAll("div.flex.items-center.gap-3.py-2"));
      const targetRow = rows.find((row) => row.innerText.includes(`Link: ${id}`));
      if (targetRow) {
        const btn = targetRow.querySelector('button[aria-label^="Remove access for "]');
        if (btn) {
          btn.click();
          return true;
        }
      }
      return false;
    }, shortId);
    failIf(!deleted, failures, "could not find or click delete button for invite");

    // Verify it disappears without reload
    const disappeared = await ownerPage
      .waitForFunction((id) => !document.body.innerText.includes(`Link: ${id}`), { timeout: 10000 }, shortId)
      .then(() => true)
      .catch(() => false);
    failIf(!disappeared, failures, "invite link did not disappear from UI after deletion");

    console.log("[step] over-limit user attempts to redeem deleted invite via UI");
    await navigate(overLimitPage, `${BASE_URL}/list/invite/${deleteInviteId}`);
    await overLimitPage.waitForSelector(".alert-error", { timeout: 10000 }).catch(() => {});
    const deletedInviteError = await overLimitPage.evaluate(() => {
      const el = document.querySelector(".alert-error");
      return el ? el.innerText : "";
    });
    failIf(
      !deletedInviteError.includes("Could not accept invite:"),
      failures,
      `over-limit user expected error for deleted invite, got: "${deletedInviteError}"`,
    );

    // Leave list path
    console.log("[step] reader leaves the shared list via UI");
    await navigate(readerPage, `${BASE_URL}/list/${listId}`);
    await readerPage.click('[data-testid="list-settings-btn"]');
    await readerPage.waitForSelector('[data-testid="list-leave-btn"]', { timeout: 10000 });
    await readerPage.click('[data-testid="list-leave-btn"]');

    // Wait for redirect to /list
    await readerPage.waitForFunction(() => window.location.pathname === "/list", { timeout: 10000 });

    // Verify it's gone for the reader
    const readerListsAfterLeave = await api(readerPage, "GET", "/api/v1/list");
    const readerListAfterLeave = readerListsAfterLeave.body.find((entry) => entry.list.id === listId);
    failIf(readerListAfterLeave, failures, "list still present for reader after leaving");

    // Verify it's still there for the owner
    const ownerListsAfterLeave = await api(ownerPage, "GET", "/api/v1/list");
    const ownerListAfterLeave = ownerListsAfterLeave.body.find((entry) => entry.list.id === listId);
    failIf(!ownerListAfterLeave, failures, "list disappeared for owner after reader left");

    for (const page of [ownerPage, readerPage, invitedPage, overLimitPage, manualPage, inviteWriterPage]) {
      await page.close();
    }
  } finally {
    await browser.close();
  }

  if (failures.length) {
    console.error(`[fail] ${failures.length} shared-list assertion(s) failed:`);
    for (const f of failures) console.error(`  - ${f}`);
    process.exit(1);
  }
  console.log("[ok] shared-list flow passed");
}

main().catch((err) => {
  console.error("[error]", err && err.stack ? err.stack : err);
  process.exit(1);
});
