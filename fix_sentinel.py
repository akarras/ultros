import re

with open(".jules/sentinel.md", "r") as f:
    content = f.read()

# Replace the block I added at the end with an accurate description
new_text = """## 2024-05-24 - [Fix OAuth Cookie Path Scoping]
**Vulnerability:** Broken Authentication Flow
**Learning:** The `pkce_challenge`, `pkce_verifier`, and `oauth_state` cookies were being set during the `/begin_login` endpoint without an explicitly set `Path=/`. This scopes the cookie to the `/begin_login` path, meaning the browser wouldn't send them to the OAuth callback path, breaking the authentication flow. These are temporary, single-use cookies required strictly for the OAuth callback endpoint to verify the login attempt.
**Prevention:** Always explicitly set `.path("/")` for temporary OAuth cookies if the callback endpoint is on a different path, to ensure they are sent during the redirect."""

content = re.sub(r"## 2024-05-24 - \[Fix Cookie Path Scoping\].*?Always explicitly set `\.path\(\"/\"\)` for application-wide authentication or session cookies to ensure they are sent to all relevant routes\.", new_text, content, flags=re.DOTALL)

# Re-write the file
with open(".jules/sentinel.md", "w") as f:
    f.write(content)
