#!/usr/bin/env node
"use strict";

// Crawler regression test: read raw SSR HTML without JavaScript, then fetch
// the advertised social image from this same local server. No account,
// Discord token, populated market database, or browser is required.
//
// BASE_URL=http://127.0.0.1:8080 npm --prefix integration run test:social-cards
// TIMEOUT_MS=120000 increases the per-request budget for a debug build.
// SOCIAL_LOCALES=en,ja selects a subset for a targeted repeat.
// SOCIAL_ARTIFACTS=1 saves inspected PNGs in integration/artifacts/social-cards.
// SOCIAL_HYDRATION=1 also checks a Japanese item fresh load and language switch
// in Puppeteer. Requires npm --prefix integration ci and a hydrated WASM build.
// SOCIAL_HYDRATION=only repeats the browser probe without rerunning the matrix.
// SOCIAL_VERBOSE=1 logs request stages when investigating an SSR stream stall.
//
// Absolute https://ultros.app image URLs are deliberately remapped to BASE_URL;
// neither the metadata's origin nor redirects may send this probe to production.

const assert = require("node:assert/strict");
const { createHash } = require("node:crypto");
const fs = require("node:fs/promises");
const path = require("node:path");

const BASE_URL = new URL(process.env.BASE_URL || "http://127.0.0.1:8080");
const TIMEOUT_MS = Number(process.env.TIMEOUT_MS || 60000);
const OG_LOCALES = {
  en: "en_US",
  ja: "ja_JP",
  de: "de_DE",
  fr: "fr_FR",
  ko: "ko_KR",
  cn: "zh_CN",
  tc: "zh_TW",
};
const LOCALES = (process.env.SOCIAL_LOCALES || Object.keys(OG_LOCALES).join(","))
  .split(",")
  .map((value) => value.trim());
const ROUTES = [
  { name: "home", page: "/", card: "home/default" },
  // ARR item shared by all regional game-data packs, including those behind
  // the current global patch. The newer approved item is checked separately.
  { name: "item", page: "/item/5333", card: "item/5333" },
  {
    name: "item-world",
    page: "/item/Gilgamesh/5333",
    card: "item/5333",
    world: "Gilgamesh",
  },
  { name: "jobset", page: "/items/jobset/SAM", card: "jobset/SAM" },
  { name: "currency", page: "/currency-exchange", card: "currency/default" },
  { name: "tool", page: "/flip-finder", card: "tool/flip-finder" },
  { name: "help", page: "/help", card: "help/default" },
];
const CRAWLER_HEADERS = {
  "User-Agent": "Twitterbot/1.0",
  Accept: "text/html",
};

function decodeHtml(value) {
  const entities = { amp: "&", quot: '"', apos: "'", lt: "<", gt: ">" };
  return value.replace(/&(#x[0-9a-f]+|#\d+|amp|quot|apos|lt|gt);/gi, (_, entity) => {
    if (entity[0] !== "#") return entities[entity.toLowerCase()];
    return String.fromCodePoint(
      entity[1].toLowerCase() === "x"
        ? Number.parseInt(entity.slice(2), 16)
        : Number.parseInt(entity.slice(1), 10),
    );
  });
}

function attributes(tag) {
  const result = {};
  const pattern = /([^\s=<>]+)\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s>]+))/g;
  for (const match of tag.matchAll(pattern)) {
    result[match[1].toLowerCase()] = decodeHtml(match[2] ?? match[3] ?? match[4]);
  }
  return result;
}

