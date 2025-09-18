# Puppeteer E2E for Ultros (Leptos)

This folder contains a lightweight Puppeteer harness for screenshot-driven E2E checks of key pages (desktop and mobile breakpoints). It integrates with cargo-leptos so you can run everything from the Rust side when you want.

## What it does

- Launches Chromium via Puppeteer
- Visits a curated set of routes from the Ultros app
- Takes full-page screenshots at:
  - Desktop: 1280×800
  - Mobile: 390×844 (isMobile=true)
- Saves PNGs to `ultros/integration/artifacts` for quick visual review

## Prerequisites

- Rust toolchain and cargo-leptos installed
- Node.js 18+ and npm installed
- Internet access (Puppeteer will download a compatible Chromium on first install)

## First-time setup

From the repo root:

```bash
cd ultros/integration
npm install
```

This installs Puppeteer and pins a Chromium build for consistent results.

## Recommended workflow (fast iteration)

Terminal A: build and run the app

```bash
# from repo root
cargo leptos build
cargo leptos serve
```

The app serves at the address configured in Cargo.toml workspace metadata:
- Default: http://127.0.0.1:8080

Terminal B: run Puppeteer against the live server

```bash
# Desktop run
cd ultros/integration
npm run test:desktop

# Mobile run
npm run test:mobile
```

Screenshots will appear under:
- `ultros/integration/artifacts/*.png`

Tip: To point at a different server/port:

- macOS/Linux:
  ```bash
  BASE_URL=http://127.0.0.1:3000 npm run test:mobile
  ```

- Windows PowerShell:
  ```powershell
  $env:BASE_URL="http://127.0.0.1:3000"; npm run test:mobile
  ```

## Using cargo-leptos to drive E2E

This workspace is configured so cargo-leptos knows where the E2E tests live and how to run them:

- end2end-dir: `integration`
- end2end-cmd: `npm test`

You can invoke the E2E runner from the Rust side:

```bash
# Common on recent cargo-leptos
cargo leptos end-to-end

# If your cargo-leptos version uses the short alias:
cargo leptos end2end
```

Behavior:
- Builds the project
- Serves the app (using site-address from Cargo.toml)
- Runs `npm test` inside `ultros/integration`

If your local cargo-leptos doesn’t support the subcommand, use the Recommended workflow above.

## Tailwind/leptos note

When iterating on styling and responsive issues, prefer:

```bash
cargo leptos build
```

This will surface Tailwind or style processing errors early, and primes the target for faster subsequent serves/tests.

## Customizing routes

The current route list is defined inside `ultros/integration/package.json` in the `scripts.run` inline script:

```js
const routes = ['/', '/items', '/flip-finder', '/flip-finder/Gilgamesh', '/analyzer', '/list', '/retainers', '/currency-exchange', '/history', '/settings', '/privacy', '/cookie-policy'];
```

Edit that array to add/remove pages you care about. Re-run `npm run test:desktop` or `npm run test:mobile` to generate fresh screenshots.

## Changing viewport/device

- Desktop: 1280×800
- Mobile: 390×844, `isMobile: true`, `deviceScaleFactor: 2`

You can tweak these inside the same inline script by changing the `viewport` object.

## Artifacts

- Output folder: `ultros/integration/artifacts`
- Naming: `{route}-{mobile|desktop}.png` (route slashes sanitized)

Clean it out anytime:

```bash
rm -rf ultros/integration/artifacts
```

(Windows PowerShell)

```powershell
Remove-Item -Recurse -Force ultros/integration/artifacts
```

## Troubleshooting

- First run is slow: Puppeteer downloads Chromium; this is expected.
- Port mismatch: Set `BASE_URL` to match your running server.
- Antivirus/Corp device: Chromium download or launch can be blocked; use your system Chrome by setting `PUPPETEER_EXECUTABLE_PATH` and adjusting `puppeteer.launch()` accordingly.
- Flaky waits: The runner uses `waitUntil: 'networkidle0'` and then a short `waitForTimeout(1000)`. If pages hydrate slower locally, bump the timeout.

## CI notes

In CI, you can do:

```bash
# rust build first (faster subsequent serve)
cargo leptos build

# install node deps and run tests headless
cd ultros/integration
npm ci
npm test
```

Ensure the CI environment exposes the correct BASE_URL (default http://127.0.0.1:8080), or run through `cargo leptos end-to-end` if supported there.