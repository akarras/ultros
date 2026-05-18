## 2025-02-14 - Fix OAuth CSRF vulnerability
**Vulnerability:** OAuth `state` parameter generated during `begin_login` was ignored during `redirect` callback, allowing Cross-Site Request Forgery (CSRF).
**Learning:** Even when using a mature library like `oauth2-rs`, the framework integration points are critical. The library provides `CsrfToken` helpers, but the application is responsible for persisting and validating them.
**Prevention:** Always ensure that generated security tokens (CSRF, Nonce, PKCE) are statefully stored (e.g., via `HttpOnly` + `Secure` cookies or session store) during the initial request, and rigorously validated on the callback.
## 2025-02-15 - Fix Information Exposure in Error Responses
**Vulnerability:** Detailed error messages were exposed to users in API and web responses via `self.to_string()` and `format!("{self}")`, potentially leaking internal server details or logic.
**Learning:** Returning `format!("{self}")` or `self.to_string()` directly in `into_response()` for `WebError` or `ApiError` exposes the full error text.
**Prevention:** Mask errors going to the client as "Internal server error" for 5xx responses, while preserving original error logs server-side via `tracing::error!`.
