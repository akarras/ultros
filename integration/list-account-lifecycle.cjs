"use strict";

const assert = require("node:assert/strict");
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));

// Real socket/session/permission transitions, including a surviving tab across
// another tab's login. The worker wrapper only controls connection availability.
module.exports = async function accountLifecycle({ ownerPage, editorPage, baseUrl,
  ownerUser, editorUser, worldId, api, createList, addItem, createdLists,
  login, waitForHydration, waitForLive, setAcquired, rowState, timeout }) {
  const privateEditorLists = [];
  let switchTab;
  const preload = await editorPage.evaluateOnNewDocument(() => {
    const Native = window.WebSocket;
    const state = window.__listLifecycle = {
      offline: sessionStorage.getItem("lifecycleHoldSocket") === "1", sockets: [], errors: [], delivered: [],
      documentId: crypto.randomUUID(),
    };
    window.WebSocket = class extends Native {
      constructor(...args) {
        if (state.offline) throw new Error("Lifecycle fixture is offline");
        super(...args);
        state.sockets.push(this);
        this.addEventListener("message", event => {
          const message = JSON.parse(event.data);
          const error = message.SubscriptionEvent?.event?.Error || message.Error;
          if (error) state.errors.push(error.message);
          if (message.ListDocSubscribed) state.delivered.push(message.ListDocSubscribed.list_id);
        });
      }
    };
    const createObjectURL = URL.createObjectURL;
    URL.createObjectURL = function(blob) {
      blob.text().then(text => {
        try { state.download = JSON.parse(text); } catch {}
      });
      return createObjectURL.call(this, blob);
    };
  });
  const key = (user, id) => `ultros.listdoc.v1.${user.id}.${id}`;
  const pauseKey = id => `ultros.listdoc.write-paused.v1.${editorUser.id}.${id}`;
  async function ownedList(label, share = true) {
    const id = await createList(ownerPage, worldId, label);
    createdLists.push(id);
    assert.equal((await addItem(ownerPage, id, 5056, 3)).status, 200);
    if (share) assert.equal((await api(ownerPage, "POST", `/api/v1/list/${id}/share/user`, {
      user_id: editorUser.id, permission: "Write",
    })).status, 200);
    return id;
  }
  async function open(id) {
    await editorPage.goto(`${baseUrl}/list/${id}`, { waitUntil: "domcontentloaded" });
    await editorPage.bringToFront();
    await waitForHydration(editorPage, timeout);
    assert(await waitForLive(editorPage, timeout), "fixture starts synchronized");
    await editorPage.waitForFunction(k => !!localStorage.getItem(k), { timeout }, key(editorUser, id));
  }
  async function offlineEdit(id) {
    const snapshotKey = key(editorUser, id);
    const before = await editorPage.evaluate(k => localStorage.getItem(k), snapshotKey);
    await editorPage.evaluate(() => {
      window.__listLifecycle.offline = true;
      window.__listLifecycle.sockets.forEach(socket => socket.close());
    });
    await editorPage.setOfflineMode(true);
    await setAcquired(editorPage, true);
    await editorPage.waitForFunction(({ k, before }) => {
      const value = localStorage.getItem(k);
      return !!value && value !== before;
    }, { timeout }, { k: snapshotKey, before });
    assert.equal((await rowState(editorPage)).acquired, 1, "offline intent is visible and durable");
    return snapshotKey;
  }
  async function reconnect() {
    await editorPage.setOfflineMode(false);
    await editorPage.bringToFront();
    await editorPage.evaluate(() => { window.__listLifecycle.offline = false; });
  }
  async function serverOwned(page, id, wanted) {
    const deadline = Date.now() + timeout;
    do {
      const result = await api(page, "GET", `/api/v1/list/${id}/listings`);
      assert.equal(result.status, 200);
      const values = result.body[1].map(([row]) => row.acquired || 0);
      if (values.length === 1 && values[0] === wanted) return;
      await sleep(100);
    } while (Date.now() < deadline);
    throw new Error(`List ${id}: server owned count never became ${wanted}`);
  }
  async function expectPaused(id, snapshotKey, expectedOwned = 0) {
    await editorPage.waitForFunction(k => localStorage.getItem(k) === "1", { timeout }, pauseKey(id));
    await editorPage.waitForSelector('[data-testid="list-recovery"]');
    assert(await editorPage.evaluate(k => !!localStorage.getItem(k), snapshotKey), "rejected work is retained");
    await editorPage.waitForFunction(() => document.querySelector('[data-testid="list-recovery"]')
      ?.innerText.includes("Editing is no longer allowed"), { timeout });
    await editorPage.waitForSelector('button[aria-label^="Details for "]', { timeout });
    await rowState(editorPage); // expose the compact cart's Owned input
    await editorPage.waitForFunction(wanted => {
      const inputs = [...document.querySelectorAll('input[aria-label^="Owned for "]')];
      return inputs.length === 1 && Number(inputs[0].value) === wanted;
    }, { timeout }, expectedOwned);
    const state = await rowState(editorPage);
    assert.equal(state.acquired, expectedOwned, "cart shows the server copy, or the saved branch while the socket is unavailable");
    assert.equal(state.unacquired, 1 - expectedOwned);
    assert(await editorPage.$('input[aria-label="Owned for Bronze Ingot"][readonly]'), "server copy is read-only");
    assert.equal(await editorPage.$('[data-testid="list-recovery-use-server"]'), null,
      "read-only recovery offers no accidental branch replacement");
  }
  let originalError;
  try {
    console.log("[lifecycle] pending offline work survives Write-to-Read and reload");
    await login(editorPage, baseUrl, editorUser);
    const downgraded = await ownedList("Lifecycle downgrade");
    await open(downgraded);
    const snapshotKey = await offlineEdit(downgraded);
    assert.equal((await api(ownerPage, "POST", `/api/v1/list/${downgraded}/share/user`, {
      user_id: editorUser.id, permission: "Read",
    })).status, 200);
    await reconnect();
    await expectPaused(downgraded, snapshotKey);
    await serverOwned(ownerPage, downgraded, 0);
    await editorPage.locator('[data-testid="list-recovery"] button').filter(button => button.textContent.includes("recovery")).click();
    await editorPage.waitForFunction(() => window.__listLifecycle.download?.format === "ultros-device-list", { timeout });
    assert(await editorPage.evaluate(() => window.__listLifecycle.download.snapshot.length > 0), "preserved branch exports");
    await editorPage.evaluate(() => sessionStorage.setItem("lifecycleHoldSocket", "1"));
    await editorPage.reload({ waitUntil: "domcontentloaded" });
    await waitForHydration(editorPage, timeout);
    await expectPaused(downgraded, snapshotKey, 1);
    await editorPage.evaluate(() => {
      sessionStorage.removeItem("lifecycleHoldSocket");
      window.__listLifecycle.offline = false;
    });
    // Constructor failures use the real socket's reconnect backoff, which
    // reaches 32 seconds. Await its handshake before checking the server copy.
    await editorPage.waitForFunction(id => window.__listLifecycle.delivered.includes(id),
      { timeout: Math.max(timeout, 40000) }, downgraded);
    await expectPaused(downgraded, snapshotKey);
    assert.equal((await api(ownerPage, "POST", `/api/v1/list/${downgraded}/share/user`, {
      user_id: editorUser.id, permission: "Write",
    })).status, 200);
    await editorPage.reload({ waitUntil: "domcontentloaded" });
    await waitForHydration(editorPage, timeout);
    await expectPaused(downgraded, snapshotKey);
    await editorPage.waitForSelector('[data-testid="list-recovery-retry"]', { visible: true, timeout });
    await serverOwned(ownerPage, downgraded, 0);
    assert(await editorPage.evaluate(k => localStorage.getItem(k) === "1", pauseKey(downgraded)),
      "restoring Write never silently sends rejected intent");
    await editorPage.click('[data-testid="list-recovery-retry"]');
    await serverOwned(ownerPage, downgraded, 1);
    console.log("  + retained, exportable across reload; explicit retry after regrant converges");

    console.log("[lifecycle] session lapse preserves offline work and resumes after login");
    const expired = await ownedList("Lifecycle expired session");
    await open(expired);
    const expiredKey = await offlineEdit(expired);
    const authCookies = (await editorPage.cookies()).filter(cookie => cookie.name === "discord_auth");
    assert.equal(authCookies.length, 1, "fixture has one auth cookie");
    await editorPage.deleteCookie(...authCookies);
    await reconnect();
    await editorPage.waitForFunction(() => window.__listLifecycle.errors.includes("sign in to edit lists"), { timeout });
    await sleep(600); // let the deferred denial callback and save finish
    assert(await editorPage.evaluate(k => !!localStorage.getItem(k), expiredKey), "lapsed session never purges intent");
    await serverOwned(ownerPage, expired, 0);
    await login(editorPage, baseUrl, editorUser);
    await open(expired);
    await serverOwned(ownerPage, expired, 1);
    console.log("  + anonymous reconnect keeps the saved branch; same-account login syncs it");

    console.log("[lifecycle] stale tab cannot mix accounts or purge the previous account");
    const privateOwner = await ownedList("Lifecycle private current account", false);
    const privateEditor = await createList(editorPage, worldId, "Lifecycle private previous account");
    privateEditorLists.push(privateEditor);
    assert.equal((await addItem(editorPage, privateEditor, 5056, 3)).status, 200);
    assert.equal((await api(ownerPage, "GET", `/api/v1/list/${privateEditor}/listings`)).status, 403);
    await open(privateEditor);
    const retainedKey = await offlineEdit(privateEditor);
    const documentId = await editorPage.evaluate(() => window.__listLifecycle.documentId);
    switchTab = await editorPage.browserContext().newPage();
    const loginUrl = new URL("/test/login", baseUrl);
    loginUrl.searchParams.set("user_id", ownerUser.id);
    loginUrl.searchParams.set("username", ownerUser.username);
    loginUrl.searchParams.set("redirect", "/api/v1/current_user");
    assert.equal((await switchTab.goto(loginUrl.toString(), { waitUntil: "domcontentloaded" })).status(), 200);
    await reconnect();
    await editorPage.waitForSelector('[data-testid="list-account-changed"]', { timeout });
    assert(await editorPage.evaluate(k => !!localStorage.getItem(k), retainedKey), "new account denial cannot purge the old account");
    await editorPage.evaluate(id => {
      const a = document.createElement("a"); a.href = `/list/${id}`;
      document.body.append(a); a.click(); a.remove();
    }, privateOwner);
    await editorPage.waitForFunction(id => location.pathname === `/list/${id}`, { timeout }, privateOwner);
    assert.equal(await editorPage.evaluate(() => window.__listLifecycle.documentId), documentId, "account switch test retains the original document");
    assert.equal(await editorPage.evaluate(k => localStorage.getItem(k), key(editorUser, privateOwner)), null,
      "current account contents never enter the stale account namespace");
    assert(await editorPage.evaluate(k => !!localStorage.getItem(k), retainedKey));
    // Mount a fresh list component from a non-list route while B's socket and
    // cookie coexist with A's stale page bootstrap. A prior latched warning
    // cannot trivially prevent this new component from opening a document.
    const navigate = route => editorPage.evaluate(route => {
      const a = document.createElement("a"); a.href = route;
      document.body.append(a); a.click(); a.remove();
    }, route);
    await navigate("/");
    await editorPage.waitForFunction(() => location.pathname === "/", { timeout });
    assert.equal(await editorPage.$('[data-testid="list-account-changed"]'), null);
    await navigate(`/list/${privateOwner}`);
    await editorPage.waitForSelector('[data-testid="list-account-changed"]', { timeout });
    assert.equal(await editorPage.evaluate(() => window.__listLifecycle.documentId), documentId);
    await editorPage.evaluate(user => navigator.locks.request(`ultros-account-lists:${user}`, () => {}), editorUser.id);
    assert.deepEqual(await editorPage.evaluate(({ user, id }) => ({
      snapshot: localStorage.getItem(`ultros.listdoc.v1.${user}.${id}`),
      permission: JSON.parse(localStorage.getItem(`ultros.listdoc.index.v1.${user}`) || '{"lists":{}}').lists[id] ?? null,
    }), { user: editorUser.id, id: privateOwner }), { snapshot: null, permission: null },
    "a fresh stale-identity mount writes neither document contents nor permission metadata under A");
    assert(await editorPage.evaluate(k => !!localStorage.getItem(k), retainedKey));
    await editorPage.click('[data-testid="list-account-reload"]');
    await waitForHydration(editorPage, timeout);
    await editorPage.waitForFunction(k => !!localStorage.getItem(k), { timeout }, key(ownerUser, privateOwner));
    assert.equal(await editorPage.evaluate(k => localStorage.getItem(k), key(editorUser, privateOwner)), null);
    assert(await editorPage.evaluate(k => !!localStorage.getItem(k), retainedKey), "reload keeps the other account's work");
    await login(editorPage, baseUrl, editorUser);
    await open(privateEditor);
    await serverOwned(editorPage, privateEditor, 1);
    console.log("  + no crossed cache key; former account's offline work survives and later syncs");
  } catch (error) { originalError = error; }
  const cleanupErrors = [];
  for (const step of [
    () => editorPage.setOfflineMode(false),
    async () => { if (switchTab) await switchTab.close(); },
    () => login(editorPage, baseUrl, editorUser),
    () => require("./list-fixture-cleanup.cjs").cleanupOwnedLists(
      (method, route, body) => api(editorPage, method, route, body), privateEditorLists),
    () => editorPage.removeScriptToEvaluateOnNewDocument(preload.identifier),
  ]) try { await step(); } catch (error) { cleanupErrors.push(error); }
  if (cleanupErrors.length) throw new AggregateError([...(originalError ? [originalError] : []), ...cleanupErrors], "Lifecycle cleanup failed");
  if (originalError) throw originalError;
};
