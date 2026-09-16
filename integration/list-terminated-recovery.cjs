"use strict";
const assert = require("node:assert/strict");
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));

// Synthetic lookup-failure regression, NOT evidence of actual revocation.
// A real Unsubscribe/Unsubscribed pair ends the server relay. Only its scoped
// Error and this editor's temporary listings 503s are synthetic. All socket
// handshakes, restored subscriptions, successful REST data and prices are real.
// No standalone main: missing fixtures/callbacks fail, never silently pass.
async function runTerminatedDocRecovery(options) {
  const { page, listId, userId, stream = "document", open, prepareShop,
    commitLocalEdit, readServerCounter, localDelta, prepareDraft, readFrozenTrip,
    remoteEdit, waitRemoteConvergence, cleanup, record,
    timeoutMs = 15000 } = options;
  assert(["document", "activity"].includes(stream), "known relay kind required");
  assert(Number.isInteger(listId) && listId > 0 && userId, "owned fixture list/user required");
  assert(Number.isInteger(timeoutMs) && timeoutMs >= 6500, "recovery timeout must cover bounded backoff");
  for (const [name, fn] of Object.entries({ open, prepareShop, readServerCounter,
    prepareDraft, readFrozenTrip, remoteEdit, waitRemoteConvergence, cleanup })) {
    assert.equal(typeof fn, "function", `${name} callback is required`);
  }
  if (stream === "document") {
    assert.equal(typeof commitLocalEdit, "function", "native recovery needs a real local UI edit");
    assert(Number.isInteger(localDelta) && localDelta !== 0, "nonzero additive local edit required");
  }
  const errors = [];
  const onError = error => errors.push(String(error.stack || error));
  page.on("pageerror", onError);
  const cacheKey = `ultros.listdoc.v1.${userId}.${listId}`;
  let evidence;
  let originalError;
  let preload;
  try {
    preload = await page.evaluateOnNewDocument(({ listId }) => {
      const NativeSocket = window.WebSocket;
      const nativeFetch = window.fetch;
      const p = window.__terminatedRecovery = {
        token: `${Date.now()}-${Math.random()}`, active: true, listId,
        sockets: [], frames: [], requests: [], seq: 0, outage: false,
        dropUpdates: false, droppedUpdates: [],
      };
      const add = (direction, socket, frame, extra = {}) => {
        p.frames.push({ seq: ++p.seq, at: performance.now(), direction,
          socket: p.sockets.indexOf(socket), frame, ...extra });
      };
      class ObservedSocket extends NativeSocket {
        constructor(...args) {
          super(...args);
          if (!String(args[0]).includes("/api/v1/realtime/events")) return;
          p.sockets.push(this);
          const receive = event => {
            if (!p.active) return;
            let frame; try { frame = JSON.parse(event.data); } catch { return; }
            add("in", this, frame, { synthetic: event.__terminatedRecovery === true });
          };
          this.addEventListener("message", receive);
          this.__removeRecoveryObserver = () => this.removeEventListener("message", receive);
        }
        send(data) {
          if (p.active && p.sockets.includes(this)) {
            let frame; try { frame = JSON.parse(data); } catch { /* native validation */ }
            if (frame) {
              if (p.dropUpdates && frame.ListDocUpdate?.list_id === listId) {
                p.droppedUpdates.push({ at: performance.now(), frame });
                return; // Deliberate outbound loss; never replay it from the test.
              }
              add("out", this, frame);
            }
          }
          return super.send(data);
        }
      }
      window.WebSocket = ObservedSocket;
      window.fetch = async function(input, init) {
        const url = new URL(typeof input === "string" || input instanceof URL ? String(input) : input.url, location.href);
        if (!p.active || url.origin !== location.origin || url.pathname !== `/api/v1/list/${listId}/listings`)
          return nativeFetch.call(this, input, init);
        const request = { at: performance.now(), injected: p.outage, status: null };
        p.requests.push(request);
        if (p.outage) {
          request.status = 503;
          return new Response(JSON.stringify({ ApiError: { Message: "Synthetic relay lookup outage" } }),
            { status: 503, headers: { "Content-Type": "application/json" } });
        }
        const response = await nativeFetch.call(this, input, init);
        request.status = response.status;
        return response;
      };
      p.unsubscribe = (socket, id) => {
        // Bypass the app's subscription disposal: its live handler must receive
        // the later synthetic Error, just as it receives a real relay failure.
        NativeSocket.prototype.send.call(p.sockets[socket], JSON.stringify({ Unsubscribe: { subscription_id: id } }));
      };
      p.fail = (socket, id) => {
        const event = new MessageEvent("message", { data: JSON.stringify({ SubscriptionEvent: {
          subscription_id: id, event: { Error: { message: `forbidden: no verified read access to list ${listId}` } },
        } }) });
        Object.defineProperty(event, "__terminatedRecovery", { value: true });
        p.sockets[socket].dispatchEvent(event);
      };
      p.restore = () => {
        p.active = false; p.dropUpdates = false; p.outage = false;
        window.fetch = nativeFetch;
        if (window.WebSocket === ObservedSocket) window.WebSocket = NativeSocket;
        for (const socket of p.sockets) socket.__removeRecoveryObserver?.();
      };
    }, { listId });
    await open();
    await page.bringToFront();
    await page.waitForFunction(({ listId, stream, cacheKey }) => {
      const p = window.__terminatedRecovery;
      const kind = stream === "document" ? "SubscribeListDoc" : "SubscribeList";
      return p?.frames.some(x => x.direction === "out" && x.frame[kind]?.list_id === listId)
        && localStorage.getItem(cacheKey) !== null
        && document.querySelector('[data-testid="realtime-status-indicator"]')?.dataset.status === "live";
    }, { timeout: timeoutMs }, { listId, stream, cacheKey });
    await prepareShop();
    assert(await page.$$eval('[data-shop-key] [data-testid="shop-stack-bought"]', buttons =>
      buttons.filter(button => !button.disabled).length >= 2), "two real priced actionable fixture stacks required");
    const before = await readServerCounter();
    assert(Number.isInteger(before), "real server must expose the additive counter");
    const target = await page.evaluate(({ listId, stream, cacheKey }) => {
      const p = window.__terminatedRecovery;
      const kind = stream === "document" ? "SubscribeListDoc" : "SubscribeList";
      const sent = p.frames.findLast(x => x.direction === "out" && x.frame[kind]?.list_id === listId);
      const id = sent.frame[kind].subscription_id;
      const handshake = p.frames.findLast(x => x.direction === "in"
        && (stream === "document" ? x.frame.ListDocSubscribed?.subscription_id === id : x.frame.Subscribed?.subscription_id === id));
      if (!handshake || !Number.isInteger(id)) throw new Error("real initial subscription/handshake required");
      if (stream === "document" && !handshake.frame.ListDocSubscribed.version)
        throw new Error("real initial server version vector required");
      if (p.sockets[sent.socket].readyState !== WebSocket.OPEN) throw new Error("native socket must be OPEN");
      const baseline = { id, socket: sent.socket, token: p.token, url: location.href,
        version: handshake.frame.ListDocSubscribed?.version, cache: localStorage.getItem(cacheKey) };
      p.outage = true; p.dropUpdates = stream === "document";
      p.requests = []; p.terminatedAt = performance.now(); p.terminationSeq = p.seq;
      p.unsubscribe(sent.socket, id);
      return baseline;
    }, { listId, stream, cacheKey });
    await page.waitForFunction(({ socket, id }) => window.__terminatedRecovery.frames.some(x =>
      x.direction === "in" && !x.synthetic && x.socket === socket && x.seq > window.__terminatedRecovery.terminationSeq
      && x.frame.Unsubscribed?.subscription_id === id), { timeout: timeoutMs }, target);
    // The old relay is actually gone. Inject the exact server lookup-error text
    // into its surviving application callback, without revoking real access.
    await page.evaluate(({ socket, id }) => {
      const p = window.__terminatedRecovery; p.failedAt = performance.now(); p.fail(socket, id);
    }, target);
    if (stream === "document") {
      await commitLocalEdit();
      await page.waitForFunction(({ cacheKey, before }) => {
        const saved = localStorage.getItem(cacheKey); return saved !== null && saved !== before;
      }, { timeout: timeoutMs }, { cacheKey, before: target.cache });
      assert.equal(await readServerCounter(), before, "pending local edit has not reached the real server");
    }
    const selector = await prepareDraft();
    assert.equal(typeof selector, "string", "prepareDraft returns the actual focused quantity selector");
    const frozen = await readFrozenTrip();
    assert(frozen && JSON.stringify(frozen) !== "{}", "real frozen trip identity is required");
    const pendingCache = await page.evaluate(({ selector, cacheKey }) => {
      const p = window.__terminatedRecovery; p.draftNode = document.querySelector(selector);
      if (!p.draftNode || p.draftNode !== document.activeElement) throw new Error("real focused draft required");
      p.draftValue = p.draftNode.value; p.scroll = scrollY;
      return localStorage.getItem(cacheKey);
    }, { selector, cacheKey });
    assert(pendingCache, "pending work must have a durable cached snapshot");
    async function assertPreserved(label) {
      const state = await page.evaluate(({ cacheKey, token, url }) => {
        const p = window.__terminatedRecovery;
        return { sameDocument: p?.token === token && location.href === url,
          cache: localStorage.getItem(cacheKey), focus: p?.draftNode === document.activeElement,
          connected: p?.draftNode?.isConnected, value: p?.draftNode?.value,
          expectedValue: p?.draftValue, scroll: scrollY, expectedScroll: p?.scroll };
      }, { cacheKey, token: target.token, url: target.url });
      assert(state.sameDocument && state.connected && state.focus, `${label}: exact page and draft node/focus survive`);
      assert.equal(state.cache, pendingCache, `${label}: cached unsynced snapshot is retained exactly`);
      assert.equal(state.value, state.expectedValue, `${label}: draft retained`);
      assert.equal(state.scroll, state.expectedScroll, `${label}: scroll retained`);
      assert.deepEqual(await readFrozenTrip(), frozen, `${label}: frozen priced trip identity retained`);
    }
    await page.waitForFunction(() => window.__terminatedRecovery.requests.filter(x => x.injected).length >= 2,
      { timeout: Math.min(timeoutMs, 6500) });
    await assertPreserved("temporary 503 replies");
    const timing = await page.evaluate(() => {
      const p = window.__terminatedRecovery;
      return { first: p.requests[0].at - p.failedAt, gaps: p.requests.slice(1).map((r, i) => r.at - p.requests[i].at),
        attempts: p.requests.length, droppedUpdates: p.droppedUpdates.length };
    });
    assert(timing.first >= 500 && timing.first <= 2500, `first bounded probe ${timing.first}ms`);
    assert.equal(timing.attempts, 2, "two attempts, no request storm before the next backoff");
    assert(timing.gaps.every(gap => gap >= 1200 && gap <= 3500), `bounded second retry ${timing.gaps}`);
    assert.equal(await readServerCounter(), before, "503 recovery never submits pending work prematurely");
    const recoverySeq = await page.evaluate(() => {
      const p = window.__terminatedRecovery; p.dropUpdates = false; p.outage = false;
      p.recoveryAt = performance.now(); return p.seq;
    });
    await page.waitForFunction(({ stream, listId, target, recoverySeq }) => {
      const p = window.__terminatedRecovery;
      const kind = stream === "document" ? "SubscribeListDoc" : "SubscribeList";
      const sent = p.frames.find(x => x.direction === "out" && x.seq > recoverySeq
        && x.socket === target.socket && x.frame[kind]?.list_id === listId
        && x.frame[kind]?.subscription_id === target.id);
      if (!sent) return false;
      return p.requests.some(r => !r.injected && r.status === 200)
        && p.frames.some(x => x.direction === "in" && !x.synthetic && x.seq > sent.seq
          && x.socket === target.socket && (stream === "document"
            ? x.frame.ListDocSubscribed?.subscription_id === target.id : x.frame.Subscribed?.subscription_id === target.id));
    }, { timeout: timeoutMs }, { stream, listId, target, recoverySeq });
    if (stream === "document") {
      const version = await page.evaluate(({ id, recoverySeq }) => window.__terminatedRecovery.frames.find(x =>
        x.direction === "out" && x.seq > recoverySeq && x.frame.SubscribeListDoc?.subscription_id === id).frame.SubscribeListDoc.version,
      { id: target.id, recoverySeq });
      assert.equal(typeof version, "string", "wire version uses real base64 vector encoding");
      assert(version.length > 0, "restored native subscription carries a nonempty version vector");
      // Encoded vectors can reorder entries, so byte inequality is not semantic
      // advancement evidence. The real server counter and later remote update
      // below prove that pending work survives and converges exactly once.
      const expected = before + localDelta;
      const deadline = Date.now() + timeoutMs;
      while (await readServerCounter() !== expected && Date.now() < deadline) await sleep(100);
      assert.equal(await readServerCounter(), expected, "pending additive edit reaches real server exactly once");
      await sleep(1100);
      assert.equal(await readServerCounter(), expected, "later handshake/broadcast does not duplicate pending edit");
    }
    // Snapshot encoding may change after a real handshake; preserve nonempty
    // cached work, node/focus, draft and frozen trip rather than byte identity.
    const after = await page.evaluate(({ cacheKey, token, url }) => {
      const p = window.__terminatedRecovery;
      return { sameDocument: p.token === token && location.href === url,
        cache: !!localStorage.getItem(cacheKey), focus: p.draftNode === document.activeElement,
        draft: p.draftNode?.isConnected && p.draftNode.value === p.draftValue, seq: p.seq };
    }, { cacheKey, token: target.token, url: target.url });
    assert(after.sameDocument && after.cache && after.focus && after.draft, "successful recovery preserves page, work and focused draft");
    assert.deepEqual(await readFrozenTrip(), frozen, "successful recovery preserves frozen priced trip");
    await remoteEdit();
    await page.waitForFunction(({ target, stream, seq }) => window.__terminatedRecovery.frames.some(x => {
      const scoped = x.frame.SubscriptionEvent;
      return x.direction === "in" && !x.synthetic && x.seq > seq && x.socket === target.socket
        && scoped?.subscription_id === target.id && (stream === "document" ? scoped.event.ListDocUpdate : scoped.event.ListUpdate);
    }), { timeout: timeoutMs }, { target, stream, seq: after.seq });
    await waitRemoteConvergence();
    const finalUi = await page.evaluate(() => {
      const p = window.__terminatedRecovery;
      return { token: p.token, focus: p.draftNode === document.activeElement,
        draft: p.draftNode?.isConnected && p.draftNode.value === p.draftValue };
    });
    assert.equal(finalUi.token, target.token, "remote convergence required no reload");
    assert(finalUi.focus && finalUi.draft, "subsequent real remote edit preserves the focused draft");
    assert.deepEqual(await readFrozenTrip(), frozen, "remote convergence does not replace the frozen trip");
    assert.deepEqual(errors, [], "no application page errors during relay recovery");
    evidence = { stream, syntheticFault: "real relay Unsubscribe + synthetic scoped lookup Error and REST503",
      subscriptionId: target.id, firstProbeMs: Math.round(timing.first), retryGapsMs: timing.gaps.map(Math.round),
      droppedUpdateAttempts: timing.droppedUpdates, recovery: "real200 + same-ID native subscription and handshake + remote event" };
    if (record) await record(`terminated-${stream}-transient-recovery`, evidence);
  } catch (error) {
    originalError = error;
    throw error;
  } finally {
    const cleanupErrors = [];
    for (const action of [
      async () => { if (!page.isClosed()) await page.evaluate(() => window.__terminatedRecovery?.restore()); },
      async () => { if (preload) await page.removeScriptToEvaluateOnNewDocument(preload.identifier); },
      async () => { page.off("pageerror", onError); },
      async () => { await cleanup(); },
    ]) { try { await action(); } catch (error) { cleanupErrors.push(error); } }
    if (cleanupErrors.length) {
      if (originalError) originalError.cleanupErrors = cleanupErrors;
      else throw new AggregateError(cleanupErrors, "relay recovery cleanup failed");
    }
  }
  console.log(`[PASS] synthetic ${stream} relay termination recovers real subscription, cached work and convergence: ${JSON.stringify(evidence)}`);
  return evidence;
}

module.exports = { runTerminatedDocRecovery };
