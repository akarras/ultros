// Real browser Web Locks, localStorage and recovery-format regression.
// Rust storage tests exercise merging actual Loro snapshots under this lock.
const assert = require("node:assert/strict");
const http = require("node:http");
const fs = require("node:fs");
const path = require("node:path");
const puppeteer = require("puppeteer");

async function main() {
  const source = fs.readFileSync(
    path.join(__dirname, "../ultros/static/account-list-store.mjs"),
  );
  const server = http.createServer((req, res) => {
    res.setHeader(
      "Content-Type",
      req.url === "/store.mjs" ? "text/javascript" : "text/html",
    );
    res.end(
      req.url === "/store.mjs"
        ? source
        : "<!doctype html><title>Account storage test</title>",
    );
  });
  await new Promise((r) => server.listen(0, "127.0.0.1", r));
  let browser;
  try {
    browser = await puppeteer.launch({ headless: true });
    const a = await browser.newPage(),
      b = await browser.newPage();
    const url = `http://127.0.0.1:${server.address().port}`;
    for (const page of [a, b]) {
      await page.goto(url);
      await page.evaluate(async () => {
        window.api = await import("/store.mjs");
      });
    }
    // Deliberately hold the lock while the second tab tries to save.
    await a.evaluate(() => {
      window.pending = api.accountSaveLocked("1", 7, 0, async () => {
        localStorage.setItem("started", "yes");
        await new Promise((r) => (window.release = r));
        localStorage.setItem("items", JSON.stringify(["a"]));
      });
    });
    await a.waitForFunction(() => localStorage.getItem("started") === "yes", {
      polling: 100,
    });
    await b.evaluate(() => {
      window.pending = api.accountSaveLocked("1", 7, 0, () => {
        const items = JSON.parse(localStorage.getItem("items") || "[]");
        items.push("b");
        localStorage.setItem("items", JSON.stringify(items));
      });
    });
    await a.evaluate(() => window.release());
    await Promise.all([
      a.evaluate(() => window.pending),
      b.evaluate(() => window.pending),
    ]);
    await a.reload();
    assert.deepEqual(
      await a.evaluate(() => JSON.parse(localStorage.getItem("items"))),
      ["a", "b"],
    );
    assert.equal(
      await b.evaluate(async () => {
        try {
          await api.accountSaveLocked("1", 7, 0, () => {
            throw new Error("quota");
          });
        } catch {}
        return api.accountSaveLocked("1", 7, 0, () => true);
      }),
      true,
      "a failed save releases the lock for retry",
    );
    assert.deepEqual(
      await b.evaluate(async () => {
        const first = api.accountRemember("1", 7, new Uint8Array([1]));
        try {
          await api.accountSaveLocked("1", 7, first, () => false);
        } catch {}
        const retained = [...api.accountRecovery("1", 7)];
        const other = api.accountRecovery("2", 7);
        const latest = api.accountRemember("1", 7, new Uint8Array([2]));
        await api.accountSaveLocked("1", 7, first, () => true);
        const newer = [...api.accountRecovery("1", 7)];
        await api.accountSaveLocked("1", 7, latest, () => true);
        const cleared = api.accountRecovery("1", 7);
        api.accountRemember("1", 7, new Uint8Array([3]));
        api.accountForget("1", 7);
        return [retained, other, newer, cleared, api.accountRecovery("1", 7)];
      }),
      [[1], null, [2], null, null],
      "recovery is account-scoped and only the latest saved edit clears it",
    );
    assert.equal(
      await b.evaluate(async () => {
        Object.defineProperty(navigator, "locks", {
          value: undefined,
          configurable: true,
        });
        let called = false;
        try {
          await api.accountSaveLocked("1", 7, 0, () => {
            called = true;
          });
        } catch {}
        return called;
      }),
      false,
      "unsupported locking must never perform an unsafe write",
    );
    console.log("Account storage browser checks passed");
  } finally {
    await browser?.close();
    await new Promise((r) => server.close(r));
  }
}
main().catch((e) => {
  console.error(e);
  process.exitCode = 1;
});
