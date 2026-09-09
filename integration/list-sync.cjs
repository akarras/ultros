#!/usr/bin/env node
/* eslint-disable no-console */
/**
 * Lists local-first sync E2E (Labs `lists-sync`). Requires a server built
 * with `--features test-auth`.
 *
 * Scenario A (one list, sequential — the plan's steps 1-4 plus the REST and
 * legacy checks):
 *   1. Owner creates a list, adds two items, shares it with an editor.
 *   2. Both open it under Labs. The owner marks a row acquired; the editor
 *      sees it without a reload.
 *   3. The owner goes offline, un-marks it, comes back: everyone converges
 *      and the server agrees.
 *   4. Ctrl+Z on the owner reverts the last edit for everyone.
 *   5. A REST add from the owner's API client appears on both pages.
 *   6. A third session without Labs sees the same rows.
 *
 * Global Constraint 2 scenarios, each on its own fresh list:
 *   B. Permission revocation — the owner unshares the editor mid-session.
 *   C. List deletion — the owner deletes the list while the editor has it open.
 *   D. Account switch in one browser context — B has no share.
 *
 * Env: BASE_URL (default http://127.0.0.1:8080), HEADLESS ("false" to watch),
 * TIMEOUT_MS (default 30000).
 */

"use strict";

const USERS = {
  owner: { id: 990000000601, username: "ListSyncOwner" },
  editor: { id: 990000000602, username: "ListSyncEditor" },
  outsider: { id: 990000000603, username: "ListSyncOutsider" },
};

const DOC_PREFIX = "ultros.listdoc.v1.";

