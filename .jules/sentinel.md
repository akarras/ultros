## 2024-05-24 - [Add Global Security Headers]
**Vulnerability:** Missing security headers (X-Frame-Options, X-Content-Type-Options, Strict-Transport-Security)
**Learning:** The Axum server didn't have global security headers applied, opening the door to clickjacking, MIME-sniffing and MITM attacks via plain HTTP.
**Prevention:** Always add global security headers via middleware (e.g. `SetResponseHeaderLayer` from `tower_http`) when configuring a web server.

## 2026-06-17 - [Add Missing Security Headers - REJECTED]
**Vulnerability:** Adding restrictive Content-Security-Policy (CSP) headers without considering external dependencies.
**Learning:** A restrictive CSP (`default-src 'self'`) breaks external resources like Google Analytics, Sentry/GlitchTip error reporting, and Google AdSense iframes. `X-XSS-Protection` is deprecated and can introduce vulnerabilities. `Referrer-Policy: strict-origin-when-cross-origin` is the browser default.
**Prevention:** If adding a CSP, always use `Content-Security-Policy-Report-Only` first to monitor violations in production. Ensure allowlists include necessary domains for scripts and frames (e.g., `googletagmanager.com`, `sentry-cdn.com`, `googleads.g.doubleclick.net`). Do not add deprecated headers like `X-XSS-Protection: 1`.
