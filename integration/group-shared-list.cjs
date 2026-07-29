#!/usr/bin/env node
/* eslint-disable no-console */
/**
 * Group-based Shared-list E2E. Requires the server to be built with `--features test-auth`.
 */

"use strict";

const USERS = {
  owner: { id: 890000000001, username: "GroupListOwner" },
  member: { id: 890000000002, username: "GroupListMember" },
  nonMember: { id: 890000000003, username: "GroupListNonMember" },
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
    const memberPage = await browser.newPage();
    const nonMemberPage = await browser.newPage();
    for (const page of [ownerPage, memberPage, nonMemberPage]) {
      page.setDefaultTimeout(TIMEOUT_MS);
    }

    await login(ownerPage, BASE_URL, USERS.owner);
    await login(memberPage, BASE_URL, USERS.member);
    await login(nonMemberPage, BASE_URL, USERS.nonMember);

    // 1. Owner creates a group
    const groupName = `Group E2E ${Date.now()}`;
    const createGroup = await api(ownerPage, "POST", "/api/v1/group/create", {
      name: groupName,
    });
    failIf(createGroup.status !== 200, failures, `create group expected 200, got ${createGroup.status}`);
    const groupId = createGroup.body.id;
    failIf(!groupId, failures, "group id missing after create");

    // 2. Owner adds member to group
    const addMember = await api(ownerPage, "POST", `/api/v1/group/${groupId}/member/add/${USERS.member.id}`);
    failIf(addMember.status !== 200, failures, `add member expected 200, got ${addMember.status}`);

    // 3. Owner creates a list
    const worldData = await api(ownerPage, "GET", "/api/v1/world_data");
    failIf(worldData.status !== 200, failures, `world_data expected 200, got ${worldData.status}`);
    const worldId = worldData.body.regions[0].datacenters[0].worlds[0].id;
    const listName = `Group Shared List ${Date.now()}`;

    const createList = await api(ownerPage, "POST", "/api/v1/list/create", {
      name: listName,
      wdr_filter: { World: worldId },
    });
    failIf(createList.status !== 200, failures, `create list expected 200, got ${createList.status}`);

    const ownerLists = await api(ownerPage, "GET", "/api/v1/list");
    const created = ownerLists.body.find((entry) => entry.list.name === listName);
    failIf(!created, failures, "created list not returned to owner");
    if (!created) throw new Error("cannot continue without created list");
    const listId = created.list.id;

    // 4. Owner shares list with group (Read permission = 1)
    const groupShare = await api(ownerPage, "POST", `/api/v1/list/${listId}/share/group`, {
      group_id: groupId,
      permission: "Read",
    });
    failIf(groupShare.status !== 200, failures, `group share expected 200, got ${groupShare.status}`);

    // 5. Member verifies they see the list
    const memberLists = await api(memberPage, "GET", "/api/v1/list");
    const memberList = memberLists.body.find((entry) => entry.list.id === listId);
    failIf(!memberList, failures, "group-shared list not returned to member");
    failIf(memberList && memberList.permission !== "Read", failures, `member permission was ${memberList && memberList.permission}, expected Read`);

    // 6. Non-member verifies they do NOT see the list
    const nonMemberLists = await api(nonMemberPage, "GET", "/api/v1/list");
    const nonMemberList = nonMemberLists.body.find((entry) => entry.list.id === listId);
    failIf(nonMemberList, failures, "group-shared list should not be returned to non-member");

    // 7. Member tries to add item (should fail with 403)
    const readAdd = await api(memberPage, "POST", `/api/v1/list/${listId}/add/item`, {
      id: 0,
      item_id: 2,
      list_id: listId,
      hq: null,
      quantity: 1,
      acquired: null,
    });
    failIf(readAdd.status !== 403, failures, `read-only member add expected 403, got ${readAdd.status}`);

    // 8. Owner updates group share to Write (Write permission = 2)
    const groupWriteShare = await api(ownerPage, "POST", `/api/v1/list/${listId}/share/group`, {
      group_id: groupId,
      permission: "Write",
    });
    failIf(groupWriteShare.status !== 200, failures, `group write share expected 200, got ${groupWriteShare.status}`);

    // 9. Member adds item (should succeed)
    const writeAdd = await api(memberPage, "POST", `/api/v1/list/${listId}/add/item`, {
      id: 0,
      item_id: 2,
      list_id: listId,
      hq: null,
      quantity: 1,
      acquired: null,
    });
    failIf(writeAdd.status !== 200, failures, `write-permission member add expected 200, got ${writeAdd.status}`);

    // Cleanup
    await api(ownerPage, "DELETE", `/api/v1/list/${listId}/delete`);
    await api(ownerPage, "DELETE", `/api/v1/group/${groupId}`);

    for (const page of [ownerPage, memberPage, nonMemberPage]) {
      await page.close();
    }
  } finally {
    await browser.close();
  }

  if (failures.length) {
    console.error(`[fail] ${failures.length} group-shared-list assertion(s) failed:`);
    for (const f of failures) console.error(`  - ${f}`);
    process.exit(1);
  }
  console.log("[ok] group-shared-list flow passed");
}

main().catch((err) => {
  console.error("[error]", err && err.stack ? err.stack : err);
  process.exit(1);
});
