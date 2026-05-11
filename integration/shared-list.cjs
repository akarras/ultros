#!/usr/bin/env node
/* eslint-disable no-console */
/**
 * Shared-list E2E. Requires the server to be built with `--features test-auth`.
 */

"use strict";

const USERS = {
  owner: { id: 880000000001, username: "SharedListOwner" },
  reader: { id: 880000000002, username: "SharedListReader" },
  invited: { id: 880000000003, username: "SharedListInvited" },
  overLimit: { id: 880000000004, username: "SharedListOverLimit" },
};

async function login(page, baseUrl, user) {
  const loginUrl = new URL("/test/login", baseUrl);
  loginUrl.searchParams.set("user_id", String(user.id));
  loginUrl.searchParams.set("username", user.username);
  loginUrl.searchParams.set("redirect", "/");
  const resp = await page.goto(loginUrl.toString(), { waitUntil: "domcontentloaded" });
  if (!resp || resp.status() >= 400) {
    throw new Error(`test login failed for ${user.username}: ${resp ? resp.status() : -1}`);
  }
}

async function api(page, method, path, body) {
  return page.evaluate(
    async ({ method, path, body }) => {
      const resp = await fetch(path, {
        method,
        credentials: "include",
        headers: body === undefined ? {} : { "Content-Type": "application/json" },
        body: body === undefined ? undefined : JSON.stringify(body),
      });
      const text = await resp.text();
      let parsed = null;
      try {
        parsed = text ? JSON.parse(text) : null;
      } catch (_) {
        parsed = text;
      }
      return { status: resp.status, body: parsed, text };
    },
    { method, path, body },
  );
}

function failIf(condition, failures, message) {
  if (condition) failures.push(message);
}

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
    const ownerPage = await browser.newPage();
    const readerPage = await browser.newPage();
    const invitedPage = await browser.newPage();
    const overLimitPage = await browser.newPage();
    for (const page of [ownerPage, readerPage, invitedPage, overLimitPage]) {
      page.setDefaultTimeout(TIMEOUT_MS);
    }

    await login(ownerPage, BASE_URL, USERS.owner);
    await login(readerPage, BASE_URL, USERS.reader);
    await login(invitedPage, BASE_URL, USERS.invited);
    await login(overLimitPage, BASE_URL, USERS.overLimit);

    const worldData = await api(ownerPage, "GET", "/api/v1/world_data");
    failIf(worldData.status !== 200, failures, `world_data expected 200, got ${worldData.status}`);
    const worldId = worldData.body.regions[0].datacenters[0].worlds[0].id;
    const name = `Shared E2E ${Date.now()}`;

    const create = await api(ownerPage, "POST", "/api/v1/list/create", {
      name,
      wdr_filter: { World: worldId },
    });
    failIf(create.status !== 200, failures, `create list expected 200, got ${create.status}`);

    const ownerLists = await api(ownerPage, "GET", "/api/v1/list");
    const created = ownerLists.body.find((entry) => entry.list.name === name);
    failIf(!created, failures, "created list not returned to owner");
    if (!created) throw new Error("cannot continue without created list");
    failIf(created.permission !== "Owner", failures, `owner permission was ${created.permission}`);
    const listId = created.list.id;

    const readShare = await api(ownerPage, "POST", `/api/v1/list/${listId}/share/user`, {
      user_id: USERS.reader.id,
      permission: "Read",
    });
    failIf(readShare.status !== 200, failures, `read share expected 200, got ${readShare.status}`);

    const readerLists = await api(readerPage, "GET", "/api/v1/list");
    const readerList = readerLists.body.find((entry) => entry.list.id === listId);
    failIf(!readerList, failures, "read-shared list not returned to reader");
    failIf(readerList && readerList.permission !== "Read", failures, `reader permission was ${readerList && readerList.permission}`);

    const readAdd = await api(readerPage, "POST", `/api/v1/list/${listId}/add/item`, {
      id: 0,
      item_id: 2,
      list_id: listId,
      hq: null,
      quantity: 1,
      acquired: null,
    });
    failIf(readAdd.status !== 403, failures, `read-only add expected 403, got ${readAdd.status}`);

    const writeShare = await api(ownerPage, "POST", `/api/v1/list/${listId}/share/user`, {
      user_id: USERS.reader.id,
      permission: "Write",
    });
    failIf(writeShare.status !== 200, failures, `write share expected 200, got ${writeShare.status}`);

    const writeAdd = await api(readerPage, "POST", `/api/v1/list/${listId}/add/item`, {
      id: 0,
      item_id: 2,
      list_id: listId,
      hq: null,
      quantity: 1,
      acquired: null,
    });
    failIf(writeAdd.status !== 200, failures, `write add expected 200, got ${writeAdd.status}`);

    const invite = await api(ownerPage, "POST", `/api/v1/list/${listId}/invite/create`, {
      permission: "Read",
      max_uses: 1,
    });
    failIf(invite.status !== 200, failures, `create invite expected 200, got ${invite.status}`);
    const inviteId = invite.body.id;
    failIf(!inviteId || inviteId.length < 32, failures, "invite id was missing or too short");

    const inviteUse = await api(invitedPage, "POST", `/api/v1/invite/${inviteId}/use`);
    failIf(inviteUse.status !== 200, failures, `invite use expected 200, got ${inviteUse.status}`);
    failIf(inviteUse.body !== listId, failures, `invite returned list ${inviteUse.body}, expected ${listId}`);

    const overLimitUse = await api(overLimitPage, "POST", `/api/v1/invite/${inviteId}/use`);
    failIf(overLimitUse.status !== 400, failures, `over-limit invite expected 400, got ${overLimitUse.status}`);

    for (const page of [ownerPage, readerPage, invitedPage, overLimitPage]) {
      await page.close();
    }
  } finally {
    await browser.close();
  }

  if (failures.length) {
    console.error(`[fail] ${failures.length} shared-list assertion(s) failed:`);
    for (const f of failures) console.error(`  - ${f}`);
    process.exit(1);
  }
  console.log("[ok] shared-list flow passed");
}

main().catch((err) => {
  console.error("[error]", err && err.stack ? err.stack : err);
  process.exit(1);
});
