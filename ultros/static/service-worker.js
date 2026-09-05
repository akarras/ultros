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
