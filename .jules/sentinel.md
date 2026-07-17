## 2024-05-24 - [Add Global Security Headers]
**Vulnerability:** Missing security headers (X-Frame-Options, X-Content-Type-Options, Strict-Transport-Security)
**Learning:** The Axum server didn't have global security headers applied, opening the door to clickjacking, MIME-sniffing and MITM attacks via plain HTTP.
**Prevention:** Always add global security headers via middleware (e.g. `SetResponseHeaderLayer` from `tower_http`) when configuring a web server.

## 2024-05-24 - [Add Timeouts to Webhook Client]
**Vulnerability:** Denial of Service (DoS) via Server Tarpitting / SSRF
**Learning:** `reqwest::Client::new()` in Rust does not have a default timeout. If a user sets a malicious webhook URL that holds the connection open, it could exhaust server resources and block other alerts from being sent.
**Prevention:** Always configure an explicit `.timeout()` when instantiating `reqwest::Client` for outbound HTTP requests to user-controlled URLs.

## 2024-05-24 - [Fix Cookie Path Scoping]
**Vulnerability:** Incomplete Cookie Scope Configuration
**Learning:** The `discord_auth` cookie was being set during a `/redirect` endpoint without an explicitly set `Path=/`. This scopes the cookie to the `/redirect` path, meaning the browser wouldn't send the auth cookie to other paths (like `/api/v1/user`), effectively breaking authentication outside that route. Other cookies in the same file were properly using `cookie.set_path("/")` or `CookieBuilder` with `.path("/")`.
**Prevention:** Always explicitly set `cookie.set_path("/")` for application-wide authentication or session cookies to ensure they are sent to all relevant routes.
