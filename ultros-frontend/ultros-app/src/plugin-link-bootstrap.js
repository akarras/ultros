// Runs before application startup, with third-party scripts disabled by the
// plugin document. The URL credential never reaches an HTTP URL or storage API.
(() => {
  function capture() {
  if (location.pathname !== "/plugin/verify" || !location.hash.startsWith("#pair=")) return false;
  const fragment = location.hash.slice(6);
  history.replaceState(history.state, "", location.pathname + location.search);
  const token = /^[A-Za-z0-9_-]{43}$/.test(fragment) ? fragment : "";
  const controller = new AbortController();
  const timeout = setTimeout(() => controller.abort(), 15000);
  window.__ULTROS_PLUGIN_LINK_CAPTURE__ = fetch("/api/v1/plugin/browser/link", {
    method: "POST",
    credentials: "same-origin",
    redirect: "error",
    cache: "no-store",
    headers: { "Content-Type": "application/x-www-form-urlencoded" },
    body: "token=" + encodeURIComponent(token),
    signal: controller.signal,
  }).then(response => response.ok ? null : response.status === 429
    ? "Too many link attempts. Wait a few minutes, then try again."
    : "This connection link is invalid or has expired. Start a new connection in game, or use its short code.")
    .catch(() => "Could not prepare the connection link. Open it again, or use the short code from your game.")
    .finally(() => clearTimeout(timeout));
  return true;
  }
  window.__ULTROS_CAPTURE_PLUGIN_LINK__ = capture;
  capture();
  addEventListener("hashchange", () => {
    if (capture()) window.__ULTROS_PLUGIN_LINK_CAPTURE__.then(() => location.reload());
  });
})();
