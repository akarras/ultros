#!/usr/bin/env node
/* eslint-disable no-console */
/**
 * Cross-platform Puppeteer runner for Ultros (Leptos) E2E screenshots + asserts.
 *
 * Env:
 *  - BASE_URL: base address of the running server (default http://127.0.0.1:8080)
 *  - DEVICE:   "mobile" or "desktop" (default "desktop")
 *  - ROUTES:   comma-separated list of routes to visit (default built-in list)
 *  - TIMEOUT_MS: navigation timeout in ms (default 60000)
 *  - HEADLESS: "new" | "true" | "false" (default "new")
 *  - PUPPETEER_EXECUTABLE_PATH: path to Chrome/Chromium binary (optional)
 *  - CONCURRENCY: number of parallel pages to run (default 16)
 *  - STRICT_CONSOLE: "1" to fail on console errors / page errors (default "1")
 *  - CONSOLE_ALLOW: comma-separated substrings to ignore in console errors
 *  - SKIP_ASSERTS: "1" to skip per-route content assertions (default "0")
 */

"use strict";

const fs = require("fs");
const path = require("path");

function parseHeadless(value) {
  if (value === undefined || value === null || value === "") return "new";
  const v = String(value).toLowerCase();
  if (v === "new") return "new";
  if (v === "true" || v === "1") return true;
  if (v === "false" || v === "0") return false;
  return "new";
}

function envFlag(name, def) {
  const v = process.env[name];
  if (v === undefined || v === "") return def;
  return v === "1" || v.toLowerCase() === "true";
}

