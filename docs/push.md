# Web Push setup

Ultros supports Web Push as a notification delivery method (alongside Discord
DM, Discord channel, and webhook). Browsers that have subscribed receive
notifications via the standard Push API; the backend signs and encrypts each
push using VAPID + aes128gcm via the [`web-push`] crate.

[`web-push`]: https://crates.io/crates/web-push

## Generating VAPID keys (one-time)

VAPID keys are a P-256 keypair. **Do not regenerate them after deployment** —
every active browser subscription is tied to the public key, so rotating it
forces every user to re-subscribe.

### Option A — openssl

```bash
# Private key (PEM, SEC1).
openssl ecparam -name prime256v1 -genkey -noout -out vapid_private.pem

# Public key, uncompressed point, base64url-encoded with no padding.
# This is the format browsers expect for `applicationServerKey`.
openssl ec -in vapid_private.pem -pubout -outform DER 2>/dev/null \
  | tail -c 65 \
  | base64 \
  | tr '+/' '-_' \
  | tr -d '='
```

### Option B — Node `web-push` helper

```bash
npx web-push generate-vapid-keys --json
```

Whichever you use: keep the PEM private key secret, treat it like a database
password. Put it in your secrets manager / .env / etc. Anybody with the
private key can forge push notifications to your users.

## Environment variables

The web binary reads three env vars at startup:

| Name | Description |
|---|---|
| `VAPID_PUBLIC_KEY` | Base64url-encoded uncompressed P-256 public key (88 chars). |
| `VAPID_PRIVATE_KEY` | PEM contents of the private key (multi-line; quote in `.env`). |
| `VAPID_CONTACT_EMAIL` | `mailto:` URI placed in the JWT `sub` claim, e.g. `mailto:ops@ultros.app`. Some push services reject pushes without this. |

If any of the three are missing, Web Push is **disabled** — the bot/web still
start, but:

* `GET /api/v1/push/vapid-public-key` returns 503.
* `POST /api/v1/push/subscribe` returns an error.
* Existing `WebPush` endpoint rows fail at delivery time with a clear message
  in `alert_event.delivery_error`.

This is intentional so contributors can run Ultros locally without setting up
keys.

## Operational notes

* The service worker is served from `/service-worker.js` (not
  `/static/service-worker.js`) so it gets site-wide scope. The
  `Service-Worker-Allowed: /` header is set on that response.
* Push subscriptions that the browser revokes (user disabled notifications,
  uninstalled the PWA, cleared site data, etc.) come back from the push
  service as `EndpointNotFound`/`EndpointNotValid`. The delivery path
  soft-deletes those `push_subscription` rows automatically.
* `notification_endpoint` rows of method `WebPush` cannot be created via
  `POST /api/v1/endpoints` — they must come from
  `POST /api/v1/push/subscribe`, since the row is useless without an
  accompanying `push_subscription`.
