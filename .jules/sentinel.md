## 2025-02-20 - Set HttpOnly Flag for Auth and PKCE Cookies
**Vulnerability:** The authentication cookies (`discord_auth`, `pkce_challenge`, `pkce_verifier`) did not have the `HttpOnly` flag set, making them accessible to client-side scripts and vulnerable to Cross-Site Scripting (XSS) attacks.
**Learning:** Even if cookies are set with `Secure` and `SameSite` flags, sensitive cookies like authentication and PKCE tokens must also use the `HttpOnly` flag to prevent client-side access.
**Prevention:** Always verify that the `HttpOnly` flag is explicitly enabled when setting session or authentication cookies via `CookieBuilder` or `Cookie::new`.
