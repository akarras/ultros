#!/usr/bin/env node
/* eslint-disable no-console */
/**
 * Cross-platform Puppeteer runner for Ultros (Leptos) E2E screenshots + asserts.
 *
 * Env:
 *  - BASE_URL: base address of the running server (default http://127.0.0.1:8080)
 *  - DEVICE:   "mobile", "desktop", or "wide" (default "desktop"). "wide" is a
 *              2560px pass: the ad rail only mounts at >=1536px and its slot
 *              once overflowed the document at >=1660px (issue #1234), which
 *              the 1280px desktop pass could never see.
 *  - ROUTES:   comma-separated list of routes to visit (default built-in list)
 *  - TIMEOUT_MS: navigation timeout in ms (default 60000)
 *  - HEADLESS: "new" | "true" | "false" (default "new")
 *  - PUPPETEER_EXECUTABLE_PATH: path to Chrome/Chromium binary (optional)
 *  - CONCURRENCY: number of parallel pages to run (default 16)
 *  - STRICT_CONSOLE: "1" to fail on console errors / page errors (default "1")
 *  - CONSOLE_ALLOW: comma-separated substrings to ignore in console errors
 *  - SKIP_ASSERTS: "1" to skip per-route content assertions (default "0")
 *  - SKIP_OVERFLOW: "1" to skip the horizontal-overflow assertion (default "0")
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
  "/items": { titleIncludes: "Items Explorer" },
  "/item/46010": {
    titleIncludes: "Ceremonial Shamshir",
    // The freshness verdict words ("Fresh"/"Caution"/"Verify In-Game"/"No Data")
    // are no longer visible text — FreshnessBadge shows the data age ("Data 1h
    // 17m old") and moved the verdict into its `title` tooltip, which innerText
    // never sees. Assert on the sales-cadence badge sitting beside it instead:
    // same market-data header, and its four labels cover every data state
    // (including "no sales"), so this still proves the page rendered its data.
    bodyIncludesAny: [
      "Fast mover",
      "Steady mover",
      "Slow mover",
      "Not enough data",
    ],
  },
  "/items/category/Gunbreaker's Arms": { titleIncludes: "Gunbreaker" },
  "/flip-finder": { titleIncludes: "Ultros" },
  "/flip-finder/Gilgamesh": { titleIncludes: "Gilgamesh" },
  "/list": { titleIncludes: "Ultros" },
  "/retainers": { titleIncludes: "Ultros" },
  "/currency-exchange": { titleIncludes: "Ultros" },
  "/recipe-analyzer?world=Gilgamesh": { titleIncludes: "Recipe Analyzer" },
  // With the lab on, the Profit header carries an "after 5% tax" sub-label
  // at every width. The strip row itself is md+ only, and the mobile pass
  // reads innerText, which drops display:none content.
  "/recipe-analyzer?world=Gilgamesh&labs=analyzer-ledger": {
    titleIncludes: "Recipe Analyzer",
    bodyIncludesAny: ["after 5% tax"],
  },
  // Both labs on with the four Phase D columns requested. The new columns
  // are md+ only, so the only cross-device assertion is the title; the
  // sweep still checks console errors and horizontal overflow.
  "/recipe-analyzer?world=Gilgamesh&labs=analyzer-ledger,analyzer-signal-columns&cols=confidence,cost-sale-median,rev-sale-median,hop-gain,hop-worlds": {
    titleIncludes: "Recipe Analyzer",
  },
  "/history": { titleIncludes: "Ultros" },
  "/settings": { titleIncludes: "Ultros" },
  "/groups": { titleIncludes: "Groups", bodyIncludesAny: ["Groups", "No groups found"] },
  // Both legal pages set their own <MetaTitle> now, so neither falls back to the
  // app-default title that carries "Ultros". Assert the page's own name instead.
  "/privacy": { titleIncludes: "Privacy Policy", bodyIncludesAny: ["privacy", "Privacy"] },
  "/cookie-policy": { titleIncludes: "Cookie Policy", bodyIncludesAny: ["cookie", "Cookie"] },
};

/**
 * Routes known to overflow horizontally right now. Each entry names the
 * `devices` it applies to and the `reason`.
 *
 * These are *not* silently tolerated: a listed route that has stopped
 * overflowing fails the run, so a fix cannot land without also deleting its
 * exception. Scope `devices` as narrowly as the bug actually is — a route
 * exempted at a width where it already fits would mask a future regression
 * there. Keep this empty.
 */
