// Ultros Web Push service worker.
//
// Served from `/service-worker.js` (root scope) so notifications can land
// regardless of which page registered it. Backend wires the
// Service-Worker-Allowed: / response header so this is allowed.

// Device-list offline support deliberately NEVER stores server-rendered HTML,
// current_user, account APIs, or market responses. The only navigation fallback
// is a generated anonymous client-rendered shell, prepared after a successful
// online load. Device documents remain primary data in their separate IndexedDB.
const GUEST_META = 'ultros-guest-offline-v2';
const GUEST_GENERATION_PREFIX = 'ultros-guest-offline-v2-generation-';
const GUEST_ACTIVE = '/__ultros_guest_offline_active__';
const GUEST_SHELL = '/__ultros_guest_offline_shell__';

async function activeGuestGeneration() {
  const response = await (await caches.open(GUEST_META)).match(GUEST_ACTIVE);
  if (!response) return null;
  const name = await response.text();
  return name.startsWith(GUEST_GENERATION_PREFIX) ? name : null;
}

async function cachedGuestResource(request) {
  const read = async () => {
    const name = await activeGuestGeneration();
    // Never resurrect the previous non-transactional v1 cache: it may contain
    // mixed dependencies after an interrupted write.
    return name ? (await caches.open(name)).match(request) : undefined;
  };
  // Keep cleanup from deleting the chosen generation between pointer and body
  // reads. Existing cached support is still readable without the optional API.
  return self.navigator?.locks
    ? self.navigator.locks.request('ultros-guest-offline-prepare-v2', { mode: 'shared' }, read)
    : read();
}

async function pruneGuestGenerations(active) {
  const names = await caches.keys();
  await Promise.all(names.filter(name =>
    (name.startsWith(GUEST_GENERATION_PREFIX) && name !== active)
    || name === 'ultros-guest-offline-v1').map(name => caches.delete(name)));
}

async function prepareGuestOffline(message) {
  // WorkerNavigator exposes the same origin-scoped Web Locks API as windows.
  // A lock spans old/new worker instances as well as competing page requests;
  // an in-memory queue alone cannot protect cleanup during worker replacement.
  // Unsupported browsers keep any existing generation and report not-ready.
  if (!self.navigator?.locks) throw new Error('Offline preparation requires Web Locks');
  return self.navigator.locks.request('ultros-guest-offline-prepare-v2', async () => {
    await pruneGuestGenerations(await activeGuestGeneration());
    return stageGuestOffline(message);
  });
}

function publicGuestAsset(value) {
  const url = new URL(value, self.location.origin);
  if (url.origin !== self.location.origin || url.search || url.hash) return false;
  return /^\/pkg\/.+\.(?:js|wasm|css)$/.test(url.pathname)
    || /^\/static\/data\/[^/]+\/(?:en|ja|de|fr|cn|ko|tc)\.rkyv$/.test(url.pathname)
    || /^\/static\/(?:guest-list-store|list-companion|guest-offline)\.mjs$/.test(url.pathname)
    || url.pathname === '/api/v1/world_data';
}

function guestNavigation(pathname) {
  return pathname === '/list' || pathname === '/list/'
    || /^\/list\/device\/[^/]+\/?$/.test(pathname);
}

function scriptJson(value) {
  return JSON.stringify(value).replace(/</g, '\\u003c');
}

