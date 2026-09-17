"use strict";

// Task 25: end-to-end coverage for the notification inbox + browser
// (guest) price alerts. Needs a `--features test-auth` build for
// `/test/login`.
//
// Covers, in one browser context so the WebSocket instrumentation and
// localStorage carry across the sign-in navigation:
//   1. Guest creates a client-side price-alert rule from `/alerts` via
//      `AlertDrawer` (guest mode) — persisted to `ultros.guest_alerts.v1`.
//   2. The realtime evaluator (`GuestAlertEvaluator`) opens an `AddSubscribe`
//      market subscription for that rule's item; a synthetic `Listings/Added`
//      event injected over the (instrumented) WebSocket fires the rule,
//      landing a local hit in the notification inbox (`ultros.inbox.local.v1`)
//      and lighting up the sidebar `.side-nav-count` pill.
//   3. Marking the inbox read clears the pill and flips `read: true` locally.
//   4. Signing in surfaces `GuestAlertAdoptionBanner`; adopting turns the
//      guest rule into a real server-side `Alert` (`POST /api/v1/alerts`),
//      clears the guest rule, and writes a durable adoption receipt.
//   5. A synthetic `Notification` (`AlertEvent`) injected over the signed-in
//      socket's `SubscribeNotifications` subscription lands in the inbox the
//      same way a real server-pushed alert fire would.
//
// BASE_URL=<isolated test-auth server> node integration/notification-inbox.cjs
const assert = require("node:assert/strict");
const puppeteer = require("puppeteer");

const base = new URL(process.env.BASE_URL || "http://127.0.0.1:8080").origin;
const ITEM_NAME = "Bronze Ingot";
const WORLD_NAME = "Gilgamesh";
// Deliberately tiny (predicate is `<=`, and the injected listing below sets
// `price_per_unit: 1`, so it still matches): a threshold like 999999 makes
// almost *any* real listing on this item match too, turning this into a
// live-data-dependent flake instead of a deterministic synthetic-event test.
const THRESHOLD = 1;
const USER_ID = 990000004000 + (Date.now() % 100000);
const USERNAME = "e2e-inbox";

// Installed before any page script runs (`evaluateOnNewDocument`), and
// reinstalled automatically on every navigation — including the
// `/test/login` redirect — so the instrumentation always covers whichever
// socket the app opens for the current document.
function installInstrumentation() {
  window.__ultrosHydrated = false;
  window.addEventListener("ultros:hydrated", () => {
    window.__ultrosHydrated = true;
  });

  window.__ultrosSockets = [];
  window.__ultrosSent = [];
  const NativeWebSocket = window.WebSocket;

  function PatchedWebSocket(url, protocols) {
    const socket =
      protocols === undefined ? new NativeWebSocket(url) : new NativeWebSocket(url, protocols);
    window.__ultrosSockets.push(socket);
    const nativeSend = socket.send.bind(socket);
    socket.send = (data) => {
      try {
        window.__ultrosSent.push(JSON.parse(data));
      } catch (e) {
        // Non-JSON frame; nothing for this harness to record.
      }
      return nativeSend(data);
    };
    return socket;
  }
  PatchedWebSocket.prototype = NativeWebSocket.prototype;
  PatchedWebSocket.CONNECTING = NativeWebSocket.CONNECTING;
  PatchedWebSocket.OPEN = NativeWebSocket.OPEN;
  PatchedWebSocket.CLOSING = NativeWebSocket.CLOSING;
  PatchedWebSocket.CLOSED = NativeWebSocket.CLOSED;
  window.WebSocket = PatchedWebSocket;

  // Dispatches a synthetic server->client frame on the most recently opened
  // OPEN socket, exactly as `RealtimeClient`'s real `onmessage` receives one
  // (`event.data().as_string()` then `serde_json::from_str::<ServerClient>`).
  window.__ultrosInject = (json) => {
    const open = window.__ultrosSockets.filter((s) => s.readyState === NativeWebSocket.OPEN);
    const socket = open[open.length - 1];
    if (!socket) {
      throw new Error("no open websocket to inject into");
    }
    socket.dispatchEvent(new MessageEvent("message", { data: JSON.stringify(json) }));
  };
}

