"use strict";

// Hold an actual list fetch across client-side navigation. Returning a stale
// success used to access a disposed signal; a stale denial cleared the new doc.
module.exports = async function navigationRace({ page, baseUrl, userId, worldId,
  createList, addItem, api, createdLists, waitForState, waitForDocKey, timeout }) {
  for (const mode of ["load", "revalidate"])
  for (const status of [200, 403])
  for (const destination of ["list", "unmount"]) {
    const a = await createList(page, worldId, `Navigation source ${mode} ${status}`);
    const b = await createList(page, worldId, `Navigation target ${mode} ${status}`);
    createdLists.push(a, b);
    for (const [id, item] of [[a, 5], [b, 6], [b, 7]]) {
      const result = await addItem(page, id, item);
      if (result.status !== 200) throw new Error(`navigation fixture: ${result.status}`);
    }
    await page.goto(`${baseUrl}/list/${a}`, { waitUntil: "domcontentloaded" });
    const ready = await waitForState(page, s => s.unacquired === 1, timeout);
    if (ready.unacquired !== 1) throw new Error("source list did not hydrate");
    await waitForDocKey(page, userId, a, true, timeout);
    const existing = mode === "revalidate"
      ? await api(page, "GET", `/api/v1/list/${a}/listings`)
      : null;
    const errors = [];
    const onError = error => errors.push(String(error));
    const onConsole = message => {
      if (message.type() === "error") errors.push(message.text());
    };
    page.on("pageerror", onError);
    page.on("console", onConsole);
    try {
      await page.evaluate(({ a, status }) => {
        const original = window.fetch;
        window.__listNavigationProbe = { captured: false, released: false };
        window.fetch = async function(input, init) {
          const url = new URL(typeof input === "string" ? input : input.url, location.href);
          const response = await original.call(this, input, init);
          const probe = window.__listNavigationProbe;
          if (url.pathname === `/api/v1/list/${a}/listings` && !probe.captured) {
            probe.captured = true;
            await new Promise(resolve => { probe.release = resolve; });
            probe.released = true;
            if (status === 403) return new Response(JSON.stringify({ ApiError: "Forbidden" }), {
              status, headers: { "Content-Type": "application/json" },
            });
            const body = await response.json();
            body[0].permission = "Read";
            return new Response(JSON.stringify(body), {
              status: 200, headers: { "Content-Type": "application/json" },
            });
          }
          return response;
        };
        window.__restoreListFetch = () => { window.fetch = original; };
      }, { a, status });
      // Adding a new item forces load_view's missing-coverage fetch. Editing
      // an already covered row leaves that cache valid and triggers only the
      // trailing silent permission revalidation.
      const added = mode === "load"
        ? await addItem(page, a, 8)
        : await api(page, "POST", "/api/v1/list/item/edit", {
            ...existing.body[1][0][0], quantity: 2,
          });
      if (added.status !== 200) throw new Error(`revalidation trigger: ${added.status}`);
      await page.waitForFunction(() => window.__listNavigationProbe?.captured, { timeout });
      const navigate = path => page.evaluate(path => {
        const link = document.createElement("a");
        link.href = path;
        link.textContent = "Navigation regression target";
        document.body.appendChild(link);
        link.click();
        link.remove();
      }, path);
      if (destination === "unmount") {
        await navigate("/list");
        await page.waitForFunction(() => location.pathname === "/list", { timeout });
        await page.evaluate(() => window.__listNavigationProbe.release());
        await page.waitForFunction(() => window.__listNavigationProbe.released, { timeout });
        // Let the response body and disposed resource continuation settle
        // while the list-view component is absent.
        await new Promise(resolve => setTimeout(resolve, 250));
      }
      await navigate(`/list/${b}`);
      await page.waitForFunction(b => location.pathname === `/list/${b}`, { timeout }, b);
      // The snapshot key proves B's handle opened while A's response is held.
      const keys = await waitForDocKey(page, userId, b, true, timeout);
      if (!keys.length) throw new Error("target handle did not open before stale response");
      await page.evaluate(() => {
        if (!window.__listNavigationProbe?.captured) throw new Error("navigation reloaded document");
        window.__listNavigationProbe.release();
      });
      await page.waitForFunction(() => window.__listNavigationProbe.released, { timeout });
      // The target rows may already satisfy waitForState; allow the released
      // response's body parsing and WASM continuation to run before asserting.
      await new Promise(resolve => setTimeout(resolve, 250));
      const target = await waitForState(page, s => s.unacquired === 2, timeout);
      if (target.unacquired !== 2) throw new Error(`stale ${status} response broke target list`);
      await page.click('button[aria-label="Mark as acquired"]');
      const deadline = Date.now() + timeout;
      let persisted = false;
      while (Date.now() < deadline) {
        const res = await api(page, "GET", `/api/v1/list/${b}/listings`);
        persisted = res.status === 200 && res.body[1].some(([item]) => item.acquired > 0);
        if (persisted) break;
        await new Promise(resolve => setTimeout(resolve, 100));
      }
      if (!persisted) throw new Error(`target doc detached after stale ${status} response`);
      const permission = await page.evaluate(({ userId, b }) =>
        JSON.parse(localStorage.getItem(`ultros.listdoc.index.v1.${userId}`) || "{}")
          .lists?.[b]?.permission, { userId, b });
      if (permission !== 3) throw new Error(`stale permission contaminated target: ${permission}`);
      if (errors.length) throw new Error(errors.join("\n"));
      console.log(`  + stale ${mode} ${status} after ${destination} leaves successor editable and syncing`);
    } finally {
      await page.evaluate(() => {
        window.__listNavigationProbe?.release?.();
        window.__restoreListFetch?.();
      }).catch(() => {});
      page.off("pageerror", onError);
      page.off("console", onConsole);
    }
  }
};
