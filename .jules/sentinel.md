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

## 2026-07-26 - [Fix Open Redirect Bypass via Whitespace]
**Vulnerability:** Open Redirect bypass
**Learning:** Browsers sometimes normalize or ignore whitespace characters like tabs (`\t`) and spaces (` `) in URL paths. A URL like `/\t/evil.com` or `/ /evil.com` starts with `/` and thus bypasses a strict `starts_with("//")` protocol-relative check, but the browser may resolve it to `//evil.com` and redirect the user.
**Prevention:** Always strip or reject raw whitespace characters (like spaces and tabs) when validating relative redirect URLs to prevent open redirect vulnerabilities.

## 2026-08-23 - [Pin Path=/ on the OAuth PKCE cookies (hardening, not a live bug)]
**Vulnerability:** None currently exploitable - defensive hardening.
**Learning:** The `pkce_challenge`, `pkce_verifier` and `oauth_state` cookies are set by `begin_login`, mounted at `/login`, and read back by the callback at `/redirect`. Both are single-segment paths, so RFC 6265 section 5.1.4's default-path algorithm already yields `Path=/` and the cookies were being sent to the callback correctly - login was never broken. The cookies are fragile to a *future* route move, though: mounting login under a nested path such as `/auth/login` would make the default-path `/auth`, and the callback at `/redirect` would silently stop receiving them.
**Prevention:** Don't rely on the RFC 6265 default-path for cookies that are read from a different route than the one that sets them - state it explicitly with `.path("/")`. Also: verify a cookie-scoping claim against the route table and the default-path rules before filing it as a vulnerability; "no explicit Path" is not by itself a bug.
