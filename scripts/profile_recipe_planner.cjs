// Time the real planner engine compiled to WASM, without DOM/network costs.
// Usage: node scripts/profile_recipe_planner.cjs target/recipe-profile/planner.wasm
const fs = require('node:fs');
const assert = require('node:assert/strict');

async function main() {
  const { instance } = await WebAssembly.instantiate(fs.readFileSync(
    process.argv[2] || 'target/recipe-profile/planner.wasm',
  ));
  const { setup, search } = instance.exports;
  console.log('worlds,items,needed,listings_per_world,locked,median_ms,checksum');
  for (const [worlds, items, needed, listings] of [
    [8, 6, 10, 20],
    [8, 7, 9999, 90],
    [8, 24, 100, 20],
    [32, 24, 100, 20],
    [32, 24, 1000, 20],
    [32, 64, 100, 20],
  ]) {
    setup(worlds, items, needed, listings);
    for (const locked of [false, true]) {
      const checksum = search(locked);
      const times = [];
      for (let i = 0; i < 5; i++) {
        const start = performance.now();
        const result = search(locked);
        times.push(performance.now() - start);
        assert.equal(result, checksum, 'identical snapshots must produce identical plans');
      }
      times.sort((a, b) => a - b);
      console.log(`${worlds},${items},${needed},${listings},${locked},${times[2].toFixed(2)},${checksum}`);
    }
  }
}

main().catch(error => { console.error(error); process.exitCode = 1; });
