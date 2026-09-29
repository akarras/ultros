const { test } = require('node:test');
const assert = require('node:assert/strict');

const modulePromise = import('../ultros/static/guest-list-store.mjs');

test('guest backup preserves Unicode and every byte, omitting ownership and storage identity', async () => {
  const { encodeGuestListBackup, decodeGuestListBackup } = await modulePromise;
  const document = {
    id: 'device:private-id',
    revision: 9,
    user_id: 123,
    name: 'Raid supplies — 薬',
    snapshot: Uint8Array.from({ length: 256 }, (_, i) => i),
  };
  const backup = encodeGuestListBackup(document);
  assert.deepEqual(Object.keys(JSON.parse(backup)).sort(), ['format', 'name', 'snapshot', 'version']);
  assert.deepEqual(decodeGuestListBackup(backup), { name: document.name, snapshot: document.snapshot });
});

test('guest backup rejects damaged and unsupported envelopes without substituting an empty list', async () => {
  const { decodeGuestListBackup } = await modulePromise;
  for (const text of [
    '', 'null', '{}', '[]',
    JSON.stringify({ format: 'ultros-device-list', version: 2, name: 'A', snapshot: 'AQ==' }),
    JSON.stringify({ format: 'ultros-device-list', version: 1, name: 'A', snapshot: '%' }),
    JSON.stringify({ format: 'ultros-device-list', version: 1, name: 'A', snapshot: '' }),
    JSON.stringify({ format: 'ultros-device-list', version: 1, name: ' ', snapshot: 'AQ==' }),
  ]) {
    assert.throws(() => decodeGuestListBackup(text), { code: 'invalid' });
  }
});
