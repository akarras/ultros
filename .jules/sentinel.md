## 2024-05-24 - [Add Global Security Headers]
**Vulnerability:** Missing security headers (X-Frame-Options, X-Content-Type-Options, Strict-Transport-Security)
**Learning:** The Axum server didn't have global security headers applied, opening the door to clickjacking, MIME-sniffing and MITM attacks via plain HTTP.
**Prevention:** Always add global security headers via middleware (e.g. `SetResponseHeaderLayer` from `tower_http`) when configuring a web server.

## 2026-06-17 - [Add Missing Security Headers]
**Vulnerability:** Missing security headers (Content-Security-Policy, Referrer-Policy, X-XSS-Protection)
**Learning:** The Axum server didn't have all recommended global security headers applied, opening the door to XSS attacks and exposing referrer information. While some headers were present, others were missing.
**Prevention:** Always add a complete set of global security headers via middleware (e.g. `SetResponseHeaderLayer` from `tower_http`) when configuring a web server, including CSP, Referrer-Policy, and XSS protection.
