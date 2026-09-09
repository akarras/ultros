#!/usr/bin/env node
"use strict";

// Requires an isolated test-auth server. Creates its own groups and list;
// exercises real APIs, SSR, hydration, and client-side navigation.
const assert = require("node:assert/strict");
const puppeteer = require("puppeteer");

async function main() {
  const base = process.env.BASE_URL || "http://127.0.0.1:8080";
  const timeout = Number(process.env.TIMEOUT_MS || 30000);
  const browser = await puppeteer.launch({ headless: true, args: ["--no-sandbox"] });
  const errors = [];
  const seed = Date.now() * 1000 + Math.floor(Math.random() * 100);
  const ownerId = String(seed);
  const memberId = String(seed + 1);
  const memberName = `GroupsMember${seed}`;
  const groupName = `Groups gate ${seed}`;
  let groupId;
  let listId;
  let owner;

  async function newPage() {
    const context = await browser.createBrowserContext();
    const page = await context.newPage();
    page.setDefaultTimeout(timeout);
    await page.setViewport({ width: 1280, height: 900 });
    await page.setCookie(
      { name: "HIDE_ADS", value: "true", url: base, path: "/" },
      { name: "i18n_pref_locale", value: "en", url: base, path: "/" },
    );
    await page.evaluateOnNewDocument(() => {
      window.__groupsHydrated = false;
      window.addEventListener("ultros:hydrated", () => { window.__groupsHydrated = true; });
    });
    page.on("pageerror", error => errors.push(error.message));
    page.on("console", message => {
      if (message.type() === "error") console.error("Browser console:", message.text());
    });
    return page;
  }

  async function load(page, path) {
    const response = await page.goto(new URL(path, base).toString(), { waitUntil: "domcontentloaded" });
    assert(response.ok(), `SSR ${path}: ${response.status()}`);
    const html = await response.text();
    try {
      await page.waitForFunction(() => window.__groupsHydrated);
    } catch (error) {
      console.error("Hydration timeout", { path, url: page.url(), errors,
        body: (await page.$eval("body", body => body.innerText)).slice(0, 1500) });
      throw error;
    }
    return html;
  }

  async function login(page, id, name, redirect = "/groups?lang=en") {
    const params = new URLSearchParams({ user_id: id, username: name, redirect });
    await load(page, `/test/login?${params}`);
  }

  async function api(page, method, path, body, expected = 200) {
    const result = await page.evaluate(async ({ method, path, body }) => {
      const response = await fetch(path, {
        method,
        headers: body === undefined ? {} : { "Content-Type": "application/json" },
        body: body === undefined ? undefined : JSON.stringify(body),
      });
      return { status: response.status, text: await response.text() };
    }, { method, path, body });
    assert.equal(result.status, expected, `${method} ${path}: ${result.text}`);
    return result.text ? JSON.parse(result.text) : null;
  }

  async function clickText(page, text) {
    await page.waitForFunction(text => [...document.querySelectorAll("button")]
      .some(button => button.offsetParent !== null && button.textContent.trim() === text && !button.disabled), {}, text);
    await page.evaluate(text => [...document.querySelectorAll("button")]
      .find(button => button.offsetParent !== null && button.textContent.trim() === text && !button.disabled).click(), text);
  }

  async function mutation(page, method, path, click) {
    const responsePromise = page.waitForResponse(response =>
      new URL(response.url()).pathname === path && response.request().method() === method);
    await click();
    const response = await responsePromise;
    assert(response.ok(), `${method} ${path}: ${response.status()} ${await response.text()}`);
    return response.json();
  }

  async function listPermission(page, expected) {
    const entry = (await api(page, "GET", "/api/v1/list"))
      .find(entry => entry.list.id === listId);
    assert.equal(entry?.permission, expected, "role membership determines list access");
  }

  try {
    owner = await newPage();
    await login(owner, ownerId, "GroupsGateOwner");
    await clickText(owner, "Create Group");
    await owner.type("#new-group-name", groupName);
    const group = await mutation(owner, "POST", "/api/v1/group/create", () => clickText(owner, "Save"));
    groupId = group.id;
    const groupPath = `/groups/${groupId}`;
    await owner.waitForSelector(`a[href="${groupPath}"]`);
    const documentToken = `groups-${seed}`;
    await owner.evaluate(token => { window.__groupsDocumentToken = token; }, documentToken);
    await owner.click(`a[href="${groupPath}"]`);
    await owner.waitForFunction(path => location.pathname === path, {}, groupPath);
    await owner.waitForFunction(name => document.querySelector("h1")?.textContent.includes(name), {}, groupName);
    assert.equal(await owner.evaluate(() => window.__groupsDocumentToken), documentToken, "group link navigates within the hydrated document");
    assert((await load(owner, `${groupPath}?lang=en`)).includes(groupName), "group name renders in SSR");
    await owner.waitForFunction(name => document.querySelector("h1")?.textContent.includes(name), {}, groupName);
    console.log("[ok] create group, client navigation, direct SSR and hydration");

    await clickText(owner, "New role");
    await owner.type('input[aria-label="Role name"]', "Gate readers");
    const role = await mutation(owner, "POST", `/api/v1/group/${groupId}/roles`, () => clickText(owner, "Create role"));
    await owner.waitForSelector('button[aria-label="Rename role"]');
    await owner.click('button[aria-label="Rename role"]');
    const renameInput = 'input[aria-label="Rename role"]';
    await owner.waitForSelector(renameInput);
    await owner.$eval(renameInput, input => { input.value = ""; input.dispatchEvent(new Event("input", { bubbles: true })); });
    await owner.type(renameInput, "Gate members");
    const rolePath = `/api/v1/group/${groupId}/roles/${role.id}`;
    await mutation(owner, "PATCH", rolePath, () => owner.$eval(renameInput, input => input.parentElement.querySelector("button").click()));
    assert.equal((await api(owner, "GET", `/api/v1/group/${groupId}`)).roles[0].name, "Gate members");

    // This id has never logged in: the optional name must allow a transactional
    // user upsert instead of a foreign-key failure from the role picker.
    const memberPath = `${rolePath}/members/${memberId}`;
    await api(owner, "POST", memberPath, { display_name: memberName });
    assert((await api(owner, "GET", `${rolePath}/members`)).some(member => String(member.user_id) === memberId));
    const member = await newPage();
    await login(member, memberId, memberName, "/settings?lang=en");
    assert((await load(member, "/groups?lang=en")).includes(groupName), "member's populated group overview renders in SSR and hydrates");
    assert((await load(member, `${groupPath}?lang=en`)).includes("Gate members"), "member sees roles in SSR");
    await member.waitForFunction(() => document.body.innerText.includes("Gate members"));
    assert.equal(await member.$('button[aria-label="Rename role"]'), null, "member has no owner controls");
    await api(member, "POST", `/api/v1/group/${groupId}/roles`, { name: "Forbidden" }, 403);
    console.log("[ok] role create/rename, never-logged-in member, and owner-only authorization");

    const worlds = await api(owner, "GET", "/api/v1/world_data");
    const world = worlds.regions[0].datacenters[0].worlds[0].id;
    const listName = `Role list ${seed}`;
    await api(owner, "POST", "/api/v1/list/create", { name: listName, wdr_filter: { World: world } });
    listId = (await api(owner, "GET", "/api/v1/list")).find(entry => entry.list.name === listName).list.id;
    await listPermission(member, undefined);
    await api(owner, "POST", `/api/v1/list/${listId}/share/role`, { role_id: role.id, permission: "Read" });
    await listPermission(member, "Read");
    assert((await load(member, `/list/${listId}?lang=en`)).includes(listName), "role-shared list renders in SSR");
    await member.waitForFunction(name => document.body.innerText.includes(name), {}, listName);
    await api(member, "POST", `/api/v1/list/${listId}/add/item`, {
      id: 0, list_id: listId, item_id: 2, hq: null, quantity: 1, acquired: 0,
    }, 403);
    await api(owner, "POST", `/api/v1/list/${listId}/share/role`, { role_id: role.id, permission: "Write" });
    await listPermission(member, "Write");
    await api(member, "POST", `/api/v1/list/${listId}/add/item`, {
      id: 0, list_id: listId, item_id: 2, hq: null, quantity: 1, acquired: 0,
    });

    await load(owner, `${groupPath}?lang=en`);
    await owner.click('button[aria-label="Show members"]');
    const roleSearchId = `group-member-search-${groupId}-role-${role.id}`;
    await owner.waitForSelector(`#${roleSearchId}`);
    const searchIds = await owner.$$eval(`input[id^="group-member-search-${groupId}"]`, inputs => inputs.map(input => input.id));
    assert.equal(searchIds.length, 2, "both group and role member searches are present");
    assert.equal(new Set(searchIds).size, searchIds.length, "member search inputs have distinct IDs");
    await owner.click(`label[for="${roleSearchId}"]`);
    assert.equal(await owner.evaluate(() => document.activeElement.id), roleSearchId, "role search label focuses its own input");
    await owner.waitForSelector('button[aria-label="Remove from role"]');
    await mutation(owner, "DELETE", memberPath, () => owner.click('button[aria-label="Remove from role"]'));
    await listPermission(member, undefined);
    await api(member, "GET", `/api/v1/list/${listId}`, undefined, 403);
    assert((await api(owner, "GET", `/api/v1/group/${groupId}/members`)).some(row => String(row.user_id) === memberId), "removing a role preserves plain group membership");
    console.log("[ok] role grants Read/Write list access; removing role membership revokes it immediately");

    await api(owner, "POST", memberPath, { display_name: memberName });
    await listPermission(member, "Write");
    await load(owner, `${groupPath}?lang=en`);
    await owner.click('button[aria-label="Delete role"]');
    await mutation(owner, "DELETE", rolePath, () => owner.click('button[aria-label="Click again to confirm delete"]'));
    await listPermission(member, undefined);
    assert.equal((await api(owner, "GET", `/api/v1/group/${groupId}`)).roles.length, 0);
    await api(owner, "DELETE", `/api/v1/group/${groupId}/member/remove/${memberId}`);
    await api(member, "GET", `/api/v1/group/${groupId}`, undefined, 403);
    assert((await load(owner, "/groups?lang=en")).includes(groupName), "populated group cards render in SSR and hydrate");
    await load(owner, `${groupPath}?lang=en`);
    await owner.click('button[aria-label="Delete Group"]');
    await mutation(owner, "DELETE", `/api/v1/group/${groupId}`, () => owner.click('button[aria-label="Click again to confirm delete"]'));
    groupId = undefined;
    await owner.waitForFunction(() => location.pathname === "/groups");
    console.log("[ok] role and group deletion revoke access and return to the groups page");
    assert.deepEqual(errors, [], "no page errors during group/list SSR, hydration, and navigation");
  } finally {
    try {
      if (owner && listId !== undefined) await api(owner, "DELETE", `/api/v1/list/${listId}/delete`);
    } finally {
      try {
        if (owner && groupId !== undefined) await api(owner, "DELETE", `/api/v1/group/${groupId}`);
      } finally {
        await browser.close();
      }
    }
  }
}

main().catch(error => { console.error(error); process.exitCode = 1; });