function readMetadata(html, label) {
  const head = html.match(/<head\b[^>]*>([\s\S]*?)<\/head>/i);
  assert.ok(head, `${label}: SSR response has no document head`);
  const metas = [...head[1].matchAll(/<meta\b[^>]*>/gi)].map(([tag]) => attributes(tag));
  function one(key, attribute = key.startsWith("og:") ? "property" : "name") {
    // Count incorrect name="og:*" tags too: they are still duplicate metadata
    // even when a particular scraper happens to ignore the wrong attribute.
    const matches = metas.filter((meta) => meta.property === key || meta.name === key);
    assert.equal(matches.length, 1, `${label}: expected exactly one ${key}, got ${matches.length}`);
    assert.equal(matches[0][attribute], key, `${label}: ${key} needs ${attribute} attribute`);
    assert.ok(matches[0].content?.trim(), `${label}: ${key} must not be empty`);
    return matches[0].content;
  }
  const result = Object.fromEntries(
    [
      "og:title", "og:description", "og:url", "og:locale", "og:image",
      "og:image:alt", "og:image:type", "og:image:width", "og:image:height",
      "twitter:title", "twitter:description", "twitter:image", "twitter:image:alt",
      "twitter:card",
    ].map((key) => [key, one(key)]),
  );
  assert.equal(result["og:title"], result["twitter:title"], `${label}: title parity`);
  assert.equal(result["og:description"], result["twitter:description"], `${label}: description parity`);
  assert.equal(result["og:image"], result["twitter:image"], `${label}: image parity`);
  assert.equal(result["og:image:alt"], result["twitter:image:alt"], `${label}: alt text parity`);
  assert.equal(result["twitter:card"], "summary_large_image", `${label}: large preview format`);
  assert.equal(result["og:image:type"], "image/png", `${label}: image media type`);
  assert.equal(result["og:image:width"], "1200", `${label}: image width metadata`);
  assert.equal(result["og:image:height"], "630", `${label}: image height metadata`);
  return result;
}

async function localFetch(relativePath, headers = {}) {
  const url = new URL(relativePath, BASE_URL);
  assert.equal(url.origin, BASE_URL.origin, `Probe must stay on the local server: ${url}`);
  try {
    if (process.env.SOCIAL_VERBOSE === "1") console.log(`GET ${url.pathname}${url.search}`);
    const response = await fetch(url, {
      headers,
      redirect: "error",
      signal: AbortSignal.timeout(TIMEOUT_MS),
    });
    if (process.env.SOCIAL_VERBOSE === "1") console.log(`HEADERS ${response.status} ${url.pathname}${url.search}`);
    return response;
  } catch (error) {
    throw new Error(`${url.pathname}${url.search}: ${error.message}`, { cause: error });
  }
}

async function metadataFor(route, locale, headers = {}) {
  const url = new URL(route.page, BASE_URL);
  if (locale) url.searchParams.set("lang", locale);
  const response = await localFetch(url, { ...CRAWLER_HEADERS, ...headers });
  assert.equal(response.status, 200, `${url.pathname}${url.search}: SSR HTTP status`);
  assert.match(response.headers.get("content-type") || "", /text\/html/i);
  let html;
  try {
    // Metadata is in the initial server-rendered head. Some existing market
    // routes keep streaming while unrelated resources settle; a crawler probe
    // should not wait for those body resources to validate this feature.
    const reader = response.body.getReader();
    const decoder = new TextDecoder();
    html = "";
    try {
      while (!/<\/head>/i.test(html)) {
        const chunk = await reader.read();
        if (chunk.done) break;
        html += decoder.decode(chunk.value, { stream: true });
        assert.ok(html.length < 2 * 1024 * 1024, "SSR head exceeded 2 MiB");
      }
    } finally {
      await reader.cancel();
    }
  } catch (error) {
    throw new Error(`${url.pathname}${url.search}: SSR response head: ${error.message}`, { cause: error });
  }
  return readMetadata(html, `${route.name}/${locale || "default"}`);
}

function imagePathFor(metadata, route, locale) {
  const image = new URL(metadata["og:image"]);
  assert.equal(image.protocol, "https:", `${route.name}/${locale}: image must be an absolute HTTPS URL`);
  assert.equal(image.pathname, `/social/v2/${locale}/${route.card}`, `${route.name}/${locale}: versioned localized image path`);
  assert.deepEqual(
    [...image.searchParams.entries()],
    route.world ? [["world", route.world]] : [],
    `${route.name}/${locale}: only stable market scope belongs in the image URL`,
  );
  const page = new URL(metadata["og:url"]);
  assert.equal(page.protocol, "https:", `${route.name}/${locale}: absolute sharing URL`);
  assert.equal(page.origin, image.origin, `${route.name}/${locale}: shared page and image origin`);
  assert.equal(page.pathname, route.page, `${route.name}/${locale}: sharing path`);
  assert.deepEqual([...page.searchParams.entries()], [["lang", locale]], `${route.name}/${locale}: sharing URL must fix the language`);
  return image.pathname + image.search;
}

