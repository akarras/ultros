"use strict";
const assert = require("node:assert/strict");

// Only IDs returned by this run's create calls may enter this helper. Inventory
// handles lists deliberately deleted by lifecycle scenarios without accepting
// a failed DELETE as proof of absence. Every requested delete is attempted.
async function cleanupOwnedLists(api, ownedIds) {
  const ids = new Set(ownedIds);
  const errors = [];
  const inventory = await api("GET", "/api/v1/list");
  assert.equal(inventory.status, 200, "owned fixture inventory must remain authenticated");
  assert(Array.isArray(inventory.body), "owned fixture inventory must be a list");
  const present = new Set(inventory.body.map(entry => entry.list.id));
  for (const id of ids) {
    if (!present.has(id)) continue;
    try {
      const result = await api("DELETE", `/api/v1/list/${id}/delete`);
      assert.equal(result.status, 200, `owned fixture ${id} cleanup must succeed`);
    } catch (error) { errors.push(error); }
  }
  try {
    const remaining = await api("GET", "/api/v1/list");
    assert.equal(remaining.status, 200, "verify owned fixture cleanup");
    assert.deepEqual(remaining.body.filter(entry => ids.has(entry.list.id)), [], "owned fixture lists must not remain after cleanup");
  } catch (error) { errors.push(error); }
  if (errors.length) throw new AggregateError(errors, "Owned list fixture cleanup failed");
}

// Run all cleanup even if a preceding operation fails, including browser.close.
// Keep the original assertion alongside cleanup errors, rather than replacing it.
async function finishCleanup(steps, originalError) {
  const errors = [];
  for (const step of steps) {
    try { await step(); } catch (error) { errors.push(error); }
  }
  if (errors.length) throw new AggregateError(originalError ? [originalError, ...errors] : errors, "Acceptance cleanup failed");
}
module.exports = { cleanupOwnedLists, finishCleanup };
