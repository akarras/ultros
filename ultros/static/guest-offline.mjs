// Preparation is opt-in from the hydrated Labs device-list experience.
// No server HTML or account state is sent to the worker.
let preparing;

export async function prepareGuestOffline(catalogUrl, lang = 'en') {
  if (preparing) return preparing;
  preparing = prepare(catalogUrl, lang).catch(() => false).then(ready => {
    window.__ULTROS_GUEST_OFFLINE_READY__ = ready;
    window.dispatchEvent(new CustomEvent('ultros:guest-offline-ready', { detail: { ready } }));
    if (!ready) preparing = undefined; // allow a later explicit retry
    return ready;
  });
  return preparing;
}

async function prepare(catalogUrl, lang) {
  if (!('serviceWorker' in navigator) || !window.isSecureContext) return false;
  const paths = new Set();
  const collect = value => {
    const url = new URL(value, location.origin);
    if (url.origin === location.origin && !url.search && !url.hash
      && /^\/pkg\/.+\.(js|mjs|wasm|css)$/.test(url.pathname)) paths.add(url.pathname);
  };
  performance.getEntriesByType('resource').forEach(entry => collect(entry.name));
  document.querySelectorAll('link[href], script[src]').forEach(node => collect(node.href || node.src));
  const assets = [...paths];
  // Production places the whole build under /pkg/<build-id>/; development
  // may serve directly from /pkg/. Match the entry basename in either layout.
  const js = assets.find(path => /^\/pkg\/(?:[^/]+\/)*ultros(?:[.-][^/]*)?\.js$/.test(path));
  const wasm = assets.find(path => /^\/pkg\/(?:[^/]+\/)*ultros[^/]*\.wasm$/.test(path));
  const style = document.querySelector('link#leptos');
  const css = style ? new URL(style.href).pathname : assets.find(path => path.endsWith('.css'));
  if (!js || !wasm || !css || !catalogUrl) return false;
  // modulepreloads and resource timings include wasm-bindgen's snippet imports.
  // The explicit archive URL is required even when this load used IndexedDB.
  assets.push(catalogUrl);
  const registration = await navigator.serviceWorker.register('/service-worker.js');
  if (registration.installing) {
    const installing = registration.installing;
    await new Promise(resolve => {
      if (installing.state === 'installed' || installing.state === 'activated'
        || installing.state === 'redundant') return resolve();
      installing.addEventListener('statechange', () => {
        if (['installed', 'activated', 'redundant'].includes(installing.state)) resolve();
      });
    });
  }
  await navigator.serviceWorker.ready;
  const worker = registration.waiting || registration.active;
  if (!worker) return false;
  return new Promise(resolve => {
    const channel = new MessageChannel();
    const finish = ready => {
      clearTimeout(timeout);
      channel.port1.close();
      resolve(ready);
    };
    const timeout = setTimeout(() => finish(false), 60000);
    channel.port1.onmessage = async event => {
      if (event.data?.ready !== true) return finish(false);
      if (navigator.serviceWorker.controller?.scriptURL === worker.scriptURL
        && worker.state === 'activated') return finish(true);
      // Prepared replacements call skipWaiting; wait until this tab is actually
      // controlled before advertising that a disconnected reload is available.
      const controlled = () => {
        if (navigator.serviceWorker.controller === worker) {
          navigator.serviceWorker.removeEventListener('controllerchange', controlled);
          finish(true);
        }
      };
      navigator.serviceWorker.addEventListener('controllerchange', controlled);
      controlled();
    };
    worker.postMessage({ type: 'ULTROS_PREPARE_GUEST_OFFLINE', assets, js, wasm, css, lang }, [channel.port2]);
  });
}


export function prepare_guest_offline(catalogUrl, lang) {
  const prepare = () => {
    const path = location.pathname.replace(/\/$/, '');
    if (path !== '/list' && !path.startsWith('/list/device/')) return;
    let cookie = '';
    try {
        cookie = decodeURIComponent((document.cookie.split(';').map(s => s.trim()).find(s => s.startsWith('LABS=')) || '').slice(5));
    } catch (_) { /* an invalid cookie cannot enable an experiment */ }
    const query = new URL(location.href).searchParams.get('labs') || '';
    const enabled = [cookie, query].some(value => value.split(',').some(token => token.trim() === 'lists-sync'));
    if (!enabled && !window.__ULTROS_OFFLINE_GUEST__) return;
    prepareGuestOffline(catalogUrl, lang)
        .catch(() => { window.__ULTROS_GUEST_OFFLINE_READY__ = false; });
  };
  if (!window.__ultrosGuestOfflineListener) {
    window.__ultrosGuestOfflineListener = prepare;
    window.addEventListener('ultros:guest-list-opened', prepare);
  }
  prepare();
}
