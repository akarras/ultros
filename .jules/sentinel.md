## 2025-02-14 - Fix OAuth CSRF vulnerability
**Vulnerability:** OAuth `state` parameter generated during `begin_login` was ignored during `redirect` callback, allowing Cross-Site Request Forgery (CSRF).
**Learning:** Even when using a mature library like `oauth2-rs`, the framework integration points are critical. The library provides `CsrfToken` helpers, but the application is responsible for persisting and validating them.
**Prevention:** Always ensure that generated security tokens (CSRF, Nonce, PKCE) are statefully stored (e.g., via `HttpOnly` + `Secure` cookies or session store) during the initial request, and rigorously validated on the callback.
