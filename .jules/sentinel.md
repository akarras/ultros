## 2024-05-24 - [Add Global Security Headers]
**Vulnerability:** Missing security headers (X-Frame-Options, X-Content-Type-Options, Strict-Transport-Security)
**Learning:** The Axum server didn't have global security headers applied, opening the door to clickjacking, MIME-sniffing and MITM attacks via plain HTTP.
**Prevention:** Always add global security headers via middleware (e.g. `SetResponseHeaderLayer` from `tower_http`) when configuring a web server.

## 2024-05-24 - [Add Timeouts to Webhook Client]
**Vulnerability:** Denial of Service (DoS) via Server Tarpitting / SSRF
**Learning:** `reqwest::Client::new()` in Rust does not have a default timeout. If a user sets a malicious webhook URL that holds the connection open, it could exhaust server resources and block other alerts from being sent.
**Prevention:** Always configure an explicit `.timeout()` when instantiating `reqwest::Client` for outbound HTTP requests to user-controlled URLs.