const KNOWN_OVERFLOW = {};

// Substrings in console errors that we always ignore (third-party noise, expected hydration churn).
const DEFAULT_CONSOLE_ALLOW = [
  "favicon",
  "ERR_BLOCKED_BY_CLIENT", // ad/tracker blockers
  "net::ERR_ABORTED",       // navigation aborts during fast clicks
  "googlesyndication.com",  // AdSense vendor script throws in headless Chrome
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
    "/recipe-analyzer?world=Gilgamesh",
    "/recipe-analyzer?world=Gilgamesh&labs=analyzer-ledger",
    "/recipe-analyzer?world=Gilgamesh&labs=analyzer-ledger,analyzer-signal-columns&cols=confidence,cost-sale-median,rev-sale-median,hop-gain,hop-worlds",
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

/**
 * Assert the page itself does not scroll horizontally.
 *
 * `html` is `overflow-x: hidden`, so a document wider than the viewport is not
 * merely ugly — the surplus is clipped with no scrollbar and no wrap, i.e.
 * simply unreachable. That is how #1055 presented: the Flip Finder's "Columns"
 * and "Clear all" controls rendered outside the viewport.
 *
 * The assertion is on `documentElement` deliberately. Several surfaces are
 * legitimately wider than the viewport and scroll inside their own scrollport
 * (`.analyzer-hscroll` for the Flip Finder grid, `.filter-chip-row`,
 * `.item-explorer-chip-row`); measuring descendants would flag all of them.
 *
 * Caveat for whoever extends this: headless Chrome uses overlay scrollbars, so
 * `100vw === clientWidth` here. A page laid out against `100vw` while the real
 * browser reserves a classic scrollbar gutter — the other half of #1082 — is
 * invisible to this check. It guards content overflow, not viewport-unit
 * mistakes.
 */
async function checkHorizontalOverflow(page, route, device) {
  const result = await page.evaluate(() => {
    const doc = document.documentElement;
    const limit = doc.clientWidth + 1;
    const offenders = [];

    for (const el of document.querySelectorAll("body *")) {
      const rect = el.getBoundingClientRect();
      if (rect.width === 0 || rect.height === 0) continue;
      if (rect.right <= limit) continue;

      const style = getComputedStyle(el);
      // Viewport-anchored elements (toasts, full-bleed background layers) get
      // stretched *by* an overflowing document rather than causing it.
      if (style.position === "fixed") continue;

      // Anything inside a horizontal scrollport is scrollable-to, not clipped.
      let parent = el.parentElement;
      let contained = false;
      while (parent && parent !== document.body) {
        const overflowX = getComputedStyle(parent).overflowX;
        if (overflowX !== "visible") {
          contained = true;
          break;
        }
        parent = parent.parentElement;
      }
      if (contained) continue;

      const cls = typeof el.className === "string" ? el.className : "";
      offenders.push({
        desc: (el.tagName.toLowerCase() + (cls ? `.${cls}` : "")).slice(0, 90),
        right: Math.round(rect.right),
      });
    }

    offenders.sort((a, b) => b.right - a.right);
    return {
      scrollWidth: doc.scrollWidth,
      clientWidth: doc.clientWidth,
      offenders: offenders.slice(0, 5),
    };
  });

  const surplus = result.scrollWidth - result.clientWidth;
  const overflows = surplus > 1;
  const entry = KNOWN_OVERFLOW[route];
  const known = entry && entry.devices.includes(device) ? entry : null;

  if (known) {
    if (overflows) {
      console.warn(
        `[warn] ${route} [${device}]: known horizontal overflow (${surplus}px) — ${known.reason}`,
      );
      return [];
    }
    return [
      `horizontal overflow check: no longer overflows on ${device} — drop "${device}" from ` +
        `the KNOWN_OVERFLOW entry for "${route}" (was: ${known.reason})`,
    ];
  }

  if (!overflows) return [];

  const blame = result.offenders.length
    ? `; widest offenders: ${result.offenders
        .map((o) => `${o.desc} (right=${o.right})`)
        .join(", ")}`
    : "";
  return [
    `horizontal overflow: document is ${surplus}px wider than the viewport ` +
      `(scrollWidth ${result.scrollWidth} vs clientWidth ${result.clientWidth}) — ` +
      `html{overflow-x:hidden} clips the surplus, so it cannot be scrolled to${blame}`,
  ];
}

async function main() {
  const puppeteer = require("puppeteer");

  const BASE_URL = process.env.BASE_URL || "http://127.0.0.1:8080";
  const DEVICE = (process.env.DEVICE || "desktop").toLowerCase();
  const isMobile = DEVICE.startsWith("m");
  const isWide = DEVICE.startsWith("w");
  const TIMEOUT_MS = Number(process.env.TIMEOUT_MS || 60000);
  const STRICT_CONSOLE = envFlag("STRICT_CONSOLE", true);
  const SKIP_ASSERTS = envFlag("SKIP_ASSERTS", false);
  const SKIP_OVERFLOW = envFlag("SKIP_OVERFLOW", false);
  // All device passes share this runner, and the same route can fit at one
  // width and not the other, so failures name the width they were seen at.
  const DEVICE_LABEL = isMobile ? "mobile" : isWide ? "wide" : "desktop";
  const userAllow = (process.env.CONSOLE_ALLOW || "")
    .split(",")
    .map((s) => s.trim())
    .filter(Boolean);
  const consoleAllow = [...DEFAULT_CONSOLE_ALLOW, ...userAllow];

  const viewport = isMobile
    ? { width: 390, height: 844, isMobile: true, deviceScaleFactor: 2 }
    : isWide
      ? { width: 2560, height: 1200, deviceScaleFactor: 1 }
      : { width: 1280, height: 800, deviceScaleFactor: 1 };

  const headless = parseHeadless(process.env.HEADLESS);
  const executablePath = process.env.PUPPETEER_EXECUTABLE_PATH || undefined;
  const routes = getRoutes();
  const CONCURRENCY = Math.max(1, Number(process.env.CONCURRENCY || 16));

  const outdir = path.resolve(__dirname, "artifacts");
  fs.mkdirSync(outdir, { recursive: true });

  console.log(`[info] BASE_URL=${BASE_URL}`);
  console.log(`[info] DEVICE=${DEVICE_LABEL}`);
  console.log(`[info] OUTPUT_DIR=${outdir}`);
  console.log(`[info] HEADLESS=${headless}`);
  console.log(
    `[info] STRICT_CONSOLE=${STRICT_CONSOLE} SKIP_ASSERTS=${SKIP_ASSERTS} SKIP_OVERFLOW=${SKIP_OVERFLOW}`,
  );
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

        // Applies to every route, not just the ones with content assertions.
        if (!SKIP_OVERFLOW) {
          // AdSense never fills in headless Chrome, so the Ad component marks
          // itself `.ad.hidden` and `:has(.ad.hidden)` collapses the whole ad
          // rail — which once overflowed the document by 16px at >=1660px
          // (issue #1234) while looking fine to this check. Un-hide the
          // placeholder so the rail lays out the way it does for a real
          // viewer with a served ad, then measure.
          if (isWide) {
            await page.evaluate(() => {
              for (const ad of document.querySelectorAll(".ad.hidden")) {
                ad.classList.remove("hidden");
              }
            });
          }
          const fails = await checkHorizontalOverflow(page, r, DEVICE_LABEL);
          for (const f of fails) failures.push(`${r} [${DEVICE_LABEL}]: ${f}`);
        }

        if (STRICT_CONSOLE) {
          const newConsole = consoleErrors.slice(beforeConsole);
          const newPage = pageErrors.slice(beforePage);
          for (const e of newConsole) failures.push(`${r}: console.error: ${e.text}`);
          for (const e of newPage) failures.push(`${r}: page error: ${e.text}`);
        }

        const safe = sanitizeFileComponent(r);
        const filename = `${safe}-${DEVICE_LABEL}.png`;
        const file = path.join(outdir, filename);

        // Chrome refuses to capture past an internal bitmap limit, and a route
        // whose list is long enough (the Flip Finder grid against a data-rich
        // instance) trips it. That used to reject out of the worker and take
        // Promise.all — i.e. every remaining route's assertions — down with it.
        // A screenshot is a diagnostic; it must not decide whether the suite runs.
        try {
          await page.screenshot({ path: file, fullPage: true });
          console.log(`[ok] ${url} -> ${file}`);
        } catch (e) {
          console.warn(
            `[warn] ${r}: full-page screenshot failed (${e && e.message}); capturing viewport only`,
          );
          try {
            await page.screenshot({ path: file });
            console.log(`[ok] ${url} -> ${file} (viewport only)`);
          } catch (e2) {
            console.warn(`[warn] ${r}: viewport screenshot also failed (${e2 && e2.message})`);
          }
        }
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
