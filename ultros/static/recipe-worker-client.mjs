// One running request and one latest pending request per mounted planner.
// Payloads stay JSON strings so 64-bit listing IDs never pass through JS numbers.
export function runtimeAssets() {
  const paths = new Set();
  const collect = value => {
    const url = new URL(value, location.origin);
    if (url.origin === location.origin && !url.search && !url.hash) paths.add(url.href);
  };
  performance.getEntriesByType('resource').forEach(entry => collect(entry.name));
  document.querySelectorAll('link[href], script[src]').forEach(node => collect(node.href || node.src));
  const js = [...paths].find(path => /\/pkg\/(?:[^/]+\/)*ultros(?:[.-][^/]*)?\.js$/.test(new URL(path).pathname));
  const wasm = [...paths].find(path => /\/pkg\/(?:[^/]+\/)*ultros[^/]*\.wasm$/.test(new URL(path).pathname));
  if (!js || !wasm) throw new Error('Recipe runtime assets are unavailable; reload the page.');
  return { js, wasm };
}

export class PlannerQueue {
  constructor({ spawn, assets = runtimeAssets, compiled, deadlineMs = 30000 } = {}) {
    this.spawn = spawn || (assets => {
      const url = new URL('/static/recipe-planner-worker.mjs', location.origin);
      url.searchParams.set('runtime', assets.js);
      return new Worker(url, { type: 'module', name: 'recipe-planner' });
    });
    this.assets = assets;
    this.compiled = compiled;
    this.deadlineMs = deadlineMs;
    this.serial = 0;
    this.closed = false;
    this.active = null;
    this.pending = null;
    this.worker = null;
  }
  submit(payload) {
    if (this.closed) return Promise.reject(new Error('Planner disposed'));
    return new Promise((resolve, reject) => {
      const job = { id: ++this.serial, payload, resolve, reject };
      this.pending?.reject(new Error('Planner request superseded'));
      this.active?.reject(new Error('Planner request superseded'));
      this.pending = job;
      this.pump();
    });
  }
  pump() {
    if (this.closed || this.active || !this.pending) return;
    const job = this.pending;
    this.pending = null;
    this.active = job;
    try {
      const assets = this.assets();
      if (!this.worker) {
        this.worker = this.spawn(assets);
        this.worker.onmessage = event => this.finish(event.data);
        this.worker.onerror = event => {
          event.preventDefault?.();
          this.fail(new Error(`Recipe worker failed: ${event.message || 'load error'}`));
        };
        this.worker.onmessageerror = () => this.fail(new Error('Recipe worker response could not be read.'));
      }
      this.timer = setTimeout(() => this.fail(new Error('Recipe planning timed out.')), this.deadlineMs);
      const message = { id: job.id, payload: job.payload, runtime: assets, compiled: this.compiled };
      try { this.worker.postMessage(message); }
      catch (error) {
        // A browser that cannot clone modules can still fetch the same runtime
        // in its worker; it never falls back to calculating on the UI thread.
        if (message.compiled && error.name === 'DataCloneError') {
          this.worker.postMessage({ ...message, compiled: undefined });
        } else { throw error; }
      }
    } catch (error) { this.fail(error); }
  }
  finish(message) {
    if (!this.active || message.id !== this.active.id) return;
    clearTimeout(this.timer);
    const job = this.active;
    this.active = null;
    if (message.error) {
      console.warn("Recipe planning failed:", message.error);
      job.reject(new Error(message.error));
      this.worker?.terminate();
      this.worker = null;
    } else if (typeof message.result === 'string') {
      job.resolve(message.result);
    } else {
      job.reject(new Error('Invalid recipe worker response'));
    }
    this.pump();
  }
  fail(error) {
    console.warn("Recipe planning failed:", String(error));
    clearTimeout(this.timer);
    this.worker?.terminate();
    this.worker = null;
    this.active?.reject(error);
    this.active = null;
    this.pump();
  }
  dispose() {
    this.closed = true;
    clearTimeout(this.timer);
    this.worker?.terminate();
    this.worker = null;
    this.active?.reject(new Error('Planner disposed'));
    this.pending?.reject(new Error('Planner disposed'));
    this.active = this.pending = null;
  }
}

const planners = new Map();
let serial = 0;
export function createRecipePlanner(compiled) {
  const id = ++serial;
  planners.set(id, new PlannerQueue({ compiled }));
  return id;
}
export function planRecipe(id, payload) {
  const planner = planners.get(id);
  return planner ? planner.submit(payload) : Promise.reject(new Error('Planner disposed'));
}
export function disposeRecipePlanner(id) {
  planners.get(id)?.dispose();
  planners.delete(id);
}