async function stageGuestOffline(message) {
  const { js, wasm, css } = message;
  for (const [value, suffix] of [[js, '.js'], [wasm, '.wasm'], [css, '.css']]) {
    if (typeof value !== 'string' || !publicGuestAsset(value)
      || !new URL(value, self.location.origin).pathname.startsWith('/pkg/')
      || !value.endsWith(suffix)) throw new Error('Invalid offline build manifest');
  }
  if (!Array.isArray(message.assets) || message.assets.length > 256
    || !message.assets.every(value => typeof value === 'string' && publicGuestAsset(value))) {
    throw new Error('Invalid offline asset manifest');
  }
  const paths = [...new Set([...message.assets, js, wasm, css,
    '/static/guest-list-store.mjs', '/static/list-companion.mjs', '/api/v1/world_data'])];
  // Fetch without cookies even when preparation starts in a signed-in tab.
  // Do not publish the shell until every required resource has been saved.
  const responses = await Promise.all(paths.map(async path => {
    const response = await fetch(new Request(new URL(path, self.location.origin), {
      credentials: 'omit', cache: 'reload',
    }));
    if (!response.ok || response.redirected) throw new Error('Offline resource unavailable');
    return [path, response];
  }));
  const worlds = await responses.find(([path]) => path === '/api/v1/world_data')[1].clone().json();
  const bootstrap = { world_data: worlds, region: '', current_user: null };
  const lang = /^(en|ja|de|fr|cn|ko|tc)$/.test(message.lang) ? message.lang : 'en';
  // All dynamic HTML attributes are restricted to a locale or emitted by DOM JS.
  const html = `<!doctype html><html lang="${lang}" translate="no" data-theme="dark" data-palette="ultros"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>Device lists · Ultros</title><script>window.__ULTROS_OFFLINE_GUEST__=true;window.__ULTROS_BOOTSTRAP__=${scriptJson(bootstrap)};</script><script type="module">const style=document.createElement('link');style.rel='stylesheet';style.href=${scriptJson(css)};document.head.append(style);try {const app=await import(${scriptJson(js)});await app.default({module_or_path:${scriptJson(wasm)}});app.hydrate();}catch(error){document.body.textContent='Could not open offline lists. Reconnect and reopen Lists to refresh offline support. Your saved lists remain on this device.';}</script></head><body class="notranslate"><p id="offline-boot-status">Opening device lists…</p></body></html>`;
  const generation = GUEST_GENERATION_PREFIX + self.crypto.randomUUID();
  const cache = await caches.open(generation);
  let published = false;
  try {
    // Sequential writes ensure there are no pending puts when failed staging
    // is deleted. The published generation is never modified in place.
    for (const [path, response] of responses) await cache.put(path, response);
    await cache.put(GUEST_SHELL, new Response(html, {
      headers: { 'Content-Type': 'text/html; charset=utf-8', 'Cache-Control': 'no-store' },
    }));
    // Cache.put replaces this one response atomically. Quota/termination before
    // publication leaves the old pointer and every old dependency untouched.
    await (await caches.open(GUEST_META)).put(GUEST_ACTIVE, new Response(generation));
    published = true;
    // Cleanup failure must not turn a successfully published generation into a
    // reported failure. The next locked preparation also prunes abandoned work.
    await pruneGuestGenerations(generation).catch(() => {});
  } finally {
    if (!published) await caches.delete(generation).catch(() => {});
  }
}

self.addEventListener('message', event => {
  if (event.data?.type !== 'ULTROS_PREPARE_GUEST_OFFLINE') return;
  event.waitUntil(prepareGuestOffline(event.data).then(
    async () => {
      await self.skipWaiting();
      event.ports?.[0]?.postMessage({ ready: true });
    },
    () => event.ports?.[0]?.postMessage({ ready: false }),
  ));
});

self.addEventListener('activate', event => {
  event.waitUntil(self.clients.claim());
});

self.addEventListener('fetch', event => {
  const request = event.request;
  const url = new URL(request.url);
  if (request.method !== 'GET' || url.origin !== self.location.origin) return;
  if (request.mode === 'navigate' && guestNavigation(url.pathname)) {
    event.respondWith(fetch(request).catch(async () => {
      const cached = await cachedGuestResource(GUEST_SHELL);
      return cached || Response.error();
    }));
  } else if (publicGuestAsset(url.href)) {
    // Public files still use the network online so a deployment is not pinned
    // to an old build. Only explicitly prepared assets are fallback candidates.
    event.respondWith(fetch(request).catch(async () => {
      const cached = await cachedGuestResource(request);
      return cached || Response.error();
    }));
  }
});

self.addEventListener('push', (event) => {
  if (!event.data) return;
  let data;
  try {
    data = event.data.json();
  } catch (_e) {
    data = { title: 'Ultros', body: event.data.text() };
  }
  const title = data.title || 'Ultros';
  const options = {
    body: data.body || '',
    icon: '/static/android-chrome-192x192.png',
    badge: '/static/favicon-32x32.png',
    data: { url: data.url || '/alerts' },
  };
  event.waitUntil(self.registration.showNotification(title, options));
});

self.addEventListener('notificationclick', (event) => {
  event.notification.close();
  const requestedUrl = (event.notification.data && event.notification.data.url) || '/alerts';
  const requestedTarget = new URL(requestedUrl, self.location.origin);
  const target = requestedTarget.origin === self.location.origin
    ? requestedTarget
    : new URL('/alerts', self.location.origin);
  event.waitUntil(
    clients.matchAll({ type: 'window', includeUncontrolled: true }).then(async (wins) => {
      const exactMatch = wins.find((win) => win.url === target.href);
      if (exactMatch && 'focus' in exactMatch) return exactMatch.focus();

      // Installed/mobile app contexts often reuse their one existing window.
      // Explicitly navigate that client so focusing it does not leave the user
      // on whichever page happened to be open when the notification arrived.
      const appWindow = wins.find((win) => new URL(win.url).origin === target.origin);
      if (appWindow && 'navigate' in appWindow) {
        const navigated = await appWindow.navigate(target.href);
        const windowToFocus = navigated || appWindow;
        if ('focus' in windowToFocus) return windowToFocus.focus();
      }

      return clients.openWindow(target.href);
    })
  );
});
