#!/usr/bin/env node
/* eslint-disable no-console */
/**
 * Login flow E2E. Requires the server to be built with `--features test-auth`,
 * which exposes `GET /test/login?user_id=...&username=...`.
 *
 * Steps:
 *   1. Hit /test/login → server mints a `discord_auth` cookie pointing at an
 *      in-memory cache entry, no Discord round-trip.
 *   2. GET /api/v1/current_user with that cookie → expect JSON with our username.
 *   3. Visit /settings → expect a 200 (logged-in user can hit gated pages).
 *
 * Env:
 *   BASE_URL  default http://127.0.0.1:8080
 *   HEADLESS  see runner.cjs
 *   TIMEOUT_MS default 30000
 */

"use strict";

const TEST_USER_ID = 7777777777777;
const TEST_USERNAME = "E2ETestUser";

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

  try {
    const page = await browser.newPage();
    page.setDefaultTimeout(TIMEOUT_MS);

    // --- Step 1: mint a session via the test-only endpoint.
    const loginUrl = new URL("/test/login", BASE_URL);
    loginUrl.searchParams.set("user_id", String(TEST_USER_ID));
    loginUrl.searchParams.set("username", TEST_USERNAME);
    loginUrl.searchParams.set("redirect", "/");
    console.log(`[login] GET ${loginUrl}`);
    const loginResp = await page.goto(loginUrl.toString(), {
      waitUntil: "domcontentloaded",
    });
    if (!loginResp || loginResp.status() >= 400) {
      throw new Error(
        `test login failed: status ${loginResp ? loginResp.status() : -1}. ` +
          `Did you build with --features test-auth?`,
      );
    }

    const cookies = await page.cookies(BASE_URL);
    const auth = cookies.find((c) => c.name === "discord_auth");
    if (!auth) {
      failures.push("expected `discord_auth` cookie to be set after /test/login");
    } else {
      console.log(`[login] discord_auth cookie set (len=${auth.value.length})`);
    }

    // --- Step 2: fetch /api/v1/current_user from inside the page so cookies attach.
    const apiResult = await page.evaluate(async () => {
      const r = await fetch("/api/v1/current_user", { credentials: "include" });
      return { status: r.status, body: await r.text() };
    });
    if (apiResult.status !== 200) {
      failures.push(
        `current_user: expected 200, got ${apiResult.status} (body: ${apiResult.body.slice(0, 200)})`,
      );
    } else {
      let parsed;
      try {
        parsed = JSON.parse(apiResult.body);
      } catch (e) {
        failures.push(`current_user: body was not JSON: ${apiResult.body.slice(0, 200)}`);
      }
      if (parsed) {
        if (parsed.username !== TEST_USERNAME) {
          failures.push(
            `current_user: expected username "${TEST_USERNAME}", got "${parsed.username}"`,
          );
        }
        if (String(parsed.id) !== String(TEST_USER_ID)) {
          failures.push(
            `current_user: expected id ${TEST_USER_ID}, got ${parsed.id}`,
          );
        }
      }
    }

    // --- Step 3: settings page should render with logged-in user (or at least 200).
    const settingsUrl = new URL("/settings", BASE_URL).toString();
    const settingsResp = await page.goto(settingsUrl, {
      waitUntil: "domcontentloaded",
    });
    const settingsStatus = settingsResp ? settingsResp.status() : -1;
    if (settingsStatus >= 400) {
      failures.push(`/settings: expected 2xx/3xx, got ${settingsStatus}`);
    }

    await page.close();
  } finally {
    await browser.close();
  }

  if (failures.length) {
    console.error(`[fail] ${failures.length} login-flow assertion(s) failed:`);
    for (const f of failures) console.error(`  - ${f}`);
    process.exit(1);
  }
  console.log("[ok] login flow passed");
}

main().catch((err) => {
  console.error("[error]", err && err.stack ? err.stack : err);
  process.exit(1);
});
