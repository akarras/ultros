const test = require("node:test");
const assert = require("node:assert/strict");

test("account recovery is readable by the existing anonymous restore decoder", async () => {
  const { accountRecoveryBackup } = await import(
    "../ultros/static/account-list-store.mjs"
  );
  const { decodeGuestListBackup } = await import(
    "../ultros/static/guest-list-store.mjs"
  );
  const snapshot = new Uint8Array([1, 2, 3, 255]);
  const result = decodeGuestListBackup(
    accountRecoveryBackup("Raid materials", snapshot),
  );
  assert.equal(result.name, "Raid materials");
  assert.deepEqual(result.snapshot, snapshot);
  assert.equal("user" in result, false);
  assert.equal("id" in result, false);
});


test("a pending replacement keeps its source fence with the account-scoped recovery snapshot", async () => {
  const api = await import("../ultros/static/account-list-store.mjs");
  const snapshot = new Uint8Array([1, 2]);
  const source = new Uint8Array([3, 4]);
  const first = api.accountRemember("recovery-user", 7, snapshot, source);
  source[0] = 99;
  snapshot[0] = 99;
  assert.deepEqual(api.accountRecovery("recovery-user", 7), new Uint8Array([1, 2]));
  assert.deepEqual(api.accountRecoverySource("recovery-user", 7), new Uint8Array([3, 4]));
  assert.equal(api.accountRecoverySource("another-user", 7), null);
  const second = api.accountRemember("recovery-user", 7, new Uint8Array([5]));
  assert.equal(api.accountPending("recovery-user", 7, first), false);
  assert.equal(api.accountPending("recovery-user", 7, second), true);
  assert.equal(api.accountRecoverySource("recovery-user", 7), null);
  api.accountForget("recovery-user", 7);
  assert.equal(api.accountRecovery("recovery-user", 7), null);
});


test("pending account recovery preserves explicit readiness independently of its bytes", async () => {
  const api = await import("../ultros/static/account-list-store.mjs");
  const bytes = new Uint8Array([1, 2, 3]);
  api.accountRemember("readiness-user", 7, bytes, null, false);
  assert.equal(api.accountRecoveryReady("readiness-user", 7), false);
  assert.equal(api.accountRecoveryReady("another-user", 7), null);
  api.accountRemember("readiness-user", 7, bytes, new Uint8Array([4]), true);
  assert.equal(api.accountRecoveryReady("readiness-user", 7), true);
  api.accountForget("readiness-user", 7);
  assert.equal(api.accountRecoveryReady("readiness-user", 7), null);
});
