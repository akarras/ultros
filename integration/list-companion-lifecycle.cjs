"use strict";
const assert = require("node:assert/strict");
const { finishCleanup } = require("./list-fixture-cleanup.cjs");
const tid = id => `[data-testid="${id}"]`;

// One fresh open real popup per lifecycle event. The owner remains logged in
// in another browser, so sign-out cannot silently lose cleanup authority.
async function runCompanionLifecycle({ base, userId, createList, ownerApi, record }) {
  const browser = await require("puppeteer").launch({ headless: true });
  const errors = [];
  let originalError;
  let page;
  let currentEvent;
  let currentList;
  let activePopup;
  const timeline = [];
  const state = async () => page && !page.isClosed() ? page.evaluate(({ userId, id }) => ({
    url: location.href, visibility: document.visibilityState, focused: document.hasFocus(),
    hydrated: window.__companionHydrated,
    cachePresent: localStorage.getItem(`ultros.listdoc.v1.${userId}.${id}`) !== null,
    connection: document.querySelector('[data-testid="realtime-status-indicator"]')?.dataset.status,
    enabledPurchases: [...document.querySelectorAll('[data-testid="shop-stack-bought"]')].filter(button => !button.disabled).length,
    body: document.body?.innerText.slice(0, 3000),
  }), { userId, id: currentList }) : null;
  try {
    page = await browser.newPage();
    page.setDefaultTimeout(90000);
    const network = await page.createCDPSession();
    await network.send("Network.enable");
    for (const [event, name] of [["Network.webSocketFrameReceived", "websocket-received"], ["Network.webSocketFrameSent", "websocket-sent"]]) {
      network.on(event, frame => {
        const payload = (frame.response || frame.request)?.payloadData || "";
        if (/List|Error|Subscribe/.test(payload) && timeline.length < 250)
          timeline.push({ time: Date.now(), event: name, requestId: frame.requestId, payload: payload.slice(0, 1000) });
      });
    }
    page.on("pageerror", error => errors.push(String(error)));
    page.on("request", request => {
      if (currentList && request.url().includes(`/api/v1/list/${currentList}`))
        timeline.push({ time: Date.now(), event: "request", method: request.method(), url: request.url() });
    });
    page.on("response", response => {
      if (currentList && response.url().includes(`/api/v1/list/${currentList}`))
        timeline.push({ time: Date.now(), event: "response", status: response.status(), url: response.url() });
    });
    page.on("dialog", dialog => dialog.type() === "beforeunload" ? dialog.accept() : dialog.dismiss());
    await page.evaluateOnNewDocument(() => {
      Object.defineProperty(window, "documentPictureInPicture", { value: undefined, configurable: true });
      window.__companionHydrated = false;
      addEventListener("ultros:hydrated", () => { window.__companionHydrated = true; });
    });
    await page.setCookie(...[["LABS", "lists-sync"], ["HIDE_ADS", "true"], ["i18n_pref_locale", "en"]]
      .map(([name, value]) => ({ name, value, url: base, path: "/" })));
    const projection = response => [response[0], response[1].map(([row]) => row)];
    for (const event of ["revoke", "delete", "signout"]) {
      currentEvent = event;
      const id = await createList([[false, 5]]);
      currentList = id;
      await ownerApi("POST", `/api/v1/list/${id}/share/user`, { user_id: userId, permission: "Write" });
      await page.bringToFront();
      const response = await page.goto(`${base}/test/login?user_id=${userId}&username=CompanionLifecycle&redirect=/list/${id}`, { waitUntil: "domcontentloaded" });
      assert(response.ok(), "test-auth login");
      await page.waitForFunction(() => window.__companionHydrated);
      await page.waitForFunction(() => document.querySelector('[data-testid="realtime-status-indicator"]')?.dataset.status === "live");
      const cacheKey = `ultros.listdoc.v1.${userId}.${id}`;
      await page.waitForFunction(key => localStorage.getItem(key) !== null, {}, cacheKey);
      const before = projection(await ownerApi("GET", `/api/v1/list/${id}/listings`));
      await page.click(tid("guest-shop-mode"));
      await page.waitForSelector(tid("shop-cheapest"), { visible: true });
      await page.click(tid("shop-cheapest"));
      await page.waitForSelector(tid("shop-stack-bought"));
      assert(await page.$$eval(tid("shop-stack-bought"), buttons => buttons.some(button => !button.disabled)), `${event}: priced purchase available before event`);
      const popupPromise = new Promise((resolve, reject) => {
        const timer = setTimeout(() => reject(new Error(`${event}: popup did not open`)), 15000);
        page.once("popup", popup => { clearTimeout(timer); resolve(popup); });
      });
      await page.click(tid("open-shopping-companion"));
      const popup = await popupPromise;
      activePopup = popup;
      popup.on("pageerror", error => errors.push(`${event} companion: ${error}`));
      await popup.bringToFront();
      await popup.waitForSelector(tid("companion-buy"));
      assert.equal(await popup.$eval(tid("companion-buy"), button => button.disabled), false, `${event}: this event has its own actionable open companion`);
      // Leave a real valid purchase draft behind; closure must not commit it.
      await popup.$eval("input", input => { input.value = "1"; input.dispatchEvent(new Event("input", { bubbles: true })); });
      const closed = new Promise((resolve, reject) => {
        const timer = setTimeout(() => reject(new Error(`${event}: open companion survived access loss`)), 15000);
        popup.once("close", () => { clearTimeout(timer); resolve(); });
      });
      if (event === "signout") await page.bringToFront();
      timeline.push({ time: Date.now(), event: "before-trigger", state: await state(), popupClosed: popup.isClosed() });
      const trigger = event === "revoke"
        ? ownerApi("DELETE", `/api/v1/list/${id}/share/user/${userId}`)
        : event === "delete"
          ? ownerApi("DELETE", `/api/v1/list/${id}/delete`)
          : page.goto(`${base}/logout`, { waitUntil: "domcontentloaded" });
      await Promise.all([trigger.then(() => timeline.push({ time: Date.now(), event: "trigger-completed", status: event === "signout" ? "navigation" : 200 })), closed]);
      await page.bringToFront();
      assert(popup.isClosed(), `${event}: companion must close`);
      if (event === "signout") {
        assert(await page.evaluate(key => localStorage.getItem(key) !== null, cacheKey), "sign-out preserves this account's cached document");
      } else {
        await page.waitForFunction(key => localStorage.getItem(key) === null, {}, cacheKey);
      }
      assert(await page.$$eval(tid("shop-stack-bought"), buttons => buttons.every(button => button.disabled)), `${event}: no stale main-view purchase remains actionable`);
      if (event === "delete") {
        assert(!(await ownerApi("GET", "/api/v1/list")).some(entry => entry.list.id === id), "deleted fixture is absent");
      } else {
        assert.deepEqual(projection(await ownerApi("GET", `/api/v1/list/${id}/listings`)), before, `${event}: unsubmitted companion draft cannot purchase`);
      }
      await record(`open-companion-${event}`, page);
      console.log(`[PASS] real priced companion ${event} closes with unsubmitted purchase draft`);
    }
    assert.deepEqual(errors, [], "companion lifecycle must not panic");
  } catch (error) {
    originalError = error;
    try {
      const beforeForeground = await state();
      const popupBefore = activePopup ? { closed: activePopup.isClosed(), url: activePopup.url() } : null;
      if (page && !page.isClosed()) await page.bringToFront();
      await new Promise(resolve => setTimeout(resolve, 1500));
      const afterForeground = await state();
      const readerStatus = page && !page.isClosed() && currentList ? await page.evaluate(async id => (await fetch(`/api/v1/list/${id}/listings`)).status, currentList) : null;
      console.error(`[companion diagnostic] ${JSON.stringify({ event: currentEvent, errors, timeline, beforeForeground,
        popupBefore, afterForeground, popupAfterClosed: activePopup?.isClosed(), readerStatus })}`);
    } catch (diagnosticError) { console.error(`[companion diagnostic failed] ${diagnosticError}`); }
    throw error;
  }
  finally { await finishCleanup([() => browser.close()], originalError); }
}
module.exports = { runCompanionLifecycle };
