"use strict";
const assert = require("node:assert/strict");
const test = require("node:test");
const { cleanupOwnedLists, finishCleanup } = require("./list-fixture-cleanup.cjs");
test("cleanup only touches owned IDs, tolerates already deleted IDs, verifies final absence", async () => {
  const entries = new Set([1, 2, 999]); const removed = [];
  await cleanupOwnedLists(async (method, route) => {
    if (method === "GET") return { status: 200, body: [...entries].map(id => ({ list: { id } })) };
    const id = Number(route.split("/")[4]); removed.push(id); entries.delete(id);
    return { status: 200 };
  }, [1, 2, 3]);
  assert.deepEqual(removed, [1, 2]); assert.deepEqual([...entries], [999]);
});
test("failed list cleanup still deletes others and rejects leaked fixtures", async () => {
  const entries = new Set([1, 2]); const removed = [];
  await assert.rejects(cleanupOwnedLists(async (method, route) => {
    if (method === "GET") return { status: 200, body: [...entries].map(id => ({ list: { id } })) };
    const id = Number(route.split("/")[4]); removed.push(id);
    if (id === 1) return { status: 503 };
    entries.delete(id); return { status: 200 };
  }, [1, 2]), error => error instanceof AggregateError && error.errors.length === 2);
  assert.deepEqual(removed, [1, 2]);
});
test("cleanup preserves original failure and always closes browser", async () => {
  const original = new Error("assertion failed"), cleanup = new Error("cleanup failed"); let closed = false;
  await assert.rejects(finishCleanup([async () => { throw cleanup; }, async () => { closed = true; }], original), error => {
    assert.deepEqual(error.errors, [original, cleanup]); return true;
  });
  assert.equal(closed, true);
});