async function clickButtonByText(page, text, scopeSelector) {
  const found = await page.evaluate(
    (text, scopeSelector) => {
      const scope = (scopeSelector && document.querySelector(scopeSelector)) || document.body;
      const btn = [...scope.querySelectorAll("button")].find((b) => b.textContent.trim() === text);
      if (!btn) return false;
      btn.click();
      return true;
    },
    text,
    scopeSelector,
  );
  assert(found, `button with text "${text}" not found (scope: ${scopeSelector || "body"})`);
}

async function waitHydrated(page) {
  await page.waitForFunction(() => window.__ultrosHydrated === true, { timeout: 90000 });
}

async function resolveWorldId(page, selector) {
  if (selector.World !== undefined) return selector.World;
  const data = await page.evaluate(async () => {
    const r = await fetch("/api/v1/world_data");
    return r.json();
  });
  for (const region of data.regions) {
    if (selector.Region !== undefined && region.id === selector.Region) {
      return region.datacenters[0].worlds[0].id;
    }
    for (const dc of region.datacenters) {
      if (selector.Datacenter !== undefined && dc.id === selector.Datacenter) {
        return dc.worlds[0].id;
      }
    }
  }
  throw new Error(`could not resolve a world id for selector ${JSON.stringify(selector)}`);
}

async function apiFetch(page, method, path, body) {
  return page.evaluate(
    async ({ method, path, body }) => {
      const r = await fetch(path, {
        method,
        credentials: "include",
        headers: body === undefined ? {} : { "Content-Type": "application/json" },
        body: body === undefined ? undefined : JSON.stringify(body),
      });
      const text = await r.text();
      let data;
      try {
        data = text ? JSON.parse(text) : null;
      } catch {
        data = text;
      }
      return { status: r.status, data };
    },
    { method, path, body },
  );
}

// Deletes every `below_threshold` alert whose `item_id` matches `itemId`,
// re-fetched fresh from the server rather than relying on an id captured
// mid-flow — a caller reaches for this in `finally` specifically so a
// failed assertion between "adoption created the alert" and "we noticed"
// can never leak the row.
async function deleteBelowThresholdAlertsForItem(page, itemId) {
  if (itemId === undefined) return;
  const { status, data } = await apiFetch(page, "GET", "/api/v1/alerts");
  if (status !== 200 || !Array.isArray(data)) return;
  const matches = data.filter(
    (a) => a.trigger && a.trigger.type === "below_threshold" && a.trigger.item_id === itemId,
  );
  for (const alert of matches) {
    await apiFetch(page, "DELETE", `/api/v1/alerts/${alert.id}`);
  }
}

