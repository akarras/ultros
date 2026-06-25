#!/usr/bin/env node
/* eslint-disable no-console */
/**
 * Focused Puppeteer E2E for the FC Crafting Analyzer material breakdown.
 *
 * Regression covered:
 *   - clicking "Material breakdown" must not follow the item link
 *   - the native details element must open
 *   - the virtual-scroller row must grow instead of clipping the expanded body
 *
 * Env:
 *   BASE_URL   default http://127.0.0.1:8080
 *   WORLD      default Goblin
 *   HEADLESS   "new" | "true" | "false" (default "new")
 *   TIMEOUT_MS default 90000
 */

"use strict";

const fs = require("fs");
const path = require("path");
const puppeteer = require("puppeteer");

function parseHeadless(value) {
  if (value === undefined || value === null || value === "") return "new";
  const v = String(value).toLowerCase();
  if (v === "new") return "new";
  if (v === "true" || v === "1") return true;
  if (v === "false" || v === "0") return false;
  return "new";
}

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

async function materialSummaryHandle(page) {
  const handle = await page.evaluateHandle(() => {
    return (
      Array.from(document.querySelectorAll("summary")).find((el) =>
        (el.textContent || "").includes("Material breakdown"),
      ) || null
    );
  });
  const element = handle.asElement();
  if (!element) await handle.dispose();
  return element;
}

async function breakdownState(page) {
  return page.evaluate(() => {
    const summary = Array.from(document.querySelectorAll("summary")).find((el) =>
      (el.textContent || "").includes("Material breakdown"),
    );
    if (!summary) return { found: false };

    const details = summary.closest("details");
    const body = details && details.querySelector(".mt-2");
    const rowWrapper = details && details.closest(".content-auto, .content-visible");
    const detailsRect = details
      ? details.getBoundingClientRect()
      : { height: 0, bottom: 0 };
    const bodyRect = body ? body.getBoundingClientRect() : { height: 0 };
    const rowRect = rowWrapper
      ? rowWrapper.getBoundingClientRect()
      : { height: 0, bottom: 0 };

    return {
      found: true,
      open: Boolean(details && details.open),
      detailsHeight: detailsRect.height,
      bodyHeight: bodyRect.height,
      rowHeight: rowRect.height,
      clipped:
        Boolean(rowWrapper) && detailsRect.bottom > rowRect.bottom + 1,
      rowClass: rowWrapper ? rowWrapper.className : "",
      rowStyle: rowWrapper ? rowWrapper.getAttribute("style") || "" : "",
      text: body ? body.textContent || "" : "",
    };
  });
}

