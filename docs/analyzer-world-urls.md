# Analyzer world URLs

Recipe, Venture, Leve, and Scrip Sources use `/<tool>/<world>`, matching FC
Crafting's world path. Bare tool routes remain available for visitors who have
not chosen a world.

Selection is resolved on the server and reactively in the browser:

1. A path world takes precedence over a legacy `?world=` parameter.
2. On a bare route, a legacy query world takes precedence over the home-world cookie.
3. If that explicit choice is absent or invalid, use the home world. An invalid
   path never lets a conflicting query world override it.
4. Without a resolved choice or cookie, leave the picker unselected.

After hydration, legacy links and cookie fallbacks replace the URL with the
canonical world path. World-picker changes push history, so Back and Forward
restore the picker and data sources. These navigations retain encoded query
values, repeated unrelated parameters, and the fragment, removing only the
legacy `world` query. Filter edits still replace history without scrolling.

Saved views remain portable: old stored `world` parameters are ignored when a
view is applied to a world path. Automatic last-view preferences retain their
existing per-tool keys and cookies.

Data scopes still have distinct roles. Venture and Leve use regional ingredient
listings/shared statistics and world-specific recent sales. Scrip Sources uses
regional listings/statistics. Recipe uses the selected world's region,
datacenter, or world according to its existing buy/sell scope controls.

`integration/analyzer-world-urls.cjs` exercises SSR, hydration, legacy links,
conflicting cookies, same/cross-region changes, non-ASCII worlds, saved views,
reload, and browser history using deterministic browser market responses. It
runs through `scripts/run_e2e.sh` or `npm --prefix integration run
test:analyzer-world-urls` against a freshly built local server.
