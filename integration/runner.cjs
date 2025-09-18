#!/usr/bin/env node
/* eslint-disable no-console */
/**
 * Cross-platform Puppeteer runner for Ultros (Leptos) E2E screenshots.
 *
 * Env:
 *  - BASE_URL: base address of the running server (default http://127.0.0.1:8080)
 *  - DEVICE:   "mobile" or "desktop" (default "desktop")
 *  - ROUTES:   comma-separated list of routes to visit (default built-in list)
 *  - TIMEOUT_MS: navigation timeout in ms (default 60000)
 *  - HEADLESS: "new" | "true" | "false" (default "new")
 *  - PUPPETEER_EXECUTABLE_PATH: path to Chrome/Chromium binary (optional)
 *  - CONCURRENCY: number of parallel pages to run (default 16)
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

function sanitizeFileComponent(s) {
  // Replace characters invalid on Windows and others with underscore
  // Also collapse consecutive underscores for readability
  const replaced = s.replace(/[\\/?%*:|"<>]/g, "_").replace(/__+/g, "_");
  return replaced.length ? replaced : "_root";
}

function getRoutes() {
  if (process.env.ROUTES && process.env.ROUTES.trim()) {
    return process.env.ROUTES.split(",")
      .map((r) => r.trim())
      .filter(Boolean);
  }
  // Default set of app pages to sanity-check
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

async function main() {
  const puppeteer = require("puppeteer");

  const BASE_URL = process.env.BASE_URL || "http://127.0.0.1:8080";
  const DEVICE = (process.env.DEVICE || "desktop").toLowerCase();
  const isMobile = DEVICE.startsWith("m");
  const TIMEOUT_MS = Number(process.env.TIMEOUT_MS || 60000);

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
  if (executablePath) console.log(`[info] USING_EXECUTABLE=${executablePath}`);
  console.log(`[info] CONCURRENCY=${CONCURRENCY}`);

  let browser;
  try {
    // Some CI environments require --no-sandbox
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
      // Set UA tweak for mobile if desired later; viewport suffices for now
      await page.setViewport(viewport);
      while (true) {
        const i = index++;
        if (i >= total) break;
        const r = routes[i];
        const url = new URL(r, BASE_URL).toString();
        console.log(`[step] visiting ${url}`);
        const resp = await navigateWithFallback(page, url, TIMEOUT_MS);

        const status = resp ? resp.status() : -1;
        if (!resp || status >= 400) {
          throw new Error(`bad status ${status} for ${url}`);
        }

        // Ensure DOM/hydration settles briefly
        await page.waitForSelector("body", { timeout: 10000 }).catch(() => {});
        await new Promise((r) => setTimeout(r, 1000));

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
    console.log("[done] screenshots complete");
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