async function readPng(imagePath, headers = {}, checkConditional = false) {
  const response = await localFetch(imagePath, { "User-Agent": CRAWLER_HEADERS["User-Agent"], ...headers });
  assert.equal(response.status, 200, `${imagePath}: PNG HTTP status`);
  assert.match(response.headers.get("content-type") || "", /^image\/png(?:;|$)/i, `${imagePath}: PNG Content-Type`);
  const cache = response.headers.get("cache-control") || "";
  assert.match(cache, /\bpublic\b/i, `${imagePath}: public image cache`);
  assert.match(cache, /\bmax-age=[1-9]\d*\b/i, `${imagePath}: positive cache lifetime`);
  assert.doesNotMatch(cache, /\b(?:private|no-store)\b/i, `${imagePath}: shareable image cache`);
  assert.equal(response.headers.get("set-cookie"), null, `${imagePath}: rendering must not create a session`);
  assert.doesNotMatch(response.headers.get("vary") || "", /\b(?:cookie|accept-language)\b/i, `${imagePath}: image URL determines its language`);
  const etag = response.headers.get("etag");
  assert.match(etag || "", /^(?:W\/)?".+"$/, `${imagePath}: image validator`);
  let png;
  try {
    png = Buffer.from(await response.arrayBuffer());
  } catch (error) {
    throw new Error(`${imagePath}: PNG response body: ${error.message}`, { cause: error });
  }
  assert.ok(png.length > 33, `${imagePath}: truncated PNG`);
  assert.equal(png.subarray(0, 8).toString("hex"), "89504e470d0a1a0a", `${imagePath}: PNG signature`);
  assert.equal(png.toString("ascii", 12, 16), "IHDR", `${imagePath}: first PNG chunk`);
  assert.equal(png.readUInt32BE(16), 1200, `${imagePath}: PNG width`);
  assert.equal(png.readUInt32BE(20), 630, `${imagePath}: PNG height`);
  if (checkConditional) {
    const conditional = await localFetch(imagePath, { "If-None-Match": etag });
    assert.equal(conditional.status, 304, `${imagePath}: unchanged image revalidation`);
    assert.equal(conditional.headers.get("etag"), etag, `${imagePath}: stable conditional validator`);
    assert.equal((await conditional.arrayBuffer()).byteLength, 0, `${imagePath}: 304 has no image body`);
  }
  return png;
}

function conflictingHeaders(locale) {
  const alternative = locale === "ja" ? "de" : "ja";
  return {
    Cookie: `i18n_pref_locale=${alternative}; HIDE_ADS=true`,
    "Accept-Language": alternative === "ja" ? "ja-JP,ja;q=0.9" : "de-DE,de;q=0.9",
  };
}

