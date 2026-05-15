#!/usr/bin/env node
/* eslint-disable no-console */
/**
 * Focused E2E for the new dashboard/analyzer surfaces:
 *   - Home page with Market Pulse KPI strip (requires HOME_WORLD cookie)
 *   - Trends page with ConfidenceBadge column
 *   - Item view for a known launder-prone item (4422, "Behemoth"-class housing)
 *   - Item view for a known healthy item (12, Fire Crystal)
 *
 * Captures both desktop and mobile screenshots into
 *   integration/artifacts/dashboard/
 * so the diff between sessions is easy to inspect visually.
 *
 * Env:
 *   BASE_URL   default http://127.0.0.1:8080
 *   WORLD      default Gilgamesh — name used in URLs *and* the HOME_WORLD cookie
 *   HEADLESS   see runner.cjs
 *   TIMEOUT_MS default 60000
 *
 * Failures (bad status, missing UI elements, console errors) are collected
 * and reported at the end; exit code is non-zero on any failure.
 *
 * Usage (server already running):
 *   cd integration && node dashboard.cjs
 *
 * Usage (full local cycle):
 *   ./scripts/run_e2e.sh          # boots server, runs the desktop+mobile suite
 *   # …then to focus on dashboard pages specifically:
 *   cd integration && node dashboard.cjs
 */

"use strict";

const fs = require("fs");
const path = require("path");

