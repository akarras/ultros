## 2024-05-24 - [Add Global Security Headers]
**Vulnerability:** Missing security headers (X-Frame-Options, X-Content-Type-Options, Strict-Transport-Security)
**Learning:** The Axum server didn't have global security headers applied, opening the door to clickjacking, MIME-sniffing and MITM attacks via plain HTTP.
**Prevention:** Always add global security headers via middleware (e.g. `SetResponseHeaderLayer` from `tower_http`) when configuring a web server.

## 2024-05-24 - [Add Timeouts to Webhook Client]
**Vulnerability:** Denial of Service (DoS) via Server Tarpitting / SSRF
**Learning:** `reqwest::Client::new()` in Rust does not have a default timeout. If a user sets a malicious webhook URL that holds the connection open, it could exhaust server resources and block other alerts from being sent.
**Prevention:** Always configure an explicit `.timeout()` when instantiating `reqwest::Client` for outbound HTTP requests to user-controlled URLs.

## 2024-05-24 - [Fix OAuth Cookie Path Scoping]
**Vulnerability:** Broken Authentication Flow
**Learning:** The `pkce_challenge`, `pkce_verifier`, and `oauth_state` cookies were being set during the `/begin_login` endpoint without an explicitly set `Path=/`. This scopes the cookie to the `/begin_login` path, meaning the browser wouldn't send them to the OAuth callback path, breaking the authentication flow. These are temporary, single-use cookies required strictly for the OAuth callback endpoint to verify the login attempt.
**Prevention:** Always explicitly set `.path("/")` for temporary OAuth cookies if the callback endpoint is on a different path, to ensure they are sent during the redirect.
