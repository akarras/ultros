"use strict";
// Deliberately separate from smoke. Every required suite must execute against
// one named fresh test-auth build; no allow-empty/skip switch can pass this gate.
const assert = require("node:assert/strict");
const { spawnSync } = require("node:child_process");
const fs = require("node:fs");
const path = require("node:path");

const suites = [
  "list-priced-acceptance.cjs",
  "list-shop-handoff.cjs",
  "lists-v2.cjs",
  "list-undo-keyboard.cjs",
  "list-adoption.cjs",
  "list-companion.cjs",
  "list-sync.cjs",
  "account-list-storage.cjs",
  "account-list-ui.cjs",
  "list-sort-accessibility.cjs",
  "device-build-prices.cjs",
];

function main() {
  assert(process.env.LISTS_ACCEPTANCE_BUILD?.trim(),
    "Set LISTS_ACCEPTANCE_BUILD to the exact fresh server/WASM commit (and dirty-tree fingerprint if built before commit). Smoke results cannot certify acceptance.");
  for (const name of ["SKIP_ASSERTS", "SKIP_PRICED", "ALLOW_EMPTY_MARKET"])
    assert(!process.env[name] || process.env[name] === "0", `${name} is incompatible with Lists acceptance`);
  assert.notEqual(process.env.STRICT_CONSOLE, "0", "Lists acceptance cannot suppress browser errors");
  const results = [];
  const startedAt = new Date().toISOString();
  const artifacts = path.join(__dirname, "artifacts", "lists-acceptance");
  fs.mkdirSync(artifacts, { recursive: true });
  for (const suite of suites) {
    assert(fs.existsSync(path.join(__dirname, suite)), `Required suite missing: ${suite}; integrate its prerequisite before acceptance`);
    console.log(`\n[ACCEPTANCE] ${suite}`);
    const result = spawnSync(process.execPath, [path.join(__dirname, suite)], {
      cwd: __dirname, env: { ...process.env, LISTS_ACCEPTANCE: "1" }, encoding: "utf8",
      maxBuffer: 32 * 1024 * 1024,
    });
    process.stdout.write(result.stdout || ""); process.stderr.write(result.stderr || "");
    const output = `${result.stdout || ""}\n${result.stderr || ""}`;
    fs.writeFileSync(path.join(artifacts, `${suite}.log`), output);
    const passed = result.status === 0 && !result.error && !/\[SMOKE ONLY\]|\[SKIP(?:PED)?\]/i.test(output);
    results.push({ suite, passed, exit: result.status, error: result.error?.message || null });
  }
  const report = { build: process.env.LISTS_ACCEPTANCE_BUILD, baseUrl: process.env.BASE_URL,
    startedAt, labs: "lists-sync", mode: "required-priced-acceptance", results };
  fs.writeFileSync(path.join(artifacts, "results.json"), JSON.stringify(report, null, 2));
  assert(results.every(result => result.passed), "Lists acceptance failed; inspect per-suite logs and results.json. Other smoke runs do not override failed/missing priced assertions.");
  console.log("PASS: required Lists acceptance suites completed (local test build; not production soak or physical FFXIV evidence).");
}
if (require.main === module) main();
module.exports = { suites };
