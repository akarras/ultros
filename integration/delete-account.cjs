#!/usr/bin/env node
"use strict";

// Requires an isolated test-auth server. Uses a unique disposable account.
const assert = require("node:assert/strict");
const puppeteer = require("puppeteer");

async function main() {
  const base = process.env.BASE_URL || "http://127.0.0.1:8080";
  const timeout = Number(process.env.TIMEOUT_MS || 30000);
  const browser = await puppeteer.launch({ headless: true, args: ["--no-sandbox"] });
  const context = await browser.createBrowserContext();
  const page = await context.newPage();
  page.setDefaultTimeout(timeout);
  const errors = [];
  page.on("pageerror", error => errors.push(error.message));
  await page.setCookie(
    { name: "HIDE_ADS", value: "true", url: base, path: "/" },
    { name: "i18n_pref_locale", value: "en", url: base, path: "/" },
  );
  await page.evaluateOnNewDocument(() => {
    window.__deleteAccountHydrated = false;
    window.addEventListener("ultros:hydrated", () => { window.__deleteAccountHydrated = true; });
  });

  const selector = '[data-testid="delete-account"]';
  const button = `${selector} button`;
  const userId = String(Date.now() * 1000 + Math.floor(Math.random() * 1000));
  let releaseFailure;
  let attempts = 0;
  const intercept = request => {
    if (request.method() === "DELETE" && new URL(request.url()).pathname === "/api/v1/current_user") {
      attempts++;
      if (attempts === 1) {
        releaseFailure = () => request.respond({
          status: 500, contentType: "application/json",
          body: JSON.stringify({ ApiError: { Message: "Simulated deletion failure" } }),
        });
        return;
      }
    }
    void request.continue();
  };

  async function api(method, path, body) {
    return page.evaluate(async ({ method, path, body }) => {
      const response = await fetch(path, {
        method,
        credentials: "include",
        headers: body === undefined ? {} : { "Content-Type": "application/json" },
        body: body === undefined ? undefined : JSON.stringify(body),
      });
      return { status: response.status, text: await response.text() };
    }, { method, path, body });
  }

  try {
    const params = new URLSearchParams({
      user_id: userId, username: `DeleteAccount${userId}`, redirect: "/profile?lang=en",
    });
    const login = await page.goto(new URL(`/test/login?${params}`, base).toString(), {
      waitUntil: "domcontentloaded",
    });
    assert(login.ok(), `test login failed: ${login.status()} (requires test-auth)`);
    await page.waitForFunction(() => window.__deleteAccountHydrated);
    await page.waitForSelector(button);

    const worlds = JSON.parse((await api("GET", "/api/v1/world_data")).text);
    const world = worlds.regions[0].datacenters[0].worlds[0].id;
    const created = await api("POST", "/api/v1/list/create", {
      name: `Delete with account ${userId}`, wdr_filter: { World: world },
    });
    assert.equal(created.status, 200, created.text);

    assert(await page.$eval(button, element => element.disabled), "confirmation is required");
    await page.click(`${selector} label`);
    await page.waitForFunction(button => !document.querySelector(button).disabled, {}, button);

    await page.setRequestInterception(true);
    page.on("request", intercept);
    const firstDelete = page.waitForRequest(request =>
      request.method() === "DELETE" && new URL(request.url()).pathname === "/api/v1/current_user");
    await page.click(button);
    await firstDelete;
    await page.waitForFunction(button => {
      const element = document.querySelector(button);
      return element.disabled && element.getAttribute("aria-busy") === "true";
    }, {}, button);
    // Clicking again during the request cannot dispatch a second deletion.
    await page.$eval(button, element => element.click());
    assert.equal(attempts, 1);
    await releaseFailure();
    releaseFailure = undefined;
    await page.waitForSelector(`${selector} [role="alert"]`);
    await page.waitForFunction(button => !document.querySelector(button).disabled, {}, button);
    assert.match(await page.$eval(`${selector} [role="alert"]`, element => element.textContent), /Unable to delete your account/);
    assert.equal((await api("GET", "/api/v1/current_user")).status, 200, "failure preserves login");

    const deletion = page.waitForResponse(response =>
      response.request().method() === "DELETE" && new URL(response.url()).pathname === "/api/v1/current_user");
    const navigation = page.waitForNavigation({ waitUntil: "domcontentloaded" });
    await page.click(button);
    const response = await deletion;
    assert.equal(response.status(), 200, "deletion returns JSON without a redirect");
    assert.match(response.headers()["content-type"], /application\/json/);
    assert.equal(await response.text(), "null");
    await navigation;
    assert.equal(new URL(page.url()).pathname, "/", "success reloads the home page");
    assert.equal(attempts, 2, "one failed attempt and one successful retry");
    assert(!(await context.cookies()).some(cookie => cookie.name === "discord_auth"), "session cookie is removed");
    assert.equal((await api("GET", "/api/v1/current_user")).status, 401, "deleted account is signed out");
    assert.deepEqual(errors, [], "no browser errors during deletion");
    console.log("[ok] account deletion: confirmation, pending state, visible failure, retry, JSON response, and sign-out");
  } finally {
    if (releaseFailure) await releaseFailure().catch(() => {});
    page.off("request", intercept);
    await page.setRequestInterception(false).catch(() => {});
    if ((await context.cookies()).some(cookie => cookie.name === "discord_auth")) {
      await api("DELETE", "/api/v1/current_user").catch(() => {});
    }
    await browser.close();
  }
}

main().catch(error => {
  console.error(error);
  process.exitCode = 1;
});