async function checkHydration() {
  const puppeteer = require("puppeteer");
  const browser = await puppeteer.launch({
    headless: true,
    executablePath: process.env.PUPPETEER_EXECUTABLE_PATH || undefined,
    args: ["--no-sandbox", "--disable-dev-shm-usage"],
  });
  const errors = [];
  let page;
  try {
    page = await browser.newPage();
    await page.setViewport({ width: 1440, height: 1000 });
    page.setDefaultTimeout(TIMEOUT_MS);
    await page.setCookie(
      { name: "i18n_pref_locale", value: "fr", url: BASE_URL.origin, path: "/" },
      { name: "HIDE_ADS", value: "true", url: BASE_URL.origin, path: "/" },
    );
    await page.setExtraHTTPHeaders({ "Accept-Language": "fr-FR,fr;q=0.9" });
    await page.setRequestInterception(true);
    page.on("request", (request) => {
      // Exercise the local app without contacting analytics, ads, or production.
      const url = new URL(request.url());
      if ((url.protocol === "http:" || url.protocol === "https:") && url.origin !== BASE_URL.origin) {
        void request.abort().catch(() => {});
      } else {
        void request.continue().catch(() => {});
      }
    });
    page.on("pageerror", (error) => errors.push(String(error)));
    page.on("console", (message) => {
      if (message.type() === "error" && /hydrat|panicked at|entered unreachable code/i.test(message.text())) {
        errors.push(message.text());
      }
    });
    await page.evaluateOnNewDocument(() => {
      window.__socialProbeHydrated = false;
      window.addEventListener("ultros:hydrated", () => { window.__socialProbeHydrated = true; });
    });
    const route = ROUTES.find((entry) => entry.name === "item");
    const response = await page.goto(new URL(`${route.page}?lang=ja`, BASE_URL).href, {
      waitUntil: "domcontentloaded",
      timeout: TIMEOUT_MS,
    });
    assert.equal(response.status(), 200, "hydration: item response status");
    const ssrHtml = await response.text();
    const ssrMetadata = readMetadata(ssrHtml, "item/ja before hydration");
    const ssrHeading = ssrHtml.match(/<h1\b[^>]*>([\s\S]*?)<\/h1>/i);
    assert.ok(ssrHeading, "hydration: SSR item name heading");
    const ssrItemName = decodeHtml(ssrHeading[1]
      .replace(/<!--[^]*?-->/g, "")
      .replace(/<button\b[^>]*>[^]*?<\/button>/gi, "")
      .replace(/<[^>]*>/g, ""))
      .trim();
    assert.ok(ssrItemName, "hydration: SSR item name is nonempty");
    assert.match(ssrItemName, /[\u3040-\u30ff\u3400-\u9fff]/u, "hydration: explicit Japanese URL must use Japanese game data in SSR");
    await page.waitForFunction(() => window.__socialProbeHydrated === true);
    await page.waitForSelector("h1");
    const hydratedItemName = await page.$eval("h1", (heading) => {
      const clone = heading.cloneNode(true);
      clone.querySelectorAll("button").forEach((button) => button.remove());
      return clone.textContent.trim();
    });
    assert.equal(hydratedItemName, ssrItemName, "hydration: Japanese game item name must survive hydration");
    assert.equal(await page.$eval("html", (html) => html.lang), "ja", "hydration: explicit page language");
    assert.deepEqual(readMetadata(await page.content(), "item/ja after hydration"), ssrMetadata, "hydration: crawler metadata survives hydration");
    console.log("PASS hydration: Japanese SSR item name, page language, and metadata survive the fresh browser load");

    await page.evaluate(() => { window.__socialProbeDocumentToken = "original-item-document"; });
    // Match the existing jobset hydration probe: an ordinary bubbling anchor
    // click exercises Leptos client navigation without calling an app setter.
    async function clientNavigate(destination) {
      await page.evaluate((href) => {
        const anchor = document.createElement("a");
        anchor.href = href;
        document.body.appendChild(anchor);
        anchor.dispatchEvent(new MouseEvent("click", {
          bubbles: true, cancelable: true, view: window, button: 0,
        }));
        anchor.remove();
      }, destination);
      await page.waitForFunction((pathname) => window.location.pathname === pathname, {}, destination);
      assert.equal(
        await page.evaluate(() => window.__socialProbeDocumentToken),
        "original-item-document",
        "hydration: route changes use client navigation without reloading the document",
      );
    }

    await clientNavigate("/settings");
    await page.waitForFunction(() => new URL(window.location.href).searchParams.get("lang") === "ja");
    // Use the full settings picker, which remains visible independently of the
    // sidebar's world and account popover stacking/positioning.
    await page.locator(() => [...document.querySelectorAll('button[role="radio"]')]
      .find((button) => button.textContent.includes("English")))
      .click();
    await page.waitForFunction(() => {
      const image = document.querySelector('meta[property="og:image"]')?.content || "";
      return document.documentElement.lang === "en"
        && new URL(window.location.href).searchParams.get("lang") === "en"
        && image.includes("/social/v2/en/home/default");
    });
    assert.deepEqual(
      readMetadata(await page.content(), "settings/en after switching"),
      await metadataFor(ROUTES[0], "en"),
      "hydration: settings language picker updates all social metadata",
    );
    const localeCookie = (await page.cookies(BASE_URL.origin))
      .find((cookie) => cookie.name === "i18n_pref_locale");
    assert.equal(localeCookie?.value, "en", "hydration: language picker saves the chosen locale");
    assert.equal(localeCookie.path, "/", "hydration: chosen locale persists across page paths");
    console.log("PASS hydration: real settings language picker updates page language, sharing metadata, URL, and locale cookie");

    await clientNavigate(route.page);
    await page.waitForFunction(() => {
      const image = document.querySelector('meta[property="og:image"]')?.content || "";
      return document.documentElement.lang === "en"
        && new URL(window.location.href).searchParams.get("lang") === "en"
        && image.includes("/social/v2/en/item/5333");
    });
    const englishMetadata = await metadataFor(route, "en");
    // Shared item-card content takes its title directly from the English game
    // item name; the metadata component adds only this fixed branding suffix.
    // Use that authoritative SSR name so a stale Settings/blank heading cannot
    // satisfy the client-navigation check while the item body is still loading.
    const brandSuffix = " · Ultros";
    assert.ok(englishMetadata["og:title"].endsWith(brandSuffix), "hydration: English item title format");
    const englishItemName = englishMetadata["og:title"].slice(0, -brandSuffix.length);
    assert.ok(englishItemName.trim(), "hydration: authoritative English item name is nonempty");
    await page.waitForFunction((expectedName) => {
      const heading = document.querySelector("h1");
      if (!heading) return false;
      const clone = heading.cloneNode(true);
      clone.querySelectorAll("button").forEach((button) => button.remove());
      return clone.textContent.trim() === expectedName;
    }, {}, englishItemName);
    assert.deepEqual(readMetadata(await page.content(), "item/en after switching"), englishMetadata, "hydration: switching language updates all social metadata");
    assert.deepEqual(errors, [], "hydration: no page errors or hydration panics");
    console.log("PASS hydration: chosen English locale and item metadata persist through client navigation back to the item");
  } catch (error) {
    if (page) {
      const state = await page.evaluate(() => ({
        url: window.location.href,
        lang: document.documentElement.lang,
        title: document.title,
        item: document.querySelector("h1")?.textContent,
        image: document.querySelector('meta[property="og:image"]')?.content,
        account: document.querySelector(".side-nav-account")?.outerHTML,
      })).catch(() => null);
      console.error("Hydration failure state:", JSON.stringify({ ...state, errors }));
      if (process.env.SOCIAL_ARTIFACTS === "1") {
        await page.screenshot({
          path: path.join(__dirname, "artifacts", "social-cards", "hydration-failure.png"),
          timeout: 10000,
        }).catch(() => {});
      }
    }
    throw error;
  } finally {
    await browser.close();
  }
}

