#!/usr/bin/env node
"use strict";

// `/groups/:id` in each of the three states it renders differently: manual,
// Discord-synced, and frozen. Requires an isolated test-auth server — the
// manual group is built through the real API, and the two Discord-backed
// states through `/test/group/fixture`, which drives the same `UltrosDb`
// calls a live guild would.
//
// `groups.cjs` already covers the group page's mutations (create/rename/delete
// a role, add/remove members, and the list permissions they grant). This
// script covers what that one cannot reach: the page's Discord-dependent
// states, and the suspense boundaries around the member and role sections —
// a role's member list, and the share modal's role <select>, neither of which
// had ever been resolved in a browser.
//
// Positive SSR assertions are made against the response body; every negative
// one ("this control must not be offered") is made against the hydrated DOM
// instead. Owner-only controls hang off the viewer resource, so their absence
// from the server's HTML proves nothing about what the owner ends up seeing.
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const puppeteer = require("puppeteer");
const { capture } = require("./capture.cjs");

const ARTIFACT_DIR = path.join(__dirname, "artifacts", "group-detail");
const READ_ONLY_ROLE = "Members of this role come from Discord.";
const ROLE_SELECT = 'select[aria-label="Role (optional)"]';

async function main() {
  const base = process.env.BASE_URL || "http://127.0.0.1:8080";
  const timeout = Number(process.env.TIMEOUT_MS || 30000);
  const browser = await puppeteer.launch({ headless: true, args: ["--no-sandbox"] });
  const errors = [];
  // Same seeding idiom as groups.cjs: unique per run so repeated runs against
  // one database never collide, with room above it for the guild ids, role
  // ids, and members this script invents.
  const seed = Date.now() * 1000 + Math.floor(Math.random() * 100);
  const ownerId = String(seed);
  const syncedMember = { user_id: seed + 1, display_name: `SyncedHolder${seed}` };
  const manualMemberId = String(seed + 2);
  const manualMemberName = `ManualHolder${seed}`;
  const frozenMember = { user_id: seed + 3, display_name: `FrozenHolder${seed}` };
  const groupIds = [];
  let listId;
  let owner;

  fs.mkdirSync(ARTIFACT_DIR, { recursive: true });

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

  // Build a group in a state only a live Discord guild could otherwise reach.
  async function fixture(page, spec) {
    const result = await api(page, "POST", "/test/group/fixture", spec);
    groupIds.push(result.group_id);
    return result;
  }

  // `textContent`, not `innerText`: the "Managed by Discord" marker is an
  // `sr-only` span, and a rendering-aware read would let the assertion that it
  // is *absent* from a frozen group pass for the wrong reason.
  const bodyText = page => page.evaluate(() => document.body.textContent);

  // Wait for the group page's own content, not merely for hydration: the
  // detail and member resources each sit behind a suspense boundary whose
  // fallback is a skeleton.
  async function groupPage(page, groupId, name) {
    const html = await load(page, `/groups/${groupId}?lang=en`);
    await page.waitForFunction(name => document.querySelector("h1")?.textContent.includes(name), {}, name);
    await page.waitForFunction(() => !document.querySelector(".skeleton-block"));
    return html;
  }

  // A role row's expand button carries the role name and, for a Discord-backed
  // role, its "Synced"/"Orphaned" marker.
  const roleButton = (page, name, label, marker = "") => page.waitForFunction(
    (name, label, marker) => [...document.querySelectorAll("button")].some(button =>
      button.getAttribute("aria-label") === label
      && button.textContent.includes(name)
      && button.textContent.includes(marker)),
    {}, name, label, marker);

  // Open a role row and wait out the suspense boundary its member list sits
  // behind. The fallback is a skeleton, so "resolved" means the skeleton is
  // gone — not merely that the row expanded.
  async function expandRole(page, roleName) {
    await roleButton(page, roleName, "Show members");
    await page.evaluate(name => [...document.querySelectorAll("button")]
      .find(button => button.getAttribute("aria-label") === "Show members"
        && button.textContent.includes(name)).click(), roleName);
    await page.waitForFunction(name => {
      const row = [...document.querySelectorAll("button")]
        .find(button => button.getAttribute("aria-label") === "Hide members"
          && button.textContent.includes(name))?.closest("div.rounded");
      return row && !row.querySelector(".skeleton-block");
    }, {}, roleName);
  }

  const shot = (page, name) =>
    capture(page, { path: path.join(ARTIFACT_DIR, `${name}.png`), fullPage: true });

  // The share modal has four <select>s and only the role one is labelled, so
  // the group picker is found by the placeholder option it alone carries.
  // Re-queried per call: the nested boundary around the role select
  // re-suspends whenever the group changes.
  async function groupSelect(page) {
    for (const select of await page.$$("select")) {
      const isGroupPicker = await select.evaluate(node => node.getAttribute("aria-label") === null
        && [...node.options].some(option => /Choose a group|no groups/i.test(option.textContent)));
      if (isGroupPicker) return select;
    }
    throw new Error("share modal has no group picker");
  }

  const roleOptions = page =>
    page.$$eval(`${ROLE_SELECT} option`, options => options.map(option => option.textContent.trim()));

  async function pickGroup(page, groupId, expectedRole) {
    const select = await groupSelect(page);
    await select.select(String(groupId));
    // Resolving the nested boundary swaps the disabled fallback for the real
    // control, so waiting on the option text is what proves it resolved.
    await page.waitForFunction((selector, role) => {
      const select = document.querySelector(selector);
      return select && !select.disabled
        && [...select.options].some(option => option.textContent.trim() === role);
    }, {}, ROLE_SELECT, expectedRole);
  }

  try {
    owner = await newPage();
    await login(owner, ownerId, `GroupDetailOwner${seed}`);

    // ---------------------------------------------------------------
    // 1. Manual group: no Discord anywhere on the page, and both the
    //    member and role sections filled in through the real API.
    // ---------------------------------------------------------------
    const manualName = `Manual detail ${seed}`;
    const manual = await api(owner, "POST", "/api/v1/group/create", { name: manualName });
    groupIds.push(manual.id);
    const manualRole = await api(owner, "POST", `/api/v1/group/${manual.id}/roles`, { name: "Manual crew" });
    await api(owner, "POST",
      `/api/v1/group/${manual.id}/roles/${manualRole.id}/members/${manualMemberId}`,
      { display_name: manualMemberName });

    const manualHtml = await groupPage(owner, manual.id, manualName);
    assert(manualHtml.includes(manualName), "manual group name renders in SSR");
    assert(manualHtml.includes("Manual crew"), "manual role renders in SSR");

    const manualBody = await bodyText(owner);
    assert(manualBody.includes(manualMemberName), "manual member is listed");
    assert(/2\s*members/.test(manualBody), `manual group counts owner and member: ${manualBody}`);
    assert(/1\s*roles/.test(manualBody), `manual group counts its role: ${manualBody}`);
    // A group that was never linked has no badge, nothing to sync, and no
    // importer — the three things each of the other two states changes.
    for (const absent of ["Synced with Discord", "No longer synced", "Sync now",
      "Last synced", "Never synced", "Import from Discord"]) {
      assert(!manualBody.includes(absent), `manual group must not claim "${absent}"`);
    }
    // The chip on the member's own row proves the members section resolved
    // against the group's roles rather than printing ids it could not name.
    assert(await owner.evaluate((member, role) => {
      const row = [...document.querySelectorAll("span")]
        .find(span => span.textContent.trim() === member)?.closest("div.rounded");
      return !!row && [...row.querySelectorAll("span")].some(span => span.textContent.trim() === role);
    }, manualMemberName, "Manual crew"), "the member's row carries its role chip");
    await owner.waitForSelector(`#group-member-search-${manual.id}`);

    await expandRole(owner, "Manual crew");
    const manualExpanded = await bodyText(owner);
    assert(manualExpanded.includes(manualMemberName), "the role's member list resolves after hydration");
    assert(!manualExpanded.includes(READ_ONLY_ROLE), "a manual role's membership is editable");
    await owner.waitForSelector(`#group-member-search-${manual.id}-role-${manualRole.id}`);
    await owner.waitForSelector('button[aria-label="Remove from role"]');
    await shot(owner, "manual");
    console.log("[ok] manual group: SSR, member chips, and the role's suspense boundary");

    // ---------------------------------------------------------------
    // 2. Discord-synced group: mirrored badge, sync controls, and a role
    //    whose membership belongs to Discord.
    // ---------------------------------------------------------------
    const syncedName = `Synced detail ${seed}`;
    const synced = await fixture(owner, {
      name: syncedName,
      guild_id: seed + 100,
      // Two roles, because a mirrored group renders a healthy Discord role and
      // one whose Discord role has since been deleted differently, and the
      // marker that says which is the only thing that distinguishes them.
      discord_roles: [
        { discord_role_id: seed + 200, name: "Guild officers", members: [syncedMember] },
        { discord_role_id: seed + 201, name: "Departed officers", orphaned: true },
      ],
    });
    const syncedHtml = await groupPage(owner, synced.group_id, syncedName);
    assert(syncedHtml.includes("Synced with Discord"), "a mirrored group carries the synced badge");
    assert(syncedHtml.includes("Last synced"), "a group that has synced says when");
    assert(!syncedHtml.includes("Never synced"), "a group that has synced is not 'never synced'");
    assert(syncedHtml.includes("Guild officers"), "the imported role renders in SSR");

    const syncedBody = await bodyText(owner);
    assert(syncedBody.includes(syncedMember.display_name), "the synced member is listed");
    assert(/2\s*roles/.test(syncedBody), `both imported roles are counted: ${syncedBody}`);
    assert(syncedBody.includes("Sync now"), "the owner of a live-linked group can sync it");
    assert(syncedBody.includes("Import from Discord"), "a guild-linked group offers the role importer");
    // The role markers and the member's lock are the places the page says
    // "Discord owns this", and all of them have to survive hydration.
    await roleButton(owner, "Guild officers", "Show members", "Synced");
    await roleButton(owner, "Departed officers", "Show members", "Orphaned");
    assert(await owner.evaluate(() => [...document.querySelectorAll("span")]
      .some(span => span.textContent.trim() === "Managed by Discord")),
      "the synced member is marked as Discord's");
    // Neither the owner nor anyone else may remove a member Discord placed.
    assert.equal(await owner.$('button[aria-label="Remove Member"]'), null,
      "a synced member has no remove button");

    await expandRole(owner, "Guild officers");
    const syncedExpanded = await bodyText(owner);
    assert(syncedExpanded.includes(READ_ONLY_ROLE),
      "a Discord-backed role says its membership is read-only");
    assert(syncedExpanded.includes(syncedMember.display_name),
      "the synced role's member list resolves after hydration");
    assert.equal(await owner.$(`#group-member-search-${synced.group_id}-role-${synced.role_ids[0]}`), null,
      "a Discord-backed role offers no member picker");
    assert.equal(await owner.$('button[aria-label="Remove from role"]'), null,
      "a Discord-backed role offers no remove buttons");

    // An orphaned role keeps its Discord ownership — nothing will refill it,
    // and the owner still may not — so its empty member list says so.
    await expandRole(owner, "Departed officers");
    const orphanedExpanded = await bodyText(owner);
    assert(orphanedExpanded.includes("Nobody holds this role yet."),
      "an orphaned role with no members resolves to its empty state");
    assert(orphanedExpanded.includes(READ_ONLY_ROLE),
      "an orphaned role is still Discord's while the guild link survives");
    assert.equal(await owner.$(`#group-member-search-${synced.group_id}-role-${synced.role_ids[1]}`), null,
      "an orphaned role offers no member picker either");
    await shot(owner, "synced");
    console.log("[ok] Discord-synced group: badge, sync controls, and a read-only role that hydrates");

    // ---------------------------------------------------------------
    // 3. Frozen group: the bot is gone, membership stays, and the owner
    //    inherits everything Discord used to manage.
    // ---------------------------------------------------------------
    const frozenName = `Frozen detail ${seed}`;
    const frozenReason = `The Ultros bot was removed from the server ${seed}`;
    const frozen = await fixture(owner, {
      name: frozenName,
      guild_id: seed + 101,
      discord_roles: [{
        discord_role_id: seed + 202,
        name: "Stranded officers",
        members: [frozenMember],
      }],
      frozen_reason: frozenReason,
    });
    const frozenHtml = await groupPage(owner, frozen.group_id, frozenName);
    assert(frozenHtml.includes("No longer synced with Discord"), "the frozen banner heading renders in SSR");
    assert(frozenHtml.includes("The Ultros bot was removed from this Discord server"),
      "the frozen banner explains what happened");
    assert(frozenHtml.includes(frozenReason), "the server's own reason rides along verbatim");
    assert(frozenHtml.includes("Stranded officers"), "the once-synced role renders in SSR");
    // The banner is a live region, so the reason has to be inside one rather
    // than merely somewhere on the page.
    assert(await owner.$$eval('[role="status"]', (nodes, reason) =>
      nodes.some(node => node.textContent.includes(reason)), frozenReason),
      "the frozen banner is a status region");

    const frozenBody = await bodyText(owner);
    assert(frozenBody.includes(frozenMember.display_name), "a frozen group keeps its members");
    // Freezing releases the guild link, so every Discord affordance goes too.
    for (const absent of ["Sync now", "Synced with Discord", "Import from Discord", "Managed by Discord"]) {
      assert(!frozenBody.includes(absent), `a frozen group must not offer "${absent}"`);
    }
    // Freezing reverts the role to a manual one, so it loses the marker
    // altogether rather than being shown as an orphan of a link that is gone.
    await roleButton(owner, "Stranded officers", "Show members");
    assert(await owner.evaluate(() => {
      const button = [...document.querySelectorAll("button")].find(button =>
        button.getAttribute("aria-label") === "Show members"
        && button.textContent.includes("Stranded officers"));
      return !/Synced|Orphaned/.test(button.textContent);
    }), "a frozen group's roles carry no sync marker");
    // The synced member was handed back to the owner, who may now remove them.
    await owner.waitForSelector('button[aria-label="Remove Member"]');

    await expandRole(owner, "Stranded officers");
    const frozenExpanded = await bodyText(owner);
    assert(frozenExpanded.includes(frozenMember.display_name),
      "the once-synced role's member list resolves after hydration");
    assert(!frozenExpanded.includes(READ_ONLY_ROLE),
      "freezing hands the role's membership to the owner");
    await owner.waitForSelector(`#group-member-search-${frozen.group_id}-role-${frozen.role_ids[0]}`);
    await owner.waitForSelector('button[aria-label="Remove from role"]');
    await shot(owner, "frozen");
    console.log("[ok] frozen group: banner, unmarked role, and membership handed back to the owner");

    // ---------------------------------------------------------------
    // 4. The share modal's role <select>. It sits in a suspense boundary
    //    of its own, keyed on the chosen group, and had only ever been
    //    rendered on the server.
    // ---------------------------------------------------------------
    const worlds = await api(owner, "GET", "/api/v1/world_data");
    const world = worlds.regions[0].datacenters[0].worlds[0].id;
    const listName = `Group detail list ${seed}`;
    await api(owner, "POST", "/api/v1/list/create", { name: listName, wdr_filter: { World: world } });
    listId = (await api(owner, "GET", "/api/v1/list")).find(entry => entry.list.name === listName).list.id;

    await load(owner, "/list?lang=en");
    await owner.waitForFunction(name => [...document.querySelectorAll(".panel.rounded-xl")]
      .some(card => card.innerText.includes(name)), {}, listName);
    await owner.evaluate(name => [...document.querySelectorAll(".panel.rounded-xl")]
      .find(card => card.innerText.includes(name))
      .querySelector('button[aria-label="Share list"]').click(), listName);

    // Until a group is picked the role select is disabled and offers only the
    // "everyone" option, so a stale pick can never share the wrong group's role.
    await owner.waitForSelector(ROLE_SELECT);
    assert.equal(await owner.$eval(ROLE_SELECT, select => select.disabled), true,
      "the role select is disabled until a group is chosen");
    assert.deepEqual(await roleOptions(owner), ["Everyone in the group"],
      "no group chosen means no roles to choose from");

    await pickGroup(owner, manual.id, "Manual crew");
    assert.deepEqual(await roleOptions(owner), ["Everyone in the group", "Manual crew"],
      "the chosen group's roles arrive in the select");

    // Switching groups re-suspends this control alone, and the roles on offer
    // follow the new group rather than the old one.
    await pickGroup(owner, synced.group_id, "Guild officers");
    assert.deepEqual(await roleOptions(owner),
      ["Everyone in the group", "Guild officers", "Departed officers"],
      "changing the group replaces the roles offered");

    await pickGroup(owner, manual.id, "Manual crew");
    await owner.select(ROLE_SELECT, String(manualRole.id));
    const sharePath = `/api/v1/list/${listId}/share/role`;
    const shared = owner.waitForResponse(response =>
      new URL(response.url()).pathname === sharePath && response.request().method() === "POST");
    await owner.evaluate(() => [...document.querySelectorAll("button")]
      .find(button => button.textContent.trim() === "Share group").click());
    const shareResponse = await shared;
    assert(shareResponse.ok(), `${sharePath}: ${shareResponse.status()} ${await shareResponse.text()}`);
    const [, sharedGroups, sharedRoles] = await api(owner, "GET", `/api/v1/list/${listId}/shares`);
    assert.deepEqual(sharedGroups, [], "picking a role shares the role, not the whole group");
    assert.equal(sharedRoles.length, 1, `exactly one role share: ${JSON.stringify(sharedRoles)}`);
    assert.equal(sharedRoles[0].role_id, manualRole.id, "the role picked in the select got the share");
    assert.equal(sharedRoles[0].role_name, "Manual crew");
    await shot(owner, "share-role-select");
    console.log("[ok] share modal: the role select resolves, follows the chosen group, and shares to it");

    assert.deepEqual(errors, [], "no page errors across the three group states");
  } finally {
    try {
      if (owner && listId !== undefined) {
        await api(owner, "DELETE", `/api/v1/list/${listId}/delete`).catch(() => {});
      }
      if (owner) {
        for (const groupId of groupIds) {
          await api(owner, "DELETE", `/api/v1/group/${groupId}`).catch(() => {});
        }
      }
    } finally {
      await browser.close();
    }
  }
}

main().catch(error => { console.error(error); process.exitCode = 1; });
