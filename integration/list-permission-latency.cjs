"use strict";

// Uses real shared-list broadcasts; only error responses are injected. The
// native clock test asserts the exact 1000ms scheduling bound. Browser checks
// allow 1000ms of local scheduling/transport tolerance, not arbitrary debouncing.
const assert = require("node:assert/strict");
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));

module.exports = async function permissionLatency({ ownerPage, editorPage, baseUrl,
  editorId, worldId, createList, addItem, api, createdLists, waitForHydration,
  waitForLive, waitForDocKey, login, editorUser, timeout }) {
  const id = await createList(ownerPage, worldId, "Permission latency");
  createdLists.push(id);
  assert.equal((await addItem(ownerPage, id, 5056)).status, 200);
  assert.equal((await api(ownerPage, "POST", `/api/v1/list/${id}/share/user`, {
    user_id: editorId, permission: "Write",
  })).status, 200);
  const preload = await editorPage.evaluateOnNewDocument(() => {
    const NativeSocket = window.WebSocket;
    window.WebSocket = class extends NativeSocket {
      constructor(...args) {
        super(...args);
        this.addEventListener("message", event => {
          const probe = window.__permissionLatency;
          if (!probe?.tracking) return;
          const message = JSON.parse(event.data);
          if (message.SubscriptionEvent?.event?.ListUpdate) {
            probe.events.push(performance.now());
          }
        });
      }
    };
    Object.defineProperty(window, "documentPictureInPicture", {
      value: undefined, configurable: true,
    });
  });
  let stop = false;
  let broadcastLoop;
  let broadcastError;
  let popup;
  const errors = [];
  const onError = error => errors.push(error.stack || String(error));
  editorPage.on("pageerror", onError);
  try {
    await editorPage.goto(`${baseUrl}/list/${id}`, { waitUntil: "domcontentloaded" });
    await waitForHydration(editorPage, timeout);
    await editorPage.waitForSelector('input[aria-label="Needed for Bronze Ingot"]');
    assert(await waitForLive(editorPage, timeout));
    await waitForDocKey(editorPage, editorId, id, true, timeout);
    const rows = await api(ownerPage, "GET", `/api/v1/list/${id}/listings`);
    const row = rows.body[1][0][0];
    await editorPage.evaluate(id => {
      const original = window.fetch;
      const probe = window.__permissionLatency = {
        tracking: true, events: [], requests: [], outcome: "ok",
      };
      window.fetch = async function(input, init) {
        const url = new URL(typeof input === "string" ? input : input.url, location.href);
        if (url.pathname === `/api/v1/list/${id}/listings`) {
          probe.requests.push(performance.now());
          if (probe.outcome !== "ok") {
            const expired = probe.outcome === "expired";
            return new Response(JSON.stringify({ ApiError: expired
              ? "NotAuthenticated" : { Message: "Injected temporary outage" } }), {
              status: expired ? 401 : 503,
              headers: { "Content-Type": "application/json" },
            });
          }
        }
        return original.call(this, input, init);
      };
      probe.restore = () => { window.fetch = original; };
    }, id);

    // Repeat edits to an already covered item, so load_view has no new item or
    // scope to fetch. These requests must be the silent permission probe.
    broadcastLoop = (async () => {
      let n = 0;
      while (!stop) {
        const response = await api(ownerPage, "POST", "/api/v1/list/item/edit", {
          ...row, quantity: 3 + (n++ % 2),
        });
        assert.equal(response.status, 200, "owner broadcast edit succeeds");
        await sleep(100);
      }
    })().catch(error => { broadcastError = error; stop = true; });
    await editorPage.waitForFunction(() => window.__permissionLatency.events.length >= 1);
    await editorPage.waitForFunction(() => window.__permissionLatency.requests.length >= 3,
      { timeout: 5000 });
    const timing = await editorPage.evaluate(() => ({
      events: window.__permissionLatency.events,
      requests: window.__permissionLatency.requests,
    }));
    assert(timing.events.length >= 12, "continuous real broadcasts reached the client");
    assert(timing.requests[0] - timing.events[0] <= 2000,
      `first check took ${timing.requests[0] - timing.events[0]}ms`);
    for (let i = 1; i < timing.requests.length; i++) {
      const gap = timing.requests[i] - timing.requests[i - 1];
      assert(gap >= 700 && gap <= 2200, `bounded/coalesced probe interval ${gap}ms`);
    }
    console.log(`  . first probe ${Math.round(timing.requests[0] - timing.events[0])}ms; `
      + `${timing.requests.length} probes during ${timing.events.length} real broadcasts`);
    console.log("  + continuous 100ms broadcasts still check access every one-second window");

    for (const outcome of ["outage", "expired"]) {
      const before = await editorPage.evaluate(outcome => {
        window.__permissionLatency.outcome = outcome;
        return window.__permissionLatency.requests.length;
      }, outcome);
      await editorPage.waitForFunction(before =>
        window.__permissionLatency.requests.length >= before + 2, { timeout: 4000 }, before);
      assert((await waitForDocKey(editorPage, editorId, id, true, 1000)).length > 0,
        `${outcome} preserves the cached document`);
      assert(await editorPage.$('input[aria-label="Needed for Bronze Ingot"]:not([readonly])'),
        `${outcome} does not treat a transient/expired session as revoked access`);
    }
    await editorPage.evaluate(() => { window.__permissionLatency.outcome = "ok"; });

    // Open the actual companion before the downgrade, including with no market
    // prices. It must retire its editable controls when read-only arrives.
    await editorPage.click('[data-testid="guest-shop-mode"]');
    await editorPage.waitForSelector('[data-testid="shop-cheapest"]', { visible: true });
    await editorPage.click('[data-testid="shop-cheapest"]');
    await editorPage.waitForSelector('[data-testid="open-shopping-companion"]', { visible: true });
    const opened = new Promise((resolve, reject) => {
      const timer = setTimeout(() => reject(new Error("Permission probe companion did not open")), timeout);
      editorPage.once("popup", popup => { clearTimeout(timer); resolve(popup); });
    });
    await editorPage.click('[data-testid="open-shopping-companion"]');
    popup = await opened;
    await popup.waitForSelector("main h1");
    await editorPage.bringToFront();
    const changedAt = Date.now();
    assert.equal((await api(ownerPage, "POST", `/api/v1/list/${id}/share/user`, {
      user_id: editorId, permission: "Read",
    })).status, 200);
    await editorPage.waitForFunction(() => {
      const needed = document.querySelector('input[aria-label="Needed for Bronze Ingot"]');
      return needed?.readOnly;
    }, { timeout: 2500 });
    assert(Date.now() - changedAt <= 2500, "write-to-read UI follows access within the normal budget");
    console.log(`  . write-to-read controls updated in ${Date.now() - changedAt}ms`);
    await Promise.race([
      popup.isClosed() ? Promise.resolve() : new Promise(resolve => popup.once("close", resolve)),
      sleep(2500).then(() => { assert(popup.isClosed(), "editable companion closes after downgrade"); }),
    ]);
    assert((await waitForDocKey(editorPage, editorId, id, true, 1000)).length > 0,
      "read-only access retains the readable document");
    console.log("  + downgrade retires editing and companion during sustained updates; errors retain data");

    // Full sign-out tears down the route while the continuous stream can have
    // another permission timer pending. It must retain this user's snapshot.
    await editorPage.goto(`${baseUrl}/logout`, { waitUntil: "domcontentloaded" });
    assert((await waitForDocKey(editorPage, editorId, id, true, 1000)).length > 0,
      "explicit sign-out retains the previous account's cached document");
    const signedOut = await api(editorPage, "GET", `/api/v1/list/${id}/listings`);
    assert.equal(signedOut.status, 401, "signed-out browser cannot read the account list");
    await login(editorPage, baseUrl, editorUser, true);
    assert.deepEqual(errors, [], "timer firing, access updates and teardown have no page errors");
    console.log("  + sign-out during broadcasts cancels the old page and retains account data");
  } finally {
    stop = true;
    if (broadcastLoop) await broadcastLoop;
    await editorPage.evaluate(() => window.__permissionLatency?.restore()).catch(() => {});
    await editorPage.removeScriptToEvaluateOnNewDocument(preload.identifier);
    editorPage.off("pageerror", onError);
    if (popup && !popup.isClosed()) await popup.close();
    if (broadcastError) throw broadcastError;
  }
};