async function main() {
  const BASE_URL = process.env.BASE_URL || "http://127.0.0.1:8080";
  const WORLD = process.env.WORLD || "Goblin";
  const TIMEOUT_MS = Number(process.env.TIMEOUT_MS || 90000);
  const headless = parseHeadless(process.env.HEADLESS);
  const consoleAllow = [
    ...DEFAULT_CONSOLE_ALLOW,
    ...(process.env.CONSOLE_ALLOW || "")
      .split(",")
      .map((s) => s.trim())
      .filter(Boolean),
  ];
  const outdir = path.resolve(__dirname, "artifacts", "fc-crafting");
  fs.mkdirSync(outdir, { recursive: true });

  const route = `/fc-crafting-analyzer/${encodeURIComponent(WORLD)}`;
  const url = new URL(route, BASE_URL).toString();
  const base = new URL(BASE_URL);

  console.log(`[info] BASE_URL=${BASE_URL}`);
  console.log(`[info] WORLD=${WORLD}`);
  console.log(`[info] OUTPUT_DIR=${outdir}`);

  const consoleErrors = [];
  const pageErrors = [];
  const isAllowed = (message) => consoleAllow.some((s) => message.includes(s));

  let browser;
  try {
    browser = await puppeteer.launch({
      headless,
      args: ["--no-sandbox", "--disable-setuid-sandbox"],
      executablePath: process.env.PUPPETEER_EXECUTABLE_PATH || undefined,
    });

    const page = await browser.newPage();
    page.setDefaultTimeout(TIMEOUT_MS);
    await page.setViewport({ width: 1280, height: 800, deviceScaleFactor: 1 });
    await page.setCookie({
      name: "HOME_WORLD",
      value: WORLD,
      domain: base.hostname,
      path: "/",
    });

    page.on("console", (msg) => {
      if (msg.type() === "error") {
        const text = msg.text();
        if (!isAllowed(text)) consoleErrors.push(text);
      }
    });
    page.on("pageerror", (err) => {
      const text = (err && err.stack) || String(err);
      if (!isAllowed(text)) pageErrors.push(text);
    });

    console.log(`[step] visiting ${url}`);
    const resp = await navigateWithFallback(page, url, TIMEOUT_MS);
    const status = resp ? resp.status() : -1;
    if (!resp || status >= 400) {
      throw new Error(`bad status ${status} for ${url}`);
    }

    await page.waitForFunction(
      () =>
        Array.from(document.querySelectorAll("summary")).some((el) =>
          (el.textContent || "").includes("Material breakdown"),
        ),
      { timeout: TIMEOUT_MS },
    );
    await new Promise((r) => setTimeout(r, 500));
    await page.screenshot({
      path: path.join(outdir, "before-breakdown.png"),
      fullPage: true,
    });

    const beforeUrl = page.url();
    const before = await breakdownState(page);
    if (!before.found) throw new Error("material breakdown summary not found");
    if (before.open) throw new Error("material breakdown unexpectedly starts open");

    const summary = await materialSummaryHandle(page);
    if (!summary) throw new Error("material breakdown summary handle not found");
    await summary.evaluate((el) =>
      el.scrollIntoView({ block: "center", inline: "nearest" }),
    );
    await summary.click();
    await summary.dispose();

    await Promise.race([
      page.waitForNavigation({ waitUntil: "domcontentloaded", timeout: 1500 }).catch(() => null),
      new Promise((r) => setTimeout(r, 800)),
    ]);

    const afterUrl = page.url();
    if (afterUrl !== beforeUrl) {
      throw new Error(
        `clicking material breakdown navigated away: before=${beforeUrl} after=${afterUrl}`,
      );
    }
    if (new URL(afterUrl).pathname.startsWith("/item/")) {
      throw new Error(`clicking material breakdown opened an item page: ${afterUrl}`);
    }

    await page.waitForFunction(
      () => {
        const summary = Array.from(document.querySelectorAll("summary")).find((el) =>
          (el.textContent || "").includes("Material breakdown"),
        );
        const details = summary && summary.closest("details");
        const body = details && details.querySelector(".mt-2");
        const rowWrapper =
          details && details.closest(".content-auto, .content-visible");
        if (!details || !body || !rowWrapper || !details.open) return false;
        const bodyRect = body.getBoundingClientRect();
        const detailsRect = details.getBoundingClientRect();
        const rowRect = rowWrapper.getBoundingClientRect();
        return (
          bodyRect.height > 0 &&
          rowRect.height > 60 &&
          detailsRect.bottom <= rowRect.bottom + 1
        );
      },
      { timeout: 10000 },
    );

    const after = await breakdownState(page);
    await page.screenshot({
      path: path.join(outdir, "after-breakdown-open.png"),
      fullPage: true,
    });

    if (!after.open) throw new Error("material breakdown did not open");
    if (after.bodyHeight <= 0) {
      throw new Error(`material breakdown body has no visible height: ${JSON.stringify(after)}`);
    }
    if (after.rowHeight <= 60) {
      throw new Error(`virtual row did not grow after opening: ${JSON.stringify(after)}`);
    }
    if (after.clipped) {
      throw new Error(`material breakdown is clipped by the virtual row: ${JSON.stringify(after)}`);
    }
    if (!/\d+\s*x\s+/.test(after.text)) {
      throw new Error(`material rows did not render expected quantity text: ${JSON.stringify(after)}`);
    }

    if (consoleErrors.length || pageErrors.length) {
      const lines = [
        ...consoleErrors.map((e) => `console.error: ${e}`),
        ...pageErrors.map((e) => `pageerror: ${e}`),
      ];
      throw new Error(lines.join("\n"));
    }

    console.log("[done] FC crafting material breakdown opens in-place and is not clipped");
    await browser.close();
    browser = null;
  } catch (err) {
    console.error("[error]", err && err.stack ? err.stack : err);
    if (browser) {
      try {
        await browser.close();
      } catch (_) {}
    }
    process.exitCode = 1;
  }
}

main();