async function main() {
  const browser = await puppeteer.launch({ headless: true, args: ["--no-sandbox"] });
  const createdAlertIds = new Set();
  const errors = [];
  let page;
  // Hoisted out of the `try` block so `finally` can still reach it for
  // fallback cleanup even when an assertion throws before the normal
  // `createdAlertIds.add(...)` path runs.
  let itemId;

  try {
    page = await browser.newPage();
    page.setDefaultTimeout(60000);
    await page.setViewport({ width: 1280, height: 900 });
    page.on("pageerror", (e) => errors.push(e.message));

    await page.setCookie(
      { name: "i18n_pref_locale", value: "en", url: base, path: "/" },
      { name: "HIDE_ADS", value: "true", url: base, path: "/" },
    );
    await page.evaluateOnNewDocument(installInstrumentation);

    // ---- 1. Guest flow: create a client-side price-alert rule ----
    await page.goto(new URL("/alerts", base).href, { waitUntil: "domcontentloaded" });
    await waitHydrated(page);
    await page.waitForSelector('[data-testid="guest-alerts-view"]', { visible: true });
    console.log("[ok] guest alerts view rendered post-hydration");

    await clickButtonByText(page, "Add alert", "body");
    await page.waitForSelector('[role="dialog"] #create-alert-search', { visible: true });

    await page.type('[role="dialog"] #create-alert-search', ITEM_NAME);
    await page.waitForFunction(
      (name) =>
        [...document.querySelectorAll('[role="dialog"] button')].some(
          (b) => b.textContent.trim() === name,
        ),
      {},
      ITEM_NAME,
    );
    await clickButtonByText(page, ITEM_NAME, '[role="dialog"]');

    await page.click('[role="dialog"] input[role="combobox"]');
    await page.type('[role="dialog"] input[role="combobox"]', WORLD_NAME);
    await page.waitForFunction(
      (world) =>
        [...document.querySelectorAll('[role="option"]')].some((el) =>
          el.textContent.trim().includes(world),
        ),
      {},
      WORLD_NAME,
    );
    await page.evaluate((world) => {
      const option = [...document.querySelectorAll('[role="option"]')].find((el) =>
        el.textContent.trim().includes(world),
      );
      option.click();
    }, WORLD_NAME);

    await page.type("#create-alert-threshold", String(THRESHOLD));
    await clickButtonByText(page, "Create alert", '[role="dialog"]');

    await page.waitForFunction(() => {
      try {
        const raw = localStorage.getItem("ultros.guest_alerts.v1");
        if (!raw) return false;
        const rules = JSON.parse(raw);
        return Array.isArray(rules) && rules.length === 1;
      } catch {
        return false;
      }
    });
    const guestRule = await page.evaluate(
      () => JSON.parse(localStorage.getItem("ultros.guest_alerts.v1"))[0],
    );
    assert.equal(guestRule.price_threshold, THRESHOLD);
    itemId = guestRule.item_id;
    const ruleId = guestRule.id;
    console.log(`[ok] guest rule persisted to localStorage (item ${itemId}, rule ${ruleId})`);

    // ---- 2. Realtime evaluator subscribes, synthetic listing fires it ----
    await page.waitForFunction(
      (itemId) =>
        window.__ultrosSent.some(
          (m) =>
            m.AddSubscribe &&
            m.AddSubscribe.filter &&
            Array.isArray(m.AddSubscribe.filter.Items) &&
            m.AddSubscribe.filter.Items.includes(itemId),
        ),
      { timeout: 10000 },
      itemId,
    );
    const marketSubId = await page.evaluate((itemId) => {
      const msg = window.__ultrosSent.find(
        (m) =>
          m.AddSubscribe &&
          m.AddSubscribe.filter &&
          Array.isArray(m.AddSubscribe.filter.Items) &&
          m.AddSubscribe.filter.Items.includes(itemId),
      );
      return msg.AddSubscribe.subscription_id;
    }, itemId);
    console.log(`[ok] AddSubscribe observed for item ${itemId} (subscription ${marketSubId})`);

    const worldId = await resolveWorldId(page, guestRule.world_selector);
    await page.evaluate(
      (payload) => window.__ultrosInject(payload),
      {
        SubscriptionEvent: {
          subscription_id: marketSubId,
          event: {
            Listings: {
              Added: {
                item_id: itemId,
                world_id: worldId,
                listings: [
                  [
                    {
                      id: 123456789,
                      world_id: worldId,
                      item_id: itemId,
                      retainer_id: 1,
                      price_per_unit: 1,
                      quantity: 1,
                      hq: false,
                      timestamp: "2024-01-01T00:00:00",
                    },
                    {
                      id: 1,
                      world_id: worldId,
                      name: "E2E",
                      retainer_city_id: 1,
                    },
                  ],
                ],
              },
            },
          },
        },
      },
    );

    await page.waitForFunction(
      () => document.querySelector(".side-nav-count")?.textContent.trim() === "1",
      { timeout: 5000 },
    );
    console.log("[ok] injected listing fired the guest rule; sidebar pill shows 1");

    // ---- 3. Open the inbox, mark read ----
    await page.click('button[aria-label="Notifications"]');
    await page.waitForSelector(".side-nav-inbox-panel", { visible: true });
    const rowCount = await page.$$eval(".side-nav-inbox-panel .inbox-item", (els) => els.length);
    assert.equal(rowCount, 1, "expected exactly one inbox row after the guest hit");

    await page.click('[data-testid="inbox-mark-all-read"]');
    await page.waitForFunction(() => !document.querySelector(".side-nav-count"), { timeout: 5000 });
    const localInbox = await page.evaluate(() =>
      JSON.parse(localStorage.getItem("ultros.inbox.local.v1") || "[]"),
    );
    assert(localInbox.length >= 1, "expected at least one local inbox entry");
    assert(
      localInbox.every((item) => item.read === true),
      `expected every local inbox entry read: ${JSON.stringify(localInbox)}`,
    );
    console.log("[ok] mark-all-read clears the pill and flips read=true in localStorage");

    // ---- 4. Sign in, adopt the guest rule ----
    const loginUrl = new URL("/test/login", base);
    loginUrl.searchParams.set("user_id", String(USER_ID));
    loginUrl.searchParams.set("username", USERNAME);
    loginUrl.searchParams.set("redirect", "/alerts");
    await page.goto(loginUrl.href, { waitUntil: "domcontentloaded" });
    await waitHydrated(page);

    await page.waitForSelector('[data-testid="guest-alert-adopt-banner"]', { visible: true });
    const bannerText = await page.$eval(
      '[data-testid="guest-alert-adopt-banner"]',
      (el) => el.textContent,
    );
    assert(
      bannerText.includes("Add 1 browser alert"),
      `adoption banner text unexpected: ${bannerText}`,
    );
    await page.click('[data-testid="guest-alert-adopt-button"]');

    let adoptedAlert;
    {
      const deadline = Date.now() + 20000;
      while (Date.now() < deadline) {
        const { status, data } = await apiFetch(page, "GET", "/api/v1/alerts");
        assert.equal(status, 200, `GET /api/v1/alerts: ${JSON.stringify(data)}`);
        // AlertTrigger is `#[serde(tag = "type", rename_all = "snake_case")]`
        // (ultros-api-types/src/alert.rs) — an internally-tagged enum, not
        // externally tagged, so the variant's own fields sit flat on
        // `trigger` alongside `type`, not nested under a `BelowThreshold` key.
        adoptedAlert = (data || []).find(
          (a) => a.trigger && a.trigger.type === "below_threshold" && a.trigger.item_id === itemId,
        );
        if (adoptedAlert) break;
        await new Promise((resolve) => setTimeout(resolve, 300));
      }
    }
    assert(adoptedAlert, "adopted alert did not appear in GET /api/v1/alerts within 20s");
    createdAlertIds.add(adoptedAlert.id);

    const remainingGuestRules = await page.evaluate(() => {
      const raw = localStorage.getItem("ultros.guest_alerts.v1");
      if (!raw) return [];
      try {
        return JSON.parse(raw);
      } catch {
        return [];
      }
    });
    assert.equal(remainingGuestRules.length, 0, "guest rule should be removed after adoption");

    const receiptKey = `ultros:guest-alert-adoption:v1:${USER_ID}:${ruleId}`;
    const receipt = await page.evaluate((key) => localStorage.getItem(key), receiptKey);
    assert(receipt, `expected adoption receipt at ${receiptKey}`);
    console.log(`[ok] sign-in adopted the guest rule into account alert ${adoptedAlert.id}`);

    // ---- 5. Signed-in live: synthetic server Notification ----
    await page.waitForFunction(() => window.__ultrosSent.some((m) => m.SubscribeNotifications), {
      timeout: 10000,
    });
    const notifSubId = await page.evaluate(() => {
      const msg = window.__ultrosSent.find((m) => m.SubscribeNotifications);
      return msg.SubscribeNotifications.subscription_id;
    });
    console.log(`[ok] SubscribeNotifications observed (subscription ${notifSubId})`);

    await page.evaluate(
      (payload) => window.__ultrosInject(payload),
      {
        SubscriptionEvent: {
          subscription_id: notifSubId,
          event: {
            Notification: {
              id: 987654321,
              alert_id: adoptedAlert.id,
              fired_at: new Date().toISOString(),
              item_id: itemId,
              matched_listing_id: null,
              matched_price: 1,
              delivered: true,
              delivery_error: null,
              read_at: null,
              title: "E2E notification",
              body: "E2E body",
              click_url: "/alerts",
            },
          },
        },
      },
    );

    // Assert on the injected row's own title rather than an exact pill
    // count here: this signed-in session's account alert now uses
    // THRESHOLD (1 gil), but unlike the first pill assertion right after
    // injection (above, before any other event could possibly have
    // arrived), by this point in the flow a real background fire is not
    // ruled out — the title is a precise check either way.
    await page.click('button[aria-label="Notifications"]');
    await page.waitForSelector(".side-nav-inbox-panel", { visible: true });
    await page.waitForFunction(
      () =>
        [...document.querySelectorAll(".side-nav-inbox-panel .inbox-item-title")].some(
          (el) => el.textContent.trim() === "E2E notification",
        ),
      { timeout: 5000 },
    );
    console.log("[ok] synthetic server notification lands in the signed-in inbox");

    // ---- 6. Clear is a two-tap action and deletes server-side ----
    // First click only arms the button (label flips to the confirm copy and
    // nothing is sent); the second click empties both inbox halves and
    // POSTs `/api/v1/alerts/events/clear` bounded by the newest rendered id.
    const clearRequests = [];
    page.on("request", (req) => {
      if (req.method() === "POST" && req.url().endsWith("/api/v1/alerts/events/clear")) {
        clearRequests.push(JSON.parse(req.postData() || "{}"));
      }
    });
    const clearResponse = page.waitForResponse(
      (res) => res.url().endsWith("/api/v1/alerts/events/clear"),
      { timeout: 5000 },
    );
    await page.click('[data-testid="inbox-clear"]');
    await page.waitForFunction(
      () => document.querySelector('[data-testid="inbox-clear"]')?.textContent.trim() === "Clear all?",
      { timeout: 5000 },
    );
    assert.equal(clearRequests.length, 0, "arming Clear must not send a request");
    await page.click('[data-testid="inbox-clear"]');
    const clearStatus = (await clearResponse).status();
    assert.equal(clearStatus, 200, `clear endpoint returned ${clearStatus}`);
    assert.equal(clearRequests.length, 1, "second click sends exactly one clear request");
    assert.equal(clearRequests[0].up_to_id, 987654321, "clear is bounded by the newest rendered id");
    await page.waitForFunction(
      () =>
        document.querySelectorAll(".side-nav-inbox-panel .inbox-item").length === 0 &&
        !document.querySelector(".side-nav-count"),
      { timeout: 5000 },
    );
    assert.equal(
      await page.$eval('[data-testid="inbox-clear"]', (el) => el.disabled),
      true,
      "Clear disables once the inbox is empty",
    );
    console.log("[ok] two-tap Clear empties the inbox and deletes server-side (bounded by up_to_id)");

    assert.deepEqual(errors, [], `unexpected page errors: ${JSON.stringify(errors)}`);
    console.log("Notification inbox + browser price alerts E2E passed.");
  } catch (error) {
    if (page) {
      try {
        console.error("Failure page:", page.url());
        console.error(
          "Failure DOM snapshot:",
          await page.evaluate(() => ({
            pill: document.querySelector(".side-nav-count")?.textContent,
            panel: document.querySelector(".side-nav-inbox-panel")?.textContent,
            guestAlerts: localStorage.getItem("ultros.guest_alerts.v1"),
            localInbox: localStorage.getItem("ultros.inbox.local.v1"),
          })),
        );
      } catch {
        // Best effort only.
      }
    }
    throw error;
  } finally {
    if (page) {
      for (const id of createdAlertIds) {
        try {
          await apiFetch(page, "DELETE", `/api/v1/alerts/${id}`);
        } catch (e) {
          console.error("fixture cleanup failed:", e.message);
        }
      }
      // Belt-and-suspenders: `createdAlertIds` is only populated once the
      // post-adoption polling assertion succeeds, so a failure anywhere
      // between the adoption click and that assertion (e.g. a mismatched
      // trigger-shape predicate) would otherwise leak the server-side
      // alert this run created. Re-fetch fresh and delete by item id
      // instead of trusting anything captured mid-flow. A no-op (and
      // silently ignored) when `itemId` was never set or the account
      // session is gone.
      try {
        await deleteBelowThresholdAlertsForItem(page, itemId);
      } catch (e) {
        console.error("fallback fixture cleanup failed:", e.message);
      }
    }
    await browser.close();
  }
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