async function login(page, baseUrl, user, labs) {
  const url = new URL("/test/login", baseUrl);
  url.searchParams.set("user_id", String(user.id));
  url.searchParams.set("username", user.username);
  url.searchParams.set("redirect", "/list");
  const resp = await page.goto(url.toString(), { waitUntil: "domcontentloaded" });
  if (!resp || resp.status() >= 400) {
    throw new Error(`test login failed for ${user.username}: ${resp ? resp.status() : -1}`);
  }
  // The LABS cookie is server-visible, so SSR and hydration agree on the branch.
  if (labs) {
    await page.setCookie({ name: "LABS", value: "lists-sync", url: baseUrl, path: "/" });
  } else {
    await page.deleteCookie({ name: "LABS", url: baseUrl, path: "/" }).catch(() => {});
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

async function waitForHydration(page, timeout) {
  await page.waitForFunction(
    () => !!document.querySelector('[data-testid="list-settings-btn"]'),
    { timeout },
  );
}

// Counts of the two acquire toggles tell us the rows and their state.
async function rowState(page) {
  return page.evaluate(() => ({
    unacquired: document.querySelectorAll('button[aria-label="Mark as acquired"]').length,
    acquired: document.querySelectorAll('button[aria-label="Mark unacquired"]').length,
  }));
}

async function waitForState(page, predicate, timeout) {
  const started = Date.now();
  let last = null;
  while (Date.now() - started < timeout) {
    last = await rowState(page).catch(() => last);
    if (last && predicate(last)) return last;
    await new Promise((r) => setTimeout(r, 250));
  }
  return last || { unacquired: -1, acquired: -1 };
}

// The acquired column as the *server* has it, read as the owner.
async function serverAcquired(page, listId) {
  const res = await api(page, "GET", `/api/v1/list/${listId}/listings`);
  if (res.status !== 200 || !res.body || !res.body[1]) return null;
  return res.body[1].map(([item]) => item.acquired || 0);
}

// `ultros.listdoc.v1.{user_id}.{list_id}` — the browser's cached snapshot.
async function docKeys(page, userId, listId) {
  return page.evaluate(
    (prefix) => Object.keys(window.localStorage).filter((k) => k.startsWith(prefix)),
    `${DOC_PREFIX}${userId}.${listId}`,
  );
}

async function waitForDocKey(page, userId, listId, present, timeout) {
  const started = Date.now();
  let keys = [];
  while (Date.now() - started < timeout) {
    keys = await docKeys(page, userId, listId).catch(() => keys);
    if (keys.length > 0 === present) return keys;
    await new Promise((r) => setTimeout(r, 250));
  }
  return keys;
}

async function waitForLive(page, timeout) {
  return page
    .waitForFunction(
      () =>
        document.querySelector('[data-testid="realtime-status-indicator"]')?.dataset
          .status === "live",
      { timeout },
    )
    .then(() => true)
    .catch(() => false);
}

async function bodyText(page) {
  return page.evaluate(() => document.body.innerText || "");
}

async function waitForDenied(page, timeout) {
  const started = Date.now();
  let seen = "";
  while (Date.now() - started < timeout) {
    seen = await bodyText(page).catch(() => seen);
    // The Labs page renders `list_view_failed_to_get_items` for a denial.
    if (/Failed to get items/i.test(seen)) return true;
    // Or the write chrome is gone, which is the softer "no longer yours".
    if (!/Add Item/.test(seen) && !/Mark as acquired|Mark unacquired/.test(seen)) {
      const controls = await rowState(page).catch(() => ({ unacquired: 0, acquired: 0 }));
      if (controls.unacquired + controls.acquired === 0) return true;
    }
    await new Promise((r) => setTimeout(r, 250));
  }
  return false;
}

async function createList(page, worldId, label) {
  const name = `${label} ${Date.now()}-${Math.floor(Math.random() * 10000)}`;
  const create = await api(page, "POST", "/api/v1/list/create", {
    name,
    wdr_filter: { World: worldId },
  });
  if (create.status !== 200) throw new Error(`create list: ${create.status}`);
  const lists = await api(page, "GET", "/api/v1/list");
  const found = (lists.body || []).find((e) => e.list.name === name);
  if (!found) throw new Error("created list not returned to owner");
  return found.list.id;
}

async function addItem(page, listId, itemId, quantity) {
  return api(page, "POST", `/api/v1/list/${listId}/add/item`, {
    id: 0,
    item_id: itemId,
    list_id: listId,
    hq: null,
    quantity: quantity === undefined ? 1 : quantity,
    acquired: 0,
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
  const createdLists = [];
  let ownerPage = null;

  try {
    // Separate contexts so each user keeps its own cookie jar and localStorage.
    const ownerContext = await browser.createBrowserContext();
    const editorContext = await browser.createBrowserContext();
    const legacyContext = await browser.createBrowserContext();
    const switchContext = await browser.createBrowserContext();
    ownerPage = await ownerContext.newPage();
    const editorPage = await editorContext.newPage();
    const legacyPage = await legacyContext.newPage();
    const switchPage = await switchContext.newPage();
    for (const p of [ownerPage, editorPage, legacyPage, switchPage]) {
      p.setDefaultTimeout(TIMEOUT_MS);
      await p.setViewport({ width: 1280, height: 900, deviceScaleFactor: 1 });
    }

    console.log("[step] logins");
    await login(ownerPage, BASE_URL, USERS.owner, true);
    await login(editorPage, BASE_URL, USERS.editor, true);
    await login(legacyPage, BASE_URL, USERS.owner, false);
    pass("owner, editor and legacy sessions signed in");

    const worldData = await api(ownerPage, "GET", "/api/v1/world_data");
    if (worldData.status !== 200) throw new Error(`world_data: ${worldData.status}`);
    const worldId = worldData.body.regions[0].datacenters[0].worlds[0].id;

    // ================= Scenario A =================
    console.log("[scenario A] live convergence, offline, undo, REST, legacy");
    const listId = await createList(ownerPage, worldId, "ListSync E2E A");
    createdLists.push(listId);
    for (const itemId of [5, 6]) {
      const add = await addItem(ownerPage, listId, itemId);
      if (add.status !== 200) fail(failures, `A: add item ${itemId}: ${add.status}`);
    }
    const share = await api(ownerPage, "POST", `/api/v1/list/${listId}/share/user`, {
      user_id: USERS.editor.id,
      permission: "Write",
    });
    if (share.status !== 200) fail(failures, `A: share: ${share.status}`);
    else pass(`A: list ${listId} ready with two rows, shared Write with the editor`);

    const listUrl = new URL(`/list/${listId}`, BASE_URL).toString();
    await ownerPage.goto(listUrl, { waitUntil: "domcontentloaded" });
    await editorPage.goto(listUrl, { waitUntil: "domcontentloaded" });
    await waitForHydration(ownerPage, TIMEOUT_MS);
    await waitForHydration(editorPage, TIMEOUT_MS);
    for (const [label, page] of [["owner", ownerPage], ["editor", editorPage]]) {
      const sync = await page.evaluate(
        () => !!document.querySelector('[data-testid="list-view-sync"]'),
      );
      if (!sync) fail(failures, `A: ${label} is not on the Labs list page`);
    }
    const initial = await waitForState(ownerPage, (s) => s.unacquired === 2, TIMEOUT_MS);
    if (initial.unacquired !== 2) {
      fail(failures, `A: owner expected 2 rows, saw ${JSON.stringify(initial)}`);
    } else {
      pass("A: owner renders both rows from the document");
    }
    const live = await waitForLive(ownerPage, TIMEOUT_MS);
    if (!live) fail(failures, "A: owner never reached live status");
    else pass("A: owner is live");

    console.log("[step] owner marks a row acquired; editor sees it");
    await ownerPage.click('button[aria-label="Mark as acquired"]');
    const ownerAfter = await waitForState(ownerPage, (s) => s.acquired === 1, 10000);
    if (ownerAfter.acquired !== 1) {
      fail(failures, `A: owner's own edit did not render: ${JSON.stringify(ownerAfter)}`);
    } else {
      pass("A: owner's edit renders locally");
    }
    const editorAfter = await waitForState(editorPage, (s) => s.acquired === 1, 15000);
    if (editorAfter.acquired !== 1) {
      fail(failures, `A: editor did not converge: ${JSON.stringify(editorAfter)}`);
    } else {
      pass("A: editor converged without a reload");
    }

    console.log("[step] owner edits offline, then reconnects");
    // `ListUndo::MERGE_INTERVAL_MS` is 1000: two edits closer together than
    // that collapse into one undo step, and the Ctrl+Z below would then
    // revert both. Space them so the undo step under test is the offline
    // edit alone.
    await new Promise((r) => setTimeout(r, 1500));
    await ownerPage.setOfflineMode(true);
    await ownerPage.click('button[aria-label="Mark unacquired"]');
    const offline = await waitForState(ownerPage, (s) => s.acquired === 0, 10000);
    if (offline.acquired !== 0) {
      fail(failures, `A: offline edit did not apply locally: ${JSON.stringify(offline)}`);
    } else {
      pass("A: offline edit applied locally");
    }
    const editorStill = await rowState(editorPage);
    if (editorStill.acquired !== 1) {
      fail(failures, `A: editor changed while the owner was offline: ${JSON.stringify(editorStill)}`);
    } else {
      pass("A: editor unchanged while the owner was offline");
    }
    await ownerPage.setOfflineMode(false);
    const editorSynced = await waitForState(editorPage, (s) => s.acquired === 0, 20000);
    if (editorSynced.acquired !== 0) {
      fail(failures, `A: editor did not receive the offline edit: ${JSON.stringify(editorSynced)}`);
    } else {
      pass("A: offline edit converged after reconnect");
    }
    const server = await serverAcquired(ownerPage, listId);
    if (!server || server.some((a) => a !== 0)) {
      fail(failures, `A: server rows not all unacquired: ${JSON.stringify(server)}`);
    } else {
      pass("A: server agrees (all rows unacquired)");
    }

    console.log("[step] Ctrl+Z on the owner");
    // The undo must not race the reconnect's own resync: a snapshot import
    // landing on top of it would silently swallow the revert.
    if (!(await waitForLive(ownerPage, TIMEOUT_MS))) {
      fail(failures, "A: owner did not return to live after reconnect");
    } else {
      pass("A: owner is live again after reconnect");
    }
    // "live" is set when the handshake reply lands; the snapshot import that
    // follows it can still be in flight, and an undo issued into that window
    // is overwritten by the import. Let the resync settle first.
    await new Promise((r) => setTimeout(r, 3000));
    await ownerPage.keyboard.down("Control");
    await ownerPage.keyboard.press("KeyZ");
    await ownerPage.keyboard.up("Control");
    const undone = await waitForState(ownerPage, (s) => s.acquired === 1, 10000);
    if (undone.acquired !== 1) {
      fail(failures, `A: undo did not revert the last edit: ${JSON.stringify(undone)}`);
    } else {
      pass("A: undo reverted the owner's last edit");
    }
    const editorUndone = await waitForState(editorPage, (s) => s.acquired === 1, 15000);
    if (editorUndone.acquired !== 1) {
      fail(failures, `A: undo did not reach the editor: ${JSON.stringify(editorUndone)}`);
    } else {
      pass("A: undo reverted for everyone");
    }
    const serverUndone = await serverAcquired(ownerPage, listId);
    if (!serverUndone || !serverUndone.includes(1)) {
      fail(failures, `A: server did not record the undo: ${JSON.stringify(serverUndone)}`);
    } else {
      pass("A: server recorded the undo");
    }

    console.log("[step] a REST add appears on both pages");
    const restAdd = await addItem(ownerPage, listId, 7, 2);
    if (restAdd.status !== 200) fail(failures, `A: REST add: ${restAdd.status}`);
    const ownerThree = await waitForState(
      ownerPage,
      (s) => s.unacquired + s.acquired === 3,
      15000,
    );
    const editorThree = await waitForState(
      editorPage,
      (s) => s.unacquired + s.acquired === 3,
      15000,
    );
    if (ownerThree.unacquired + ownerThree.acquired !== 3) {
      fail(failures, `A: owner missed the REST add: ${JSON.stringify(ownerThree)}`);
    } else {
      pass("A: REST write reached the owner's document");
    }
    if (editorThree.unacquired + editorThree.acquired !== 3) {
      fail(failures, `A: editor missed the REST add: ${JSON.stringify(editorThree)}`);
    } else {
      pass("A: REST write reached the editor's document");
    }

    console.log("[step] the legacy page agrees");
    await legacyPage.goto(listUrl, { waitUntil: "domcontentloaded" });
    await waitForHydration(legacyPage, TIMEOUT_MS);
    const legacySync = await legacyPage.evaluate(
      () => !!document.querySelector('[data-testid="list-view-sync"]'),
    );
    if (legacySync) fail(failures, "A: legacy session unexpectedly got the Labs page");
    // Compare against what the server actually holds rather than a fixed
    // split, so a legitimate divergence is reported as such and an earlier
    // step's failure does not also mis-blame the legacy page.
    const serverNow = (await serverAcquired(ownerPage, listId)) || [];
    const wantAcquired = serverNow.filter((a) => a > 0).length;
    const wantUnacquired = serverNow.length - wantAcquired;
    const legacy = await waitForState(
      legacyPage,
      (s) => s.acquired === wantAcquired && s.unacquired === wantUnacquired,
      TIMEOUT_MS,
    );
    if (legacy.acquired !== wantAcquired || legacy.unacquired !== wantUnacquired) {
      fail(
        failures,
        `A: legacy page disagrees with the server: page=${JSON.stringify(legacy)} server=${JSON.stringify(serverNow)}`,
      );
    } else {
      pass(`A: legacy page shows the same rows as the server (${JSON.stringify(serverNow)})`);
    }

    // ================= Scenario B: permission revocation =================
    console.log("[scenario B] permission revocation while the editor is open");
    const listB = await createList(ownerPage, worldId, "ListSync E2E B");
    createdLists.push(listB);
    for (const itemId of [5, 6]) await addItem(ownerPage, listB, itemId);
    const shareB = await api(ownerPage, "POST", `/api/v1/list/${listB}/share/user`, {
      user_id: USERS.editor.id,
      permission: "Write",
    });
    if (shareB.status !== 200) fail(failures, `B: share: ${shareB.status}`);
    const urlB = new URL(`/list/${listB}`, BASE_URL).toString();
    await editorPage.goto(urlB, { waitUntil: "domcontentloaded" });
    await waitForHydration(editorPage, TIMEOUT_MS);
    const bReady = await waitForState(editorPage, (s) => s.unacquired === 2, TIMEOUT_MS);
    if (bReady.unacquired !== 2) {
      fail(failures, `B: editor did not load both rows: ${JSON.stringify(bReady)}`);
    }
    // An edit guarantees a persisted snapshot to look for (and later, to lose).
    await editorPage.click('button[aria-label="Mark as acquired"]');
    const bEdited = await waitForState(editorPage, (s) => s.acquired === 1, 10000);
    if (bEdited.acquired !== 1) {
      fail(failures, `B: editor's edit did not apply: ${JSON.stringify(bEdited)}`);
    } else {
      pass("B: editor can write while shared");
    }
    const bKeysBefore = await waitForDocKey(editorPage, USERS.editor.id, listB, true, 15000);
    if (bKeysBefore.length === 0) {
      fail(failures, `B: no snapshot key ${DOC_PREFIX}${USERS.editor.id}.${listB} before revocation`);
    } else {
      pass(`B: editor holds ${bKeysBefore[0]}`);
    }
    const bBeforeServer = await serverAcquired(ownerPage, listB);

    const unshare = await api(
      ownerPage,
      "DELETE",
      `/api/v1/list/${listB}/share/user/${USERS.editor.id}`,
    );
    if (unshare.status !== 200) fail(failures, `B: unshare: ${unshare.status}`);
    else pass("B: owner revoked the editor's share");

    // Requirement: within 10s, with no further interaction.
    const bDenied = await waitForDenied(editorPage, 10000);
    if (!bDenied) {
      fail(
        failures,
        `B: editor still shows write controls 10s after revocation (no interaction): ${JSON.stringify(await rowState(editorPage))}`,
      );
    } else {
      pass("B: editor's page dropped its write controls / showed the denied state");
    }
    const bKeysPassive = await waitForDocKey(editorPage, USERS.editor.id, listB, false, 10000);
    if (bKeysPassive.length > 0) {
      fail(
        failures,
        `B: snapshot survived revocation with no interaction: ${JSON.stringify(bKeysPassive)}`,
      );
    } else {
      pass("B: editor's cached snapshot is gone (no interaction)");
    }
    // A later edit attempt must not reach the server. Clicking a toggle that
    // is still on screen sends an update over the document socket; the server
    // rejecting it is the client's other chance to notice the revocation.
    const bClicked = await editorPage
      .evaluate(() => {
        const b = document.querySelector(
          'button[aria-label="Mark as acquired"], button[aria-label="Mark unacquired"]',
        );
        if (!b) return false;
        b.click();
        return true;
      })
      .catch(() => false);
    console.log(`  . B: post-revocation toggle click attempted=${bClicked}`);
    const bKeysAfterEdit = await waitForDocKey(editorPage, USERS.editor.id, listB, false, 10000);
    if (bKeysAfterEdit.length > 0) {
      fail(
        failures,
        `B: snapshot survived a rejected edit attempt: ${JSON.stringify(bKeysAfterEdit)}`,
      );
    } else {
      pass("B: cached snapshot dropped once the rejected edit came back");
    }
    const bRest = await addItem(editorPage, listB, 8);
    if (bRest.status !== 403) fail(failures, `B: revoked editor's REST add expected 403, got ${bRest.status}`);
    else pass("B: revoked editor's REST write is rejected (403)");
    await new Promise((r) => setTimeout(r, 5000));
    const bAfterServer = await serverAcquired(ownerPage, listB);
    if (JSON.stringify(bAfterServer) !== JSON.stringify(bBeforeServer)) {
      fail(
        failures,
        `B: server rows changed after revocation: before=${JSON.stringify(bBeforeServer)} after=${JSON.stringify(bAfterServer)}`,
      );
    } else {
      pass("B: no post-revocation edit reached the server");
    }

    // ================= Scenario C: list deletion =================
    console.log("[scenario C] owner deletes the list while the editor has it open");
    const listC = await createList(ownerPage, worldId, "ListSync E2E C");
    const shareC = await api(ownerPage, "POST", `/api/v1/list/${listC}/share/user`, {
      user_id: USERS.editor.id,
      permission: "Write",
    });
    if (shareC.status !== 200) fail(failures, `C: share: ${shareC.status}`);
    for (const itemId of [5, 6]) await addItem(ownerPage, listC, itemId);
    await editorPage.goto(new URL(`/list/${listC}`, BASE_URL).toString(), {
      waitUntil: "domcontentloaded",
    });
    await waitForHydration(editorPage, TIMEOUT_MS);
    const cReady = await waitForState(editorPage, (s) => s.unacquired === 2, TIMEOUT_MS);
    if (cReady.unacquired !== 2) {
      fail(failures, `C: editor did not load both rows: ${JSON.stringify(cReady)}`);
    }
    await editorPage.click('button[aria-label="Mark as acquired"]');
    await waitForState(editorPage, (s) => s.acquired === 1, 10000);
    const cKeysBefore = await waitForDocKey(editorPage, USERS.editor.id, listC, true, 15000);
    if (cKeysBefore.length === 0) fail(failures, "C: no snapshot key before deletion");
    else pass(`C: editor holds ${cKeysBefore[0]}`);

    const del = await api(ownerPage, "DELETE", `/api/v1/list/${listC}/delete`);
    if (del.status !== 200) fail(failures, `C: delete: ${del.status}`);
    else pass("C: owner deleted the list");
    // Requirement: within 10s, with no further interaction.
    const cDenied = await waitForDenied(editorPage, 10000);
    if (!cDenied) {
      fail(
        failures,
        `C: editor did not reach its error state within 10s (no interaction): ${JSON.stringify(await rowState(editorPage))}`,
      );
    } else {
      pass("C: editor reached the error state");
    }
    const cKeysPassive = await waitForDocKey(editorPage, USERS.editor.id, listC, false, 10000);
    if (cKeysPassive.length > 0) {
      fail(
        failures,
        `C: snapshot survived deletion with no interaction: ${JSON.stringify(cKeysPassive)}`,
      );
    } else {
      pass("C: editor's cached snapshot is gone (no interaction)");
    }
    const cClicked = await editorPage
      .evaluate(() => {
        const b = document.querySelector(
          'button[aria-label="Mark as acquired"], button[aria-label="Mark unacquired"]',
        );
        if (!b) return false;
        b.click();
        return true;
      })
      .catch(() => false);
    console.log(`  . C: post-deletion toggle click attempted=${cClicked}`);
    const cKeysAfterEdit = await waitForDocKey(editorPage, USERS.editor.id, listC, false, 10000);
    if (cKeysAfterEdit.length > 0) {
      fail(
        failures,
        `C: snapshot survived a rejected edit attempt: ${JSON.stringify(cKeysAfterEdit)}`,
      );
    } else {
      pass("C: cached snapshot dropped once the rejected edit came back");
    }

    // ================= Scenario D: account switch in one context =========
    console.log("[scenario D] account switch inside one browser context");
    const listD = await createList(ownerPage, worldId, "ListSync E2E D");
    createdLists.push(listD);
    for (const itemId of [5, 6]) await addItem(ownerPage, listD, itemId);
    const shareD = await api(ownerPage, "POST", `/api/v1/list/${listD}/share/user`, {
      user_id: USERS.editor.id,
      permission: "Write",
    });
    if (shareD.status !== 200) fail(failures, `D: share: ${shareD.status}`);
    // A = the editor (shared), B = the outsider (no share), same context.
    await login(switchPage, BASE_URL, USERS.editor, true);
    const urlD = new URL(`/list/${listD}`, BASE_URL).toString();
    await switchPage.goto(urlD, { waitUntil: "domcontentloaded" });
    await waitForHydration(switchPage, TIMEOUT_MS);
    const dReady = await waitForState(switchPage, (s) => s.unacquired === 2, TIMEOUT_MS);
    if (dReady.unacquired !== 2) {
      fail(failures, `D: user A did not load both rows: ${JSON.stringify(dReady)}`);
    }
    await switchPage.click('button[aria-label="Mark as acquired"]');
    await waitForState(switchPage, (s) => s.acquired === 1, 10000);
    const dKeysA = await waitForDocKey(switchPage, USERS.editor.id, listD, true, 15000);
    if (dKeysA.length === 0) fail(failures, "D: user A has no snapshot key before the switch");
    else pass(`D: user A holds ${dKeysA[0]}`);

    await login(switchPage, BASE_URL, USERS.outsider, true);
    await switchPage.goto(urlD, { waitUntil: "domcontentloaded" });
    const dDenied = await waitForDenied(switchPage, 15000);
    if (!dDenied) {
      fail(
        failures,
        `D: user B was not denied: ${JSON.stringify(await rowState(switchPage))} / ${(await bodyText(switchPage)).slice(0, 200)}`,
      );
    } else {
      pass("D: user B sees the denied/error state");
    }
    const dKeysB = await waitForDocKey(switchPage, USERS.outsider.id, listD, false, 10000);
    if (dKeysB.length > 0) {
      fail(failures, `D: user B cached a snapshot it may not have: ${JSON.stringify(dKeysB)}`);
    } else {
      pass("D: no snapshot key for user B");
    }
    const dKeysAStill = await docKeys(switchPage, USERS.editor.id, listD);
    if (dKeysAStill.length === 0) {
      fail(failures, "D: user A's cached snapshot was destroyed by B signing in");
    } else {
      pass("D: user A's cached snapshot survives the account switch");
    }
    console.log("[scenario E] delayed responses across client navigation");
    await require("./list-sync-navigation.cjs")({
      page: ownerPage, baseUrl: BASE_URL, userId: USERS.owner.id, worldId,
      createList, addItem, api, createdLists, waitForState, waitForDocKey,
      timeout: TIMEOUT_MS,
    });
  } catch (e) {
    fail(failures, `uncaught: ${e && e.stack ? e.stack : e}`);
  } finally {
    if (ownerPage) {
      for (const id of createdLists) {
        await api(ownerPage, "DELETE", `/api/v1/list/${id}/delete`).catch(() => {});
      }
    }
    await browser.close();
  }

  if (failures.length) {
    console.error(`\n[fail] ${failures.length} list-sync assertion(s) failed:`);
    for (const f of failures) console.error(`  - ${f}`);
    process.exit(1);
  }
  console.log("\n[ok] list-sync: all checks passed");
}

main().catch((e) => {
  console.error("[error]", e && e.stack ? e.stack : e);
  process.exit(1);
});
