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
