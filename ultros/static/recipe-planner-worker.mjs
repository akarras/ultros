// Import the same versioned WASM used by the page, without calling hydrate().
// Planning uses only the owned request; it never needs DOM or game-data packs.
let runtime;
self.onmessage = async event => {
  const { id, payload, runtime: assets, compiled } = event.data;
  try {
    runtime ||= import(assets.js).then(async module => {
      await module.default({ module_or_path: compiled || assets.wasm });
      return module;
    });
    const module = await runtime;
    const result = module.recipe_plan(payload);
    self.postMessage({ id, result });
  } catch (error) {
    self.postMessage({ id, error: String(error) });
  }
};
