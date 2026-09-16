"use strict";
// Application regression, not a standalone companion mock. Caller supplies an
// owned account list with real stock and deletes it afterwards. Only the peer
// compatibility fault is injected; pricing, document and popup are real.
const assert = require("node:assert/strict");
const tid = id => `[data-testid="${id}"]`;

async function runShopAvailability(page, { url, listId, readProjection }) {
  const fixture = require("./fixtures/list-compatibility.json");
  const preload = await page.evaluateOnNewDocument(() => {
    window.__shopAvailability = { subscriptions: [], updates: [], hydrated: false };
    window.addEventListener("ultros:hydrated", () => { window.__shopAvailability.hydrated = true; });
    const NativeSocket = WebSocket;
    window.WebSocket = class extends NativeSocket {
      constructor(...args) {
        super(...args);
        this.addEventListener("message", event => {
          const message = JSON.parse(event.data);
          const subscribed = message.ListDocSubscribed || message.SubscriptionEvent?.event?.ListDocSubscribed;
          if (subscribed) window.__shopAvailability.subscriptions.push({ socket: this, subscribed });
        });
      }
      send(data) {
        if (typeof data === "string" && data.includes('"ListDocUpdate"')) window.__shopAvailability.updates.push(data);
        return super.send(data);
      }
    };
    Object.defineProperty(window, "documentPictureInPicture", { value: undefined, configurable: true });
  });
  let popup;
  try {
    await page.goto(url, { waitUntil: "domcontentloaded" });
    await page.waitForFunction(() => window.__shopAvailability?.hydrated);
    console.log("Shop availability: hydrated account");
    await page.waitForFunction(listId => window.__shopAvailability?.subscriptions.some(entry => entry.subscribed.list_id === listId), {}, listId);
    console.log("Shop availability: native subscription received");
    await page.waitForSelector(tid("guest-shop-mode"));
    await page.locator(tid("guest-shop-mode")).click();
    await page.waitForFunction(() => document.querySelector('[data-testid="guest-shop-mode"]')?.getAttribute("aria-pressed") === "true");
    await page.waitForSelector(tid("shop-cheapest"), { visible: true });
    await page.locator(tid("shop-cheapest")).click();
    await page.waitForSelector(tid("shop-stack-bought"));
    assert(await page.$$eval(tid("shop-stack-bought"), buttons => buttons.some(button => !button.disabled)), "live account must offer a real purchase before compatibility loss");
    console.log("Shop availability: purchasable frozen trip ready");
    const before = await readProjection();
    const popupPromise = new Promise((resolve, reject) => {
      const timer = setTimeout(() => reject(new Error("Shopping companion did not open")), 15000);
      page.once("popup", popup => { clearTimeout(timer); resolve(popup); });
    });
    await page.click(tid("open-shopping-companion"));
    popup = await popupPromise;
    console.log("Shop availability: companion opened");
    await popup.waitForSelector(tid("companion-buy"));
    const closed = new Promise((resolve, reject) => {
      const timer = setTimeout(() => reject(new Error("Unreadable document left companion open")), 15000);
      popup.once("close", () => { clearTimeout(timer); resolve(); });
    });
    await page.evaluate(({ listId, snapshot }) => {
      const probe = window.__shopAvailability;
      const { socket, subscribed } = probe.subscriptions.findLast(entry => entry.subscribed.list_id === listId);
      probe.updates.length = 0;
      // A fresh server snapshot is validated before replacing the last usable
      // document, so a rejected future version retains its title and rows.
      socket.dispatchEvent(new MessageEvent("message", { data: JSON.stringify({
        ListDocSubscribed: { ...subscribed, payload: { Snapshot: snapshot } },
      }) }));
    }, { listId, snapshot: Buffer.from(fixture.future).toString("base64") });
    await closed;
    assert(popup.isClosed(), "companion closes on document availability loss without route navigation");
    const artifactDir = require("node:path").join(__dirname, "artifacts", "device-build-prices");
    require("node:fs").mkdirSync(artifactDir, { recursive: true });
    await page.screenshot({ path: require("node:path").join(artifactDir, "shop-incompatible-before-build.png"), fullPage: true });
    assert.equal(await page.$(tid("shop-cart-summary")), null, "unknown document must not present an empty cart");
    assert.equal(await page.$(tid("open-shopping-companion")), null, "unavailable document cannot reopen companion");
    await page.locator(tid("guest-build-mode")).click();
    await page.waitForSelector(tid("list-compatibility-export"), { visible: true });
    assert.equal(await page.$(tid("list-estimate-total")), null, "unavailable Build must not claim 0 gil");
    await new Promise(resolve => setTimeout(resolve, 600));
    assert.deepEqual(await readProjection(), before, "compatibility failure must not write purchases or replace the server list");
    assert.deepEqual(await page.evaluate(() => window.__shopAvailability.updates), [], "blocked document must not emit purchase updates");
    console.log("PASS live account companion closes and last valid document stays unchanged on incompatible peer snapshot");
  } finally {
    if (popup && !popup.isClosed()) await popup.close();
    await page.removeScriptToEvaluateOnNewDocument(preload.identifier);
  }
}
module.exports = { runShopAvailability };
