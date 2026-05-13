#!/usr/bin/env node
/* eslint-disable no-console */
/**
 * Browser-push E2E smoke. Requires `--features test-auth` for login.
 *
 * Always verifies:
 *   - /service-worker.js is served as JavaScript with Service-Worker-Allowed: /
 *   - the authenticated /alerts page can register the service worker
 *   - Chrome can deliver a synthetic push event to that worker
 *
 * If VAPID is configured, also verifies:
 *   - /api/v1/push/vapid-public-key returns a usable key
 *   - PushManager.subscribe succeeds
 *   - POST /api/v1/push/subscribe creates a WebPush endpoint
 */

"use strict";

const TEST_USER_ID = 7777777777777;
const TEST_USERNAME = "E2EPushUser";

function b64urlToBytes(value) {
  const padding = "=".repeat((4 - (value.length % 4)) % 4);
  const base64 = (value + padding).replace(/-/g, "+").replace(/_/g, "/");
  const raw = Buffer.from(base64, "base64");
  return new Uint8Array(raw);
}

async function waitForRegistration(client, origin, timeoutMs) {
  let seen = [];
  client.on("ServiceWorker.workerRegistrationUpdated", ({ registrations }) => {
    seen = registrations || [];
  });
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    const hit = seen.find((r) => r.scopeURL && r.scopeURL.startsWith(`${origin}/`));
    if (hit) return hit;
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  throw new Error("service worker registration did not appear in CDP");
}

async function main() {
  const puppeteer = require("puppeteer");

  const BASE_URL = process.env.BASE_URL || "http://127.0.0.1:8080";
  const TIMEOUT_MS = Number(process.env.TIMEOUT_MS || 30000);
  const headless = process.env.HEADLESS === "false" ? false : "new";
  const origin = new URL(BASE_URL).origin;

  const browser = await puppeteer.launch({
    headless,
    args: ["--no-sandbox", "--disable-setuid-sandbox"],
  });

  const failures = [];
  try {
    const page = await browser.newPage();
    page.setDefaultTimeout(TIMEOUT_MS);

    const loginUrl = new URL("/test/login", BASE_URL);
    loginUrl.searchParams.set("user_id", String(TEST_USER_ID));
    loginUrl.searchParams.set("username", TEST_USERNAME);
    loginUrl.searchParams.set("redirect", "/");
    const loginResp = await page.goto(loginUrl.toString(), { waitUntil: "domcontentloaded" });
    if (!loginResp || loginResp.status() >= 400) {
      throw new Error(`test login failed: ${loginResp ? loginResp.status() : -1}`);
    }

    const swResp = await page.goto(new URL("/service-worker.js", BASE_URL).toString(), {
      waitUntil: "domcontentloaded",
    });
    if (!swResp || swResp.status() !== 200) {
      failures.push(`service worker status: expected 200, got ${swResp ? swResp.status() : -1}`);
    }
    const contentType = swResp ? swResp.headers()["content-type"] || "" : "";
    if (!contentType.includes("javascript")) {
      failures.push(`service worker content-type did not look like JavaScript: ${contentType}`);
    }
    const allowed = swResp ? swResp.headers()["service-worker-allowed"] || "" : "";
    if (allowed !== "/") {
      failures.push(`Service-Worker-Allowed: expected "/", got "${allowed}"`);
    }

    await page.goto(new URL("/", BASE_URL).toString(), {
      waitUntil: "domcontentloaded",
      timeout: TIMEOUT_MS,
    });
    const supported = await page.evaluate(() => ({
      serviceWorker: "serviceWorker" in navigator,
      pushManager: "PushManager" in window,
      notification: "Notification" in window,
      secureContext: window.isSecureContext,
    }));
    for (const [key, value] of Object.entries(supported)) {
      if (!value) failures.push(`browser missing push prerequisite: ${key}`);
    }

    const client = await page.target().createCDPSession();
    await client.send("ServiceWorker.enable");
    const cdpRegistrationPromise = waitForRegistration(client, origin, TIMEOUT_MS);
    const registration = await page.evaluate(async () => {
      const reg = await navigator.serviceWorker.register("/service-worker.js", { scope: "/" });
      await navigator.serviceWorker.ready;
      return { scope: reg.scope, active: Boolean(reg.active) };
    });
    if (!registration.scope.startsWith(`${origin}/`)) {
      failures.push(`unexpected service worker scope: ${registration.scope}`);
    }

    const cdpRegistration = await cdpRegistrationPromise;
    await client.send("ServiceWorker.deliverPushMessage", {
      origin,
      registrationId: cdpRegistration.registrationId,
      data: JSON.stringify({
        title: "Ultros push smoke",
        body: "Synthetic push event delivered by Puppeteer.",
        url: "/alerts",
      }),
    });

    const vapid = await page.evaluate(async () => {
      const r = await fetch("/api/v1/push/vapid-public-key", { credentials: "include" });
      return { status: r.status, body: await r.text() };
    });
    if (vapid.status === 503) {
      console.log("[push] VAPID not configured; subscription creation skipped");
    } else if (vapid.status !== 200) {
      failures.push(`vapid-public-key: expected 200 or 503, got ${vapid.status}: ${vapid.body}`);
    } else {
      let key;
      try {
        key = JSON.parse(vapid.body).key;
      } catch (e) {
        failures.push(`vapid-public-key body was not JSON: ${vapid.body.slice(0, 200)}`);
      }
      if (key) {
        await page.evaluateOnNewDocument(() => {
          // Kept for browsers that read Notification.permission before the
          // DevTools permission override has propagated.
          Object.defineProperty(Notification, "permission", { get: () => "granted" });
        });
        await browser.defaultBrowserContext().overridePermissions(origin, ["notifications"]);
        const subscriptionResult = await page.evaluate(async (publicKeyBytes) => {
          const reg = await navigator.serviceWorker.ready;
          const sub = await reg.pushManager.subscribe({
            userVisibleOnly: true,
            applicationServerKey: new Uint8Array(publicKeyBytes),
          });
          const json = sub.toJSON();
          const r = await fetch("/api/v1/push/subscribe", {
            method: "POST",
            credentials: "include",
            headers: { "content-type": "application/json" },
            body: JSON.stringify({
              endpoint: json.endpoint,
              p256dh: json.keys.p256dh,
              auth: json.keys.auth,
              user_agent: navigator.userAgent,
            }),
          });
          return { status: r.status, body: await r.text() };
        }, Array.from(b64urlToBytes(key)));
        if (subscriptionResult.status !== 200) {
          failures.push(
            `push subscribe: expected 200, got ${subscriptionResult.status}: ${subscriptionResult.body.slice(0, 300)}`,
          );
        } else {
          const created = JSON.parse(subscriptionResult.body);
          if (created.method !== "WebPush" || !created.subscription_id) {
            failures.push(`push subscribe response was not a WebPush endpoint: ${subscriptionResult.body}`);
          }
        }
      }
    }

    await page.close();
  } finally {
    await browser.close();
  }

  if (failures.length) {
    console.error(`[fail] ${failures.length} push assertion(s) failed:`);
    for (const f of failures) console.error(`  - ${f}`);
    process.exit(1);
  }
  console.log("[ok] browser push smoke passed");
}

main().catch((err) => {
  console.error("[error]", err && err.stack ? err.stack : err);
  process.exit(1);
});
