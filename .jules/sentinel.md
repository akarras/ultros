## 2024-05-24 - [Add Global Security Headers]
**Vulnerability:** Missing security headers (X-Frame-Options, X-Content-Type-Options, Strict-Transport-Security)
**Learning:** The Axum server didn't have global security headers applied, opening the door to clickjacking, MIME-sniffing and MITM attacks via plain HTTP.
**Prevention:** Always add global security headers via middleware (e.g. `SetResponseHeaderLayer` from `tower_http`) when configuring a web server.
## 2024-05-24 - [Add Global Content Security Policy (CSP)]
**Vulnerability:** Missing Content Security Policy (CSP) header
**Learning:** The Axum server didn't have a Content-Security-Policy header applied, opening the door to XSS, clickjacking, and data injection attacks by allowing execution of untrusted scripts.
**Prevention:** Always add a `CONTENT_SECURITY_POLICY` global security header via middleware (e.g. `SetResponseHeaderLayer` from `tower_http`) when configuring a web server.
