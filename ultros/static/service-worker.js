// Ultros Web Push service worker.
//
// Served from `/service-worker.js` (root scope) so notifications can land
// regardless of which page registered it. Backend wires the
// Service-Worker-Allowed: / response header so this is allowed.

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
  const url = (event.notification.data && event.notification.data.url) || '/alerts';
  event.waitUntil(
    clients.matchAll({ type: 'window' }).then((wins) => {
      for (const w of wins) {
        if (w.url.endsWith(url) && 'focus' in w) return w.focus();
      }
      return clients.openWindow(url);
    })
  );
});
