"use strict";

const assert = require("node:assert/strict");
const test = require("node:test");
const { runGroupSharedList, USERS } = require("./group-shared-list.cjs");

// A browser fixture models cookie storage per context and records requests.
// Real Puppeteer/server coverage remains `npm run test:group-shared-list`.
function fixture({ sharedContext = false, failAfterCreate = false, failDelete = false, failSetup = false } = {}) {
  const calls = [];
  let contexts = 0;
  let closed = false;
  let list;
  let permission;
  const sharedCookies = {};

  function page(cookies) {
    return {
      setDefaultTimeout() {},
      async goto(url) {
        cookies.user = Number(new URL(url).searchParams.get("user_id"));
        calls.push({ method: "LOGIN", user: cookies.user });
        return { status: () => 200 };
      },
      async evaluate(_fn, { method, path, body }) {
        const user = cookies.user;
        calls.push({ method, path, user });
        let response = null;
        let status = 200;
        if (path === "/api/v1/current_user") {
          response = { id: user };
        } else if (path === "/api/v1/group/create") {
          assert.equal(user, USERS.owner.id);
          response = { id: 10 };
        } else if (path === "/api/v1/world_data") {
          response = { regions: [{ datacenters: [{ worlds: [{ id: 20 }] }] }] };
        } else if (path === "/api/v1/list/create") {
          assert.equal(user, USERS.owner.id);
          list = { id: 30, name: body.name };
        } else if (path === "/api/v1/list") {
          if (failAfterCreate && user === USERS.member.id) throw new Error("fixture request failed");
          response = user === USERS.nonMember.id ? [] : [{ list, permission: user === USERS.owner.id ? "Owner" : permission }];
        } else if (path === "/api/v1/list/30/share/group") {
          assert.equal(user, USERS.owner.id);
          permission = body.permission;
        } else if (path === "/api/v1/list/30/add/item") {
          assert.equal(user, USERS.member.id);
          status = permission === "Write" ? 200 : 403;
        } else if (method === "DELETE") {
          assert.equal(user, USERS.owner.id);
          if (failDelete && path.includes("/list/")) throw new Error("fixture cleanup failed");
        } else {
          assert.equal(path, `/api/v1/group/10/member/add/${USERS.member.id}`);
          assert.equal(user, USERS.owner.id);
        }
        return { status, body: response };
      },
    };
  }

  const browser = {
    async createBrowserContext() {
      contexts++;
      return {
        async newPage() {
          if (failSetup && contexts === 2) throw new Error("fixture page setup failed");
          return page(sharedContext ? sharedCookies : {});
        },
      };
    },
    async close() { closed = true; },
  };
  return { browser, calls, get contexts() { return contexts; }, get closed() { return closed; } };
}

test("actors remain distinct after all logins and permissions run as the member", async () => {
  const f = fixture();
  await runGroupSharedList(f.browser, "http://fixture.invalid");
  assert.equal(f.contexts, 3);
  assert.deepEqual(f.calls.slice(0, 6).map(({ method, user }) => [method, user]), [
    ["LOGIN", USERS.owner.id], ["LOGIN", USERS.member.id], ["LOGIN", USERS.nonMember.id],
    ["GET", USERS.owner.id], ["GET", USERS.member.id], ["GET", USERS.nonMember.id],
  ]);
  assert.equal(f.calls.filter(({ path }) => path === "/api/v1/list/30/add/item").length, 2);
  assert.deepEqual(f.calls.filter(({ method }) => method === "DELETE").map(({ path }) => path), [
    "/api/v1/list/30/delete", "/api/v1/group/10",
  ]);
  assert.equal(f.closed, true);
});

test("shared cookies fail identity verification before any mutation", async () => {
  const f = fixture({ sharedContext: true });
  await assert.rejects(runGroupSharedList(f.browser, "http://fixture.invalid"), /session identity mismatch for owner/);
  assert.equal(f.calls.some(({ method }) => method === "POST" || method === "DELETE"), false);
  assert.equal(f.closed, true);
});

test("request failure still deletes the created list and group and closes the browser", async () => {
  const f = fixture({ failAfterCreate: true });
  await assert.rejects(runGroupSharedList(f.browser, "http://fixture.invalid"), /fixture request failed/);
  assert.deepEqual(f.calls.filter(({ method }) => method === "DELETE").map(({ path }) => path), [
    "/api/v1/list/30/delete", "/api/v1/group/10",
  ]);
  assert.equal(f.closed, true);
});

test("failed list cleanup does not skip group or browser cleanup, and fails the run", async () => {
  const f = fixture({ failDelete: true });
  await assert.rejects(runGroupSharedList(f.browser, "http://fixture.invalid"), /1 group-shared-list assertion/);
  assert.equal(f.calls.at(-1).path, "/api/v1/group/10");
  assert.equal(f.closed, true);
});

test("partial context setup failure still closes the browser", async () => {
  const f = fixture({ failSetup: true });
  await assert.rejects(runGroupSharedList(f.browser, "http://fixture.invalid"), /fixture page setup failed/);
  assert.equal(f.closed, true);
});