function sanitize(s) {
  return s.replace(/[\\/?%*:|"<>]/g, "_").replace(/__+/g, "_") || "_root";
}

function parseHeadless(value) {
  if (value === undefined || value === null || value === "") return "new";
  const v = String(value).toLowerCase();
  if (v === "new") return "new";
  if (v === "true" || v === "1") return true;
  if (v === "false" || v === "0") return false;
  return "new";
}

/**
 * Per-surface assertion: list of substrings; at least one must appear in
 * the rendered body text. Cheap way to confirm the page actually rendered
 * the component we care about rather than a generic "Loading…" or error.
 */
const SURFACES = (world) => [
  {
    path: "/",
    label: "home",
    requires_home_world: true,
    // The component renders these labels with the `uppercase` Tailwind
    // class; assertions match the rendered text, not the source string.
    bodyIncludesAny: [
      "ACTIVE LISTINGS",
      "SALES (24H)",
      "MARKET VOLUME",
    ],
  },
  {
    path: `/trends/${world}`,
    label: "trends",
    requires_home_world: false,
    bodyIncludesAny: [
      // ConfidenceBadge values: at least one of the band labels should
      // appear in the rendered trends rows.
      "High",
      "Medium",
      "Low",
      "Suspicious",
      // Or the column header itself (in case all rows happen to be Unknown).
      "Quality",
    ],
  },
  {
    // Item 4422 = Copper Ring (vendor price 170 gil). Real Ultros data shows
    // listings at 11M+ — the canonical launder pattern Aaron flagged.
    path: `/item/${world}/4422`,
    label: "item-4422-copper-ring-launder-prone",
    requires_home_world: false,
    bodyIncludesAny: ["Copper Ring"],
  },
  {
    // Item 12 = Lightning Crystal (high-volume consumable, no launder).
    // Healthy market control: tight clustering, clean MAD, no exclusions.
    path: `/item/${world}/12`,
    label: "item-12-lightning-crystal-healthy",
    requires_home_world: false,
    bodyIncludesAny: ["Lightning Crystal", "Crystal"],
  },
];

const DEFAULT_CONSOLE_ALLOW = [
  "favicon",
  "ERR_BLOCKED_BY_CLIENT",
  "net::ERR_ABORTED",
];

async function navigateWithFallback(page, url, timeout) {
  try {
    return await page.goto(url, { waitUntil: "networkidle0", timeout });
  } catch (_e) {
    console.warn(`[warn] networkidle0 timed out, retrying domcontentloaded: ${url}`);
    return await page.goto(url, { waitUntil: "domcontentloaded", timeout });
  }
}

async function captureOneSurface(
  browser,
  surface,
  { baseUrl, world, viewport, deviceLabel, outdir, timeout, consoleAllow },
) {
  const page = await browser.newPage();
  await page.setViewport(viewport);

  const consoleErrors = [];
  const pageErrors = [];
  const isAllowed = (m) => consoleAllow.some((s) => m.includes(s));
  page.on("console", (msg) => {
    if (msg.type() === "error") {
      const t = msg.text();
      if (!isAllowed(t)) consoleErrors.push(t);
    }
  });
  page.on("pageerror", (err) => {
    const t = (err && err.stack) || String(err);
    if (!isAllowed(t)) pageErrors.push(t);
  });

  // Set the HOME_WORLD cookie for surfaces that need it (Market Pulse only
  // renders when the user has a home world set). We do this against the
  // base URL so the cookie applies to every subsequent navigation on the
  // same page.
  if (surface.requires_home_world) {
    const u = new URL(baseUrl);
    await page.setCookie({
      name: "HOME_WORLD",
      value: world,
      domain: u.hostname,
      path: "/",
      // The real cookie is HttpOnly in production via a Set-Cookie header,
      // but client-readable cookies still flow back on requests — Puppeteer
      // doesn't enforce HttpOnly on the seeded value, so this works for
      // the SSR + hydration path.
    });
  }

  const url = new URL(surface.path, baseUrl).toString();
  console.log(`[step] ${deviceLabel} ${surface.label} → ${url}`);

  const failures = [];
  const resp = await navigateWithFallback(page, url, timeout);
  const status = resp ? resp.status() : -1;
  if (!resp || status >= 400) {
    failures.push(`${surface.label} (${deviceLabel}): bad status ${status}`);
    await page.close();
    return failures;
  }

  await page.waitForSelector("body", { timeout: 10000 }).catch(() => {});
  // Hydration churn: dynamic islands (LiveSaleTicker, MarketPulse,
  // RecentlyViewed) can still be settling after networkidle0. Brief pause
  // gives the screenshot a stable frame.
  await new Promise((r) => setTimeout(r, 2000));

  if (surface.bodyIncludesAny && surface.bodyIncludesAny.length) {
    const body = await page.evaluate(() => document.body.innerText || "");
    const hit = surface.bodyIncludesAny.some((s) => body.includes(s));
    if (!hit) {
      failures.push(
        `${surface.label} (${deviceLabel}): none of [${surface.bodyIncludesAny.join(", ")}] in body — component likely not rendered`,
      );
    }
  }

  for (const e of consoleErrors)
    failures.push(`${surface.label} (${deviceLabel}): console.error: ${e}`);
  for (const e of pageErrors)
    failures.push(`${surface.label} (${deviceLabel}): page error: ${e}`);

  const filename = `${sanitize(surface.label)}-${deviceLabel}.png`;
  const file = path.join(outdir, filename);
  await page.screenshot({ path: file, fullPage: true });
  console.log(`[ok]   ${url} → ${file}`);

  await page.close();
  return failures;
}

async function main() {
  const puppeteer = require("puppeteer");

  const BASE_URL = process.env.BASE_URL || "http://127.0.0.1:8080";
  const WORLD = process.env.WORLD || "Gilgamesh";
  const TIMEOUT_MS = Number(process.env.TIMEOUT_MS || 60000);
  const headless = parseHeadless(process.env.HEADLESS);

  const userAllow = (process.env.CONSOLE_ALLOW || "")
    .split(",")
    .map((s) => s.trim())
    .filter(Boolean);
  const consoleAllow = [...DEFAULT_CONSOLE_ALLOW, ...userAllow];

  const outdir = path.resolve(__dirname, "artifacts", "dashboard");
  fs.mkdirSync(outdir, { recursive: true });

  console.log(`[info] BASE_URL=${BASE_URL}`);
  console.log(`[info] WORLD=${WORLD}`);
  console.log(`[info] OUTPUT_DIR=${outdir}`);

  const desktopViewport = { width: 1280, height: 800, deviceScaleFactor: 1 };
  const mobileViewport = {
    width: 390,
    height: 844,
    isMobile: true,
    deviceScaleFactor: 2,
  };

  const surfaces = SURFACES(WORLD);
  const allFailures = [];

  let browser;
  try {
    browser = await puppeteer.launch({
      headless,
      args: ["--no-sandbox", "--disable-setuid-sandbox"],
      executablePath: process.env.PUPPETEER_EXECUTABLE_PATH || undefined,
    });

    // Run desktop then mobile sequentially. The surfaces themselves are
    // sequential per-device too; concurrent navigation across pages with
    // a shared cookie jar can race in subtle ways.
    for (const surface of surfaces) {
      const fails = await captureOneSurface(browser, surface, {
        baseUrl: BASE_URL,
        world: WORLD,
        viewport: desktopViewport,
        deviceLabel: "desktop",
        outdir,
        timeout: TIMEOUT_MS,
        consoleAllow,
      });
      allFailures.push(...fails);
    }

    for (const surface of surfaces) {
      const fails = await captureOneSurface(browser, surface, {
        baseUrl: BASE_URL,
        world: WORLD,
        viewport: mobileViewport,
        deviceLabel: "mobile",
        outdir,
        timeout: TIMEOUT_MS,
        consoleAllow,
      });
      allFailures.push(...fails);
    }

    await browser.close();
    browser = null;

    if (allFailures.length) {
      console.error(`[fail] ${allFailures.length} failure(s):`);
      for (const f of allFailures) console.error(`  - ${f}`);
      process.exitCode = 1;
    } else {
      console.log(
        `[done] ${surfaces.length * 2} screenshots captured under ${outdir}`,
      );
    }
  } catch (err) {
    console.error("[error]", err && err.stack ? err.stack : err);
    if (browser)
      try {
        await browser.close();
      } catch (_) {}
    process.exitCode = 1;
  }
}

main();