function sanitizeFileComponent(s) {
  const replaced = s.replace(/[\\/?%*:|"<>]/g, "_").replace(/__+/g, "_");
  return replaced.length ? replaced : "_root";
}

/**
 * Per-route assertions. Each entry has:
 *   - titleIncludes:    substring expected in <title>
 *   - bodyIncludesAny:  array of substrings; at least one must appear in body text
 *   - bodyExcludes:     array of substrings that must NOT appear (e.g., generic error pages)
 * Missing keys are skipped. Add routes here as the app grows.
 */
const ROUTE_ASSERTS = {
  "/": { titleIncludes: "Ultros" },
  "/items": { titleIncludes: "Ultros" },
  "/item/46010": { titleIncludes: "Ceremonial Shamshir" },
  "/items/category/Gunbreaker's Arms": { titleIncludes: "Gunbreaker" },
  "/flip-finder": { titleIncludes: "Ultros" },
  "/flip-finder/Gilgamesh": { titleIncludes: "Gilgamesh" },
  "/list": { titleIncludes: "Ultros" },
  "/retainers": { titleIncludes: "Ultros" },
  "/currency-exchange": { titleIncludes: "Ultros" },
  "/history": { titleIncludes: "Ultros" },
  "/settings": { titleIncludes: "Ultros" },
  "/groups": { titleIncludes: "Groups", bodyIncludesAny: ["Groups", "No groups found"] },
  "/privacy": { titleIncludes: "Ultros", bodyIncludesAny: ["privacy", "Privacy"] },
  "/cookie-policy": { titleIncludes: "Ultros", bodyIncludesAny: ["cookie", "Cookie"] },
};

// Substrings in console errors that we always ignore (third-party noise, expected hydration churn).
const DEFAULT_CONSOLE_ALLOW = [
  "favicon",
  "ERR_BLOCKED_BY_CLIENT", // ad/tracker blockers
  "net::ERR_ABORTED",       // navigation aborts during fast clicks
];

function getRoutes() {
  if (process.env.ROUTES && process.env.ROUTES.trim()) {
    return process.env.ROUTES.split(",")
      .map((r) => r.trim())
      .filter(Boolean);
  }
  return [
    "/",
    "/items",
    "/item/46010",
    "/items/category/Gunbreaker's Arms",
    "/flip-finder",
    "/flip-finder/Gilgamesh",
    "/list",
    "/retainers",
    "/currency-exchange",
    "/history",
    "/settings",
    "/groups",
    "/help",
    "/help/flip-finder",
    "/privacy",
    "/cookie-policy",
  ];
}

async function navigateWithFallback(page, url, timeout) {
  try {
    return await page.goto(url, { waitUntil: "networkidle0", timeout });
  } catch (e) {
    console.warn(
      `[warn] networkidle0 timed out, retrying domcontentloaded: ${url}`,
    );
    return await page.goto(url, { waitUntil: "domcontentloaded", timeout });
  }
}

async function runAsserts(page, route, asserts) {
  const failures = [];
  if (asserts.titleIncludes) {
    const title = await page.title();
    if (!title.includes(asserts.titleIncludes)) {
      failures.push(
        `title check: expected substring "${asserts.titleIncludes}" in "${title}"`,
      );
    }
  }
  if (asserts.bodyIncludesAny && asserts.bodyIncludesAny.length) {
    const body = await page.evaluate(() => document.body.innerText || "");
    const hit = asserts.bodyIncludesAny.some((s) => body.includes(s));
    if (!hit) {
      failures.push(
        `body check: none of [${asserts.bodyIncludesAny.join(", ")}] found`,
      );
    }
  }
  if (asserts.bodyExcludes && asserts.bodyExcludes.length) {
    const body = await page.evaluate(() => document.body.innerText || "");
    for (const bad of asserts.bodyExcludes) {
      if (body.includes(bad)) {
        failures.push(`body check: forbidden substring "${bad}" present`);
      }
    }
  }
  return failures;
}

async function main() {
  const puppeteer = require("puppeteer");

  const BASE_URL = process.env.BASE_URL || "http://127.0.0.1:8080";
  const DEVICE = (process.env.DEVICE || "desktop").toLowerCase();
  const isMobile = DEVICE.startsWith("m");
  const TIMEOUT_MS = Number(process.env.TIMEOUT_MS || 60000);
  const STRICT_CONSOLE = envFlag("STRICT_CONSOLE", true);
  const SKIP_ASSERTS = envFlag("SKIP_ASSERTS", false);
  const userAllow = (process.env.CONSOLE_ALLOW || "")
    .split(",")
    .map((s) => s.trim())
    .filter(Boolean);
  const consoleAllow = [...DEFAULT_CONSOLE_ALLOW, ...userAllow];

  const viewport = isMobile
    ? { width: 390, height: 844, isMobile: true, deviceScaleFactor: 2 }
    : { width: 1280, height: 800, deviceScaleFactor: 1 };

  const headless = parseHeadless(process.env.HEADLESS);
  const executablePath = process.env.PUPPETEER_EXECUTABLE_PATH || undefined;
  const routes = getRoutes();
  const CONCURRENCY = Math.max(1, Number(process.env.CONCURRENCY || 16));

  const outdir = path.resolve(__dirname, "artifacts");
  fs.mkdirSync(outdir, { recursive: true });

  console.log(`[info] BASE_URL=${BASE_URL}`);
  console.log(`[info] DEVICE=${isMobile ? "mobile" : "desktop"}`);
  console.log(`[info] OUTPUT_DIR=${outdir}`);
  console.log(`[info] HEADLESS=${headless}`);
  console.log(`[info] STRICT_CONSOLE=${STRICT_CONSOLE} SKIP_ASSERTS=${SKIP_ASSERTS}`);
  if (executablePath) console.log(`[info] USING_EXECUTABLE=${executablePath}`);
  console.log(`[info] CONCURRENCY=${CONCURRENCY}`);

  // Collected failures across all routes/workers; printed and asserted at end.
  const failures = [];

  let browser;
  try {
    const launchOpts = {
      headless,
      args: ["--no-sandbox", "--disable-setuid-sandbox"],
      executablePath,
    };

    browser = await puppeteer.launch(launchOpts);

    let index = 0;
    const total = routes.length;

    const worker = async (id) => {
      const page = await browser.newPage();
      await page.setViewport(viewport);

      // Per-page error sinks — re-bound on each navigation since handlers stick across navigations.
      let currentRoute = null;
      const consoleErrors = [];
      const pageErrors = [];

      const isAllowed = (msg) =>
        consoleAllow.some((s) => msg.includes(s));

      page.on("console", (msg) => {
        if (msg.type() === "error") {
          const text = msg.text();
          if (!isAllowed(text)) consoleErrors.push({ route: currentRoute, text });
        }
      });
      page.on("pageerror", (err) => {
        const text = (err && err.stack) || String(err);
        if (!isAllowed(text)) pageErrors.push({ route: currentRoute, text });
      });

      while (true) {
        const i = index++;
        if (i >= total) break;
        const r = routes[i];
        currentRoute = r;
        const url = new URL(r, BASE_URL).toString();
        console.log(`[step] visiting ${url}`);
        const beforeConsole = consoleErrors.length;
        const beforePage = pageErrors.length;

        const resp = await navigateWithFallback(page, url, TIMEOUT_MS);
        const status = resp ? resp.status() : -1;
        if (!resp || status >= 400) {
          failures.push(`${r}: bad status ${status}`);
          continue;
        }

        await page.waitForSelector("body", { timeout: 10000 }).catch(() => {});
        await new Promise((r) => setTimeout(r, 1000));

        if (!SKIP_ASSERTS && ROUTE_ASSERTS[r]) {
          const fails = await runAsserts(page, r, ROUTE_ASSERTS[r]);
          for (const f of fails) failures.push(`${r}: ${f}`);
        }

        if (STRICT_CONSOLE) {
          const newConsole = consoleErrors.slice(beforeConsole);
          const newPage = pageErrors.slice(beforePage);
          for (const e of newConsole) failures.push(`${r}: console.error: ${e.text}`);
          for (const e of newPage) failures.push(`${r}: page error: ${e.text}`);
        }

        const safe = sanitizeFileComponent(r);
        const filename = `${safe}-${isMobile ? "mobile" : "desktop"}.png`;
        const file = path.join(outdir, filename);

        await page.screenshot({ path: file, fullPage: true });
        console.log(`[ok] ${url} -> ${file}`);
      }
      await page.close();
    };

    await Promise.all(Array.from({ length: CONCURRENCY }, (_, i) => worker(i)));

    await browser.close();
    browser = null;

    if (failures.length) {
      console.error(`[fail] ${failures.length} assertion failure(s):`);
      for (const f of failures) console.error(`  - ${f}`);
      process.exitCode = 1;
    } else {
      console.log("[done] all routes ok, screenshots + asserts complete");
    }
  } catch (err) {
    console.error("[error]", err && err.stack ? err.stack : err);
    if (browser) {
      try {
        await browser.close();
      } catch (_) {
        // ignore
      }
    }
    process.exitCode = 1;
  }
}

process.on("SIGINT", () => {
  console.log("\n[info] received SIGINT, exiting...");
  process.exit(130);
});

main();