async function main() {
  assert.ok(
    ["localhost", "127.0.0.1", "[::1]"].includes(BASE_URL.hostname),
    "Use a local BASE_URL (localhost, 127.0.0.1, or [::1]); this probe never runs against production.",
  );
  assert.ok(Number.isFinite(TIMEOUT_MS) && TIMEOUT_MS > 0, "TIMEOUT_MS must be positive");
  for (const locale of LOCALES) assert.ok(OG_LOCALES[locale], `Unsupported SOCIAL_LOCALES entry: ${locale}`);
  const artifactDirectory = path.join(__dirname, "artifacts", "social-cards");
  if (process.env.SOCIAL_ARTIFACTS === "1") await fs.mkdir(artifactDirectory, { recursive: true });
  if (process.env.SOCIAL_HYDRATION === "only") {
    await checkHydration();
    return;
  }
  const descriptions = new Map();
  let passed = 0;

  for (const locale of LOCALES) {
    for (const route of ROUTES) {
      const metadata = await metadataFor(route, locale);
      assert.equal(metadata["og:locale"], OG_LOCALES[locale], `${route.name}/${locale}: crawler language`);
      const imagePath = imagePathFor(metadata, route, locale);
      const cookieMetadata = await metadataFor(route, locale, conflictingHeaders(locale));
      assert.deepEqual(cookieMetadata, metadata, `${route.name}/${locale}: explicit language overrides cookie and Accept-Language`);
      const image = await readPng(imagePath, {}, route.name === "home");
      const cookieImage = await readPng(imagePath, conflictingHeaders(locale));
      assert.equal(
        createHash("sha256").update(cookieImage).digest("hex"),
        createHash("sha256").update(image).digest("hex"),
        `${route.name}/${locale}: image bytes must not depend on cookies or Accept-Language`,
      );
      if (process.env.SOCIAL_ARTIFACTS === "1") {
        await fs.writeFile(path.join(artifactDirectory, `${route.name}-${locale}.png`), image);
      }
      descriptions.set(`${route.name}/${locale}`, metadata["og:title"] + "\n" + metadata["og:description"]);
      passed++;
      console.log(`PASS ${route.name}/${locale}: unique SSR metadata, explicit language, deterministic 1200x630 PNG`);
    }
  }

  // A URL with no explicit language has English social metadata for every
  // crawler. A visitor's preferences can still localize the normal page UI.
  for (const route of ROUTES) {
    const english = await metadataFor(route, "en");
    const defaultMetadata = await metadataFor(route, undefined, conflictingHeaders("en"));
    assert.deepEqual(defaultMetadata, english, `${route.name}/default: localized cookies must not change a language-free share`);
  }
  console.log("PASS unlocalized sharing: English metadata is independent of visitor language preferences");

  const japaneseHome = await metadataFor(ROUTES[0], "ja");
  const privateFallback = await metadataFor(
    { name: "private-settings", page: "/settings?token=social-test-marker" },
    "ja",
  );
  assert.deepEqual(privateFallback, japaneseHome, "private pages share the localized homepage card without reflecting path or query state");
  console.log("PASS private fallback: no account path or query state in social metadata");

  const tool = ROUTES.find((route) => route.name === "tool");
  const toolMetadata = await metadataFor(tool, "en");
  const filteredTool = await metadataFor(
    { name: "tool-filtered", page: "/flip-finder?world=Gilgamesh&sort=profit&gain=12345" },
    "en",
  );
  assert.deepEqual(filteredTool, toolMetadata, "transient tool filters do not create a different cached preview");
  console.log("PASS evergreen tool: transient filters stay out of sharing metadata");

  // Detect an accidentally untranslated metadata path while allowing proper
  // nouns and item names to coincide across languages.
  if (LOCALES.includes("en")) {
    for (const locale of LOCALES.filter((value) => value !== "en")) {
      for (const route of ROUTES) {
        assert.notEqual(
          descriptions.get(`${route.name}/${locale}`),
          descriptions.get(`${route.name}/en`),
          `${route.name}/${locale}: title and description cannot both fall back to English`,
        );
      }
    }
  }

  const legacyImage = await readPng("/itemcard/North-America/49318");
  const versionedImage = await readPng("/social/v2/en/item/49318?world=North-America");
  assert.equal(
    createHash("sha256").update(legacyImage).digest("hex"),
    createHash("sha256").update(versionedImage).digest("hex"),
    "legacy itemcard URL renders the same image as its versioned English replacement",
  );
  console.log("PASS legacy itemcard: byte-for-byte parity with versioned English North-America card");

  const invalidPaths = [
    "/social/v2/xx/home/default",
    "/social/v2/en/not-a-card/default",
    "/social/v2/en/item/not-an-item",
    "/social/v2/en/item/2147483647",
    "/social/v2/en/jobset/NOTAJOB",
    "/social/v2/en/tool/not-a-tool",
    "/social/v2/en/item/5333?world=NotAWorld",
    "/itemcard/NotAWorld/49318",
  ];
  for (const invalid of invalidPaths) {
    const response = await localFetch(invalid);
    await response.arrayBuffer();
    assert.ok(response.status >= 400 && response.status < 500, `${invalid}: expected 4xx, got ${response.status}`);
    console.log(`PASS ${invalid}: rejects invalid card request (${response.status})`);
  }
  if (process.env.SOCIAL_HYDRATION === "1") await checkHydration();
  console.log(`Social cards passed: ${passed} page/locale combinations, legacy parity, and ${invalidPaths.length} invalid requests.`);
}

main().catch((error) => {
  console.error(`Social-card regression failed: ${error.stack || error}`);
  process.exitCode = 1;
});
