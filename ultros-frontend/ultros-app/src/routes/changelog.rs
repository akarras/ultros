use crate::components::app_link::AppLink;
use crate::components::icon::Icon;
use crate::components::meta::{MetaDescription, MetaTitle};
use crate::global_state::changelog::use_mark_changelog_seen;
use crate::i18n::*;
use icondata as i;
use leptos::prelude::*;

/// One shipped, user-visible change, written for players rather than for
/// people reading the commit log.
///
/// `date` is an ISO-8601 `YYYY-MM-DD` string and is rendered verbatim. It is
/// both what the reader sees and what the last-seen cookie is compared
/// against. Formatting it on the client (locale-aware month names, "3 days
/// ago") would produce different text on the server and the client and take
/// hydration down with it — this repo has been bitten by that more than once —
/// and ISO dates read the same in every locale we ship.
#[derive(Clone, Copy, PartialEq)]
pub struct ChangelogEntry {
    pub date: &'static str,
    pub title: &'static str,
    pub blurb: &'static str,
    /// Where to go to use the thing, when there is one obvious place.
    pub link: Option<&'static str>,
}

/// Newest first. Both [`latest_changelog_date`] and the sidebar's what's-new
/// dot depend on that ordering; `entries_are_sorted_newest_first` guards it.
///
/// Append new entries at the top when you ship something a player would
/// notice. Purely internal work (refactors, dependency bumps, CI) does not
/// belong here.
pub const CHANGELOG: &[ChangelogEntry] = &[
    ChangelogEntry {
        date: "2026-09-04",
        title: "Analytics recovers after temporary connection failures",
        blurb: "Market analytics now retries temporary connection failures, and repeated sales no longer inflate chart totals while the database catches up. During updates, Ultros gives pending analytics writes time to finish saving.",
        link: Some("/items"),
    },
    ChangelogEntry {
        date: "2026-09-03",
        title: "Recipe Analyzer: one Labs toggle, plus Profit/day, a price trend, its drift, and 30-day volume and VWAP",
        blurb: "The Recipe Analyzer's two Labs toggles are now one, \"Recipe Analyzer: the market model\" under Settings › Labs — turn it back on there if you had either of the old ones. It carries everything they did, plus five new columns in the Columns picker: Profit/day (profit times how fast the item sells), Trend (the last 7 days of prices as a sparkline), Drift (how far that trend moved), and Volume (30d) and VWAP (30d) for a longer view than the 7-day pair. Trend and Drift load only for the rows you can see; the 30-day columns load their own data the first time you show one. Price now says how it compares with the sell world's 7-day median, and the Columns picker groups everything under Market and Location.",
        link: Some("/settings"),
    },
    ChangelogEntry {
        date: "2026-09-03",
        title: "Active listings are tidier to browse",
        blurb: "The listings table's Show more control now matches sale history and stays outside the table's scroll area. Datacenter exclusions use one clear, theme-aware toggle per datacenter, show how many listings remain, and offer a reset instead of leaving an empty table behind. They also stay scoped to the listings table, so filtering it no longer blanks the price summary or other item-page sections.",
        link: Some("/items"),
    },
    ChangelogEntry {
        date: "2026-09-03",
        title: "Being signed out no longer leaves you stuck",
        blurb: "When Discord expired or revoked your login, or when you deleted your account, Ultros often failed to finish signing you out: the site kept showing you as signed in while everything that needed your account quietly failed, and the only way out was clearing your browser's cookies by hand. You are now signed out properly the moment it happens, whatever page you were on.",
        link: None,
    },
    ChangelogEntry {
        date: "2026-09-02",
        title: "Recipe Analyzer: every price signal is a column you can sort, and Hop gain tells you whether the trip to another world pays (Labs)",
        blurb: "Turn on \"Recipe Analyzer: price signals as columns\" under Settings › Labs. The Columns picker gains a column for every cost and revenue signal, each with a \"use\" pill that makes it the formula's input, plus Hop gain / unit (what buying at home would cost minus buying across the buy scope) and Worlds to visit (which worlds hold the cheapest ingredients). Rows with an ingredient that has no listing and no vendor now say how many are unpriced. For everyone, with sub-crafts on, an unlisted intermediate that can be crafted is now costed as a craft instead of as free, so Cost / unit rises on those rows.",
        link: Some("/settings"),
    },
    ChangelogEntry {
        date: "2026-09-02",
        title: "Turning ads back on works again",
        blurb: "Switching \"hide ads\" off in Settings had no effect: the setting was never actually cleared, so ads stayed hidden and the toggle flipped itself back on after a reload. Turning the setting off now sticks, and ads come back.",
        link: Some("/settings"),
    },
    ChangelogEntry {
        date: "2026-09-02",
        title: "Recipe Analyzer: try the profit formula as a control (Labs)",
        blurb: "Turn on \"Recipe Analyzer: profit formula strip\" under Settings › Labs and the formula behind every row becomes a control above the table: pick the revenue signal and the cost signal, and the columns that feed Profit are marked so you can see exactly what the number is made of. Hover a profit for the row's arithmetic. While it's on, absurd ROIs from one fake listing are capped at 100,000%.",
        link: Some("/settings"),
    },
    ChangelogEntry {
        date: "2026-09-02",
        title: "Pages that failed to load are fixed",
        blurb: "Some pages would occasionally come back blank or half-rendered because a link on them could crash the page while it was being built. Links no longer depend on the piece that was failing, so those pages load every time.",
        link: None,
    },
    ChangelogEntry {
        date: "2026-09-02",
        title: "Shared item links are quieter and stay useful",
        blurb: "Item links shared to Discord now show a clean Ultros card that points back to the live market page. The old card packed in a price-history graph that Discord could cache long after its numbers stopped being current.",
        link: Some("/items"),
    },
    ChangelogEntry {
        date: "2026-09-01",
        title: "Recipe Analyzer now shows every matching recipe",
        blurb: "The Recipe Analyzer no longer stops after its first 100 results. Scroll through the full ranked list without loading thousands of table rows into the page at once, and keep using the same sorts and filters across the whole result set.",
        link: Some("/recipe-analyzer"),
    },
    ChangelogEntry {
        date: "2026-08-31",
        title: "Browser alerts now open the page they are about",
        blurb: "Click an item-price alert to open that item's market on the world that triggered it. Retainer undercut alerts now open the Undercuts page and jump straight to the affected retainer.",
        link: Some("/alerts"),
    },
    ChangelogEntry {
        date: "2026-08-31",
        title: "Analyzer explanations stay out of your way",
        blurb: "Recipe, venture, scrip, leve, Free Company crafting, and vendor resale tools no longer put a large calculation explainer above their results. The formulas and assumptions are still available whenever you need them - open the info icon beside the tool name.",
        link: None,
    },
    ChangelogEntry {
        date: "2026-08-31",
        title: "Sales at a Glance is gone from the item page",
        blurb: "The item page's sale history no longer carries the Sales at a Glance panel - the 24 hour and 30 day stat tiles, and the projected next sale price that was extrapolated from them. The projection in particular read as a prediction while being a straight line fit through a handful of recent sales, which is not something to trade on. The sale history table itself is unchanged and now has the full panel to itself; the same numbers, computed over a real sample, live on the analyzer pages.",
        link: Some("/items"),
    },
    ChangelogEntry {
        date: "2026-08-31",
        title: "Recipe Analyzer: see and filter by where the cheapest listing is",
        blurb: "Two more optional columns show which world and datacenter currently hold the cheapest listing of the item you'd craft. Clicking either one filters the table down to that world or datacenter, the same way the flip finder does, and the filter shows as a chip you can remove.",
        link: Some("/recipe-analyzer"),
    },
    ChangelogEntry {
        date: "2026-08-31",
        title: "Recipe Analyzer: pick your columns",
        blurb: "The analyzer gets the flip finder's Columns picker. Alongside the existing table you can now show a confidence chip (on by default), when the item last sold, how many units moved in the last 7 days, the volume-weighted average price with how far the current price sits from it, and the 5% market board tax. Every new column sorts, and your choice is kept in the page's address so you can bookmark it.",
        link: Some("/recipe-analyzer"),
    },
    ChangelogEntry {
        date: "2026-08-31",
        title: "Recipe Analyzer: a Market menu for where you buy and sell",
        blurb: "The pricing controls had ended up hidden behind the + Filter menu. They now have their own Market button in the toolbar, reworked around a buy/sell split: choose whether ingredients are priced from your world, your datacenter, or the whole region, pick the cost basis, and revenue is always your sell world's price. Buying now defaults to your datacenter rather than the whole region, so profit estimates no longer assume you fly across the region for every ingredient.",
        link: Some("/recipe-analyzer"),
    },
    ChangelogEntry {
        date: "2026-08-31",
        title: "Claiming a Viera or Hrothgar character works again",
        blurb: "Claiming a character read its Lodestone profile through a parser that only knew the races the game shipped with in 2013, so claiming any Viera or Hrothgar failed outright with a server error. Claims now read just the name and home world off the profile, so every race works - and a character id the Lodestone doesn't know says so instead of erroring.",
        link: Some("/profile"),
    },
    ChangelogEntry {
        date: "2026-08-31",
        title: "The sidebar says which world is which",
        blurb: "The home-world and price-zone rows in the sidebar looked identical whenever the price zone was set to a world. Each row now carries a small label saying what it is, plus its own icon - a house for your home world, a price tag for where prices are read from. The Discord and GitHub links moved back to the very bottom, below the controls.",
        link: None,
    },
    ChangelogEntry {
        date: "2026-08-30",
        title: "Item page layout fixes",
        blurb: "Listings and sale history now sit side by side only on displays wide enough to show both tables in full (roughly 1440p and up) - everywhere else they stack, so neither table needs a horizontal scrollbar. The sale history's Show More button no longer scrolls out of view with the table, the Sales at a Glance tiles size themselves to the space they actually have instead of overlapping, the Crafting Recipes panel hugs its content instead of trailing empty space, and a sliver of page-wide horizontal scroll caused by the ad rail is gone.",
        link: None,
    },
    ChangelogEntry {
        date: "2026-08-30",
        title: "List filters actually exclude datacenters now",
        blurb: "Excluding a datacenter on a list only used to affect the purchasing view - the item table's prices and sorting quietly ignored it. It now filters everything, same as excluding a world. The filters also moved into one tidy row (exclusions, sort, hide-acquired), and Auto-mark Purchases left its box at the top of the page for a toolbar button that opens a dialog. Very tall dialogs (like list settings on a short window) now scroll instead of hiding their bottom buttons off-screen.",
        link: Some("/list"),
    },
    ChangelogEntry {
        date: "2026-08-30",
        title: "Recipe Analyzer: profit math overhaul",
        blurb: "Multi-yield recipes are now costed per unit, NPC vendor prices floor ingredient costs, the 5% market board tax is deducted from profit, and revenue now defaults to your selected world's price instead of the region-wide minimum.",
        link: Some("/recipe-analyzer"),
    },
    ChangelogEntry {
        date: "2026-08-30",
        title: "Change your home world from the sidebar",
        blurb: "Your home world now sits at the bottom of the sidebar, right above the price zone, and can be changed from any page: click it and pick a world from the same region \u{2192} datacenter \u{2192} world menu the price zone uses. Before, changing it meant a trip to the settings page.",
        link: None,
    },
    ChangelogEntry {
        date: "2026-08-30",
        title: "Discord bot, help, and changelog moved down the sidebar",
        blurb: "The three informational links no longer sit in the tool list \u{2014} they now live in their own group at the bottom of the sidebar, just above the Discord and GitHub icons. When the sidebar is collapsed to icons, expand it to reach them. Retainers also got its own icon instead of sharing one with Groups.",
        link: None,
    },
    ChangelogEntry {
        date: "2026-08-30",
        title: "Every page uses the same width and margins",
        blurb: "Pages that had their own container width - settings, history, retainers, the item explorer, trends, lists, and the legal pages - now share the same full-width shell as the rest of the app, with margins that tighten on phones so tables get the screen back.",
        link: None,
    },
    ChangelogEntry {
        date: "2026-08-30",
        title: "Market Trends stops repeating Market Movers",
        blurb: "The Market Movers strip at the top of Market Trends duplicated what the table below it already shows, so it is gone. It is still on the home page.",
        link: Some("/trends"),
    },
    ChangelogEntry {
        date: "2026-08-29",
        title: "Item pages put the market first",
        blurb: "Active listings and recent sales now sit right under the price summary \u{2014} side by side on wide screens \u{2014} with the chart below them. Item stats moved into a collapsible Item details section, and the price summary got more compact, so the tables you came for are on screen sooner.",
        link: None,
    },
    ChangelogEntry {
        date: "2026-08-29",
        title: "Slimmer tool headers",
        blurb: "Every analyzer's big title card is now a single compact row: the tool name, an info icon, and the world picker sit together, so the results start higher on the page. The tool description that used to be behind the About this tool button now lives behind the info icon.",
        link: None,
    },
    ChangelogEntry {
        date: "2026-08-29",
        title: "A few more loading spinners became skeletons",
        blurb: "The last few pulsing-text loading states in the crafting/gathering analyzers, and the item page's sale-history chart, now show skeletons shaped like the content they load into instead of a spinner or a plain pulsing line.",
        link: None,
    },
    ChangelogEntry {
        date: "2026-08-29",
        title: "Flip Finder: clearer sales rate, plus a tax column",
        blurb: "The Velocity column is gone — the Sales/day badge shows the same rate more readably and is now on by default. A new optional, sortable Tax column shows the gil the 5% market board tax takes off each estimated sale; profit stays post-tax as before.",
        link: Some("/flip-finder"),
    },
    ChangelogEntry {
        date: "2026-08-29",
        title: "Lists gets a visual refresh",
        blurb: "The Lists and list-detail pages now have a proper heading and an about-this-page panel, and the item table uses the same styled table as the rest of the app instead of unstyled HTML. The action toolbar switched to the same sticky bar the other tools use. Loading spinners were replaced with skeletons shaped like the cards and tables they load into.",
        link: Some("/list"),
    },
    ChangelogEntry {
        date: "2026-08-29",
        title: "Alerts gets a visual refresh, one add-alert dialog",
        blurb: "The Alerts page now has a proper heading and an about-this-tool panel, and its rules table uses the same styled table as the rest of the app instead of unstyled HTML. Loading spinners were replaced with skeletons shaped like the panels they load into. The two near-identical \"add alert\" dialogs used across the app are now one dialog.",
        link: Some("/alerts"),
    },
    ChangelogEntry {
        date: "2026-08-29",
        title: "Retainers gets a visual refresh",
        blurb: "The Retainers pages now have a proper heading and an about-this-tool panel, and their listing tables use the same styled table as the rest of the app instead of unstyled HTML. Loading spinners were replaced with skeletons shaped like the tables and cards they load into.",
        link: Some("/retainers"),
    },
    ChangelogEntry {
        date: "2026-08-29",
        title: "Scrip sources, trends, and vendor resale get filter chips too",
        blurb: "The last three tools still on the old filter bar now use the same sticky filter bar as the others: only the filters you've set take up space, each shown as an editable chip, with a Clear all button. Vendor resale's filters also stopped jumping the page to the top on every keystroke.",
        link: None,
    },
    ChangelogEntry {
        date: "2026-08-29",
        title: "Venture, leve, recipe, and FC crafting analyzers get filter chips",
        blurb: "The four crafting/gathering analyzers now use the same sticky filter bar as Flip Finder: only the filters you've set take up space, each shown as an editable chip, with a Clear all button. Every filter still works from the same bookmark links as before.",
        link: None,
    },
    ChangelogEntry {
        date: "2026-08-29",
        title: "You can now leave a group you've joined",
        blurb: "Members can leave a group from its member list instead of asking the owner to remove them. The owner still manages the group itself.",
        link: Some("/groups"),
    },
    ChangelogEntry {
        date: "2026-08-28",
        title: "Sale history opens on the part you care about",
        blurb: "An item that has sold in the last week now opens its price chart on the week view instead of years of history. Quiet items still open on full history, and the All button pins it whenever you want the long view.",
        link: None,
    },
    ChangelogEntry {
        date: "2026-08-28",
        title: "Sort by any column that has a number",
        blurb: "Vendor resale now sorts by vendor price, market price, and average sale time, and Flip Finder by buy price, price drift, and last sold. Cost and time columns start best-first: cheapest buy-in, most recent sale.",
        link: Some("/vendor-resale"),
    },
    ChangelogEntry {
        date: "2026-08-28",
        title: "Sort and filter chips no longer break the page",
        blurb: "On a slow response, a sort or filter chip could take the whole page down with it while it was still loading. The chips now fall back to a plain link instead of failing the render.",
        link: None,
    },
    ChangelogEntry {
        date: "2026-08-23",
        title: "Gear sets without the Ornate outliers",
        blurb: "Ornate crafted pieces cost many times more than the regular version of the same gear, so they no longer get averaged into a job's gear set. They are still listed, just on their own.",
        link: Some("/items"),
    },
    ChangelogEntry {
        date: "2026-08-15",
        title: "Currency Exchange, rebuilt",
        blurb: "A denser, spreadsheet-style table with collapsible filters, and every reward now shows its real item icon.",
        link: Some("/currency-exchange"),
    },
    ChangelogEntry {
        date: "2026-08-06",
        title: "Check a flip before you commit",
        blurb: "Flip Finder rows now open a comparison view on the item page: the cheapest listing where you would buy, the expected sale price where you would sell, and the profit per unit and per stack after tax.",
        link: Some("/flip-finder"),
    },
    ChangelogEntry {
        date: "2026-08-06",
        title: "Charts that fit their scope",
        blurb: "A datacenter or region chart now groups lines to match instead of drawing every world. Quick range buttons, clearer time labels, and the chart's settings live in the address bar so a pasted link shows the same view. The crosshair works by touch, too.",
        link: None,
    },
    ChangelogEntry {
        date: "2026-08-06",
        title: "Groups you can actually join",
        blurb: "Create a group straight from a Discord server you manage, or hand out a shareable invite link the way lists already do.",
        link: Some("/groups"),
    },
    ChangelogEntry {
        date: "2026-08-06",
        title: "Flip Finder keeps up",
        blurb: "The table updates in place as live market data arrives, profit per day no longer flattens fast sellers, and opening the page with no filters lands on a sensible default view.",
        link: Some("/flip-finder"),
    },
    ChangelogEntry {
        date: "2026-08-05",
        title: "Vendor resale drops the fantasy prices",
        blurb: "Listings priced far above what an item actually sells for are hidden from vendor resale, and the sort headers no longer get stuck.",
        link: Some("/vendor-resale"),
    },
    ChangelogEntry {
        date: "2026-08-05",
        title: "A better fit on phones",
        blurb: "Pages run edge to edge on small screens, popovers close on tap-away, Escape or navigation, and the search sheet dismisses with a tap.",
        link: None,
    },
    ChangelogEntry {
        date: "2026-08-03",
        title: "Trends ranks what actually sells",
        blurb: "Market Trends now picks its candidates by recent sale volume across the whole market, so the movers you see are the real ones.",
        link: Some("/trends"),
    },
    ChangelogEntry {
        date: "2026-08-02",
        title: "Claim characters without the Lodestone dance",
        blurb: "Claiming a character no longer requires pasting a code into your Lodestone bio — your Discord sign-in is enough. The sidebar can then swap your home world to match whichever character you are playing.",
        link: Some("/settings"),
    },
    ChangelogEntry {
        date: "2026-08-02",
        title: "All your alerts in one drawer",
        blurb: "One place to add price and undercut alerts, and the drawer shows what is already active so you are not guessing.",
        link: Some("/alerts"),
    },
    ChangelogEntry {
        date: "2026-08-02",
        title: "Loading looks like the page",
        blurb: "Tables sketch their real columns with a shimmer while data loads, instead of a blank panel that jumps when content arrives.",
        link: None,
    },
    ChangelogEntry {
        date: "2026-08-01",
        title: "One sidebar instead of a top bar",
        blurb: "Navigation, search, your account and the language picker all moved into a single sidebar, so tools get the full width of the window. The sidebar collapses to icons when you want the space back.",
        link: None,
    },
    ChangelogEntry {
        date: "2026-08-01",
        title: "Patch markers on sale history",
        blurb: "Price charts now shade each game patch and show a calendar of past updates, so you can see which patch moved an item's price instead of guessing at the date.",
        link: None,
    },
    ChangelogEntry {
        date: "2026-07-31",
        title: "Compare worlds side by side",
        blurb: "Sale history can be drawn as a grid of small charts, one per world, sharing a crosshair — line up the same moment across your whole data center.",
        link: None,
    },
    ChangelogEntry {
        date: "2026-07-31",
        title: "Candles, ranges and density on price charts",
        blurb: "A toolbar above the chart switches how sales are drawn. Candles and ranges show how far prices spread within a day, not just the average.",
        link: None,
    },
    ChangelogEntry {
        date: "2026-07-31",
        title: "Pick your own colors",
        blurb: "Setup now offers a palette step, and you can change the color scheme any time from settings.",
        link: Some("/settings"),
    },
    ChangelogEntry {
        date: "2026-07-31",
        title: "Share a list exactly as you left it",
        blurb: "Sorting and filters on a list are kept in the address bar, so a link you paste shows what you were looking at. Deleting a list now asks first.",
        link: Some("/list"),
    },
    ChangelogEntry {
        date: "2026-07-31",
        title: "Flip Finder is easier to read",
        blurb: "Empty states that explain what to do next, a history of filters you have used, tooltips on every column, and a mobile layout that actually fits.",
        link: Some("/flip-finder"),
    },
    ChangelogEntry {
        date: "2026-07-31",
        title: "Screenshots in the help guides",
        blurb: "Help topics now show the screen they are describing.",
        link: Some("/help"),
    },
    ChangelogEntry {
        date: "2026-07-31",
        title: "A real privacy policy",
        blurb: "The privacy and cookie pages now spell out what Ultros stores about you and why.",
        link: Some("/privacy"),
    },
    ChangelogEntry {
        date: "2026-07-30",
        title: "Filter on any Flip Finder column",
        blurb: "Profit, ROI, sale speed, volume, category, name — every column in the table can be filtered, not just a fixed handful.",
        link: Some("/flip-finder"),
    },
    ChangelogEntry {
        date: "2026-07-30",
        title: "Scrip Sources shows results again",
        blurb: "The page was reading the wrong field and came up empty for most scrip types. It lists collectables properly now.",
        link: Some("/scrip-sources"),
    },
    ChangelogEntry {
        date: "2026-07-29",
        title: "Item pages load fast on busy items",
        blurb: "Sales are summarized on the server and the chart only draws the detail you are zoomed into, so items with years of history open quickly.",
        link: None,
    },
    ChangelogEntry {
        date: "2026-07-29",
        title: "Saved views in Flip Finder",
        blurb: "Save a set of filters you keep coming back to and reopen it in one click.",
        link: Some("/flip-finder"),
    },
    ChangelogEntry {
        date: "2026-07-29",
        title: "A rebuilt item page",
        blurb: "Listings live in one merged table, sections are ordered the way you actually read them, and a jump menu gets you to the part you want.",
        link: None,
    },
    ChangelogEntry {
        date: "2026-07-29",
        title: "Search hints at job sets",
        blurb: "The search bar sits front and center, and searching a job abbreviation takes you to that job's gear.",
        link: Some("/items"),
    },
    ChangelogEntry {
        date: "2026-07-25",
        title: "Browse gear by job set",
        blurb: "Item Explorer groups equipment into gear sets with their own detail pages, adds a world picker, and tucks subcategories into grouped menus.",
        link: Some("/items"),
    },
    ChangelogEntry {
        date: "2026-06-28",
        title: "Attach characters to your retainers",
        blurb: "Verify a character and assign the retainers it owns, so undercut checks know which listings are yours.",
        link: Some("/retainers/listings"),
    },
    ChangelogEntry {
        date: "2026-06-26",
        title: "Live market updates",
        blurb: "Item and list pages show whether live data is connected and refresh as new sales and listings arrive.",
        link: None,
    },
    ChangelogEntry {
        date: "2026-06-26",
        title: "See who shared a list with you",
        blurb: "Shared lists show their owner on the card, so a long list of lists stays readable.",
        link: Some("/list"),
    },
    ChangelogEntry {
        date: "2026-06-10",
        title: "A faster, sharper price chart",
        blurb: "Price history and sparklines are drawn by our own chart engine. Smaller download, smoother zooming, and a crosshair that reads the sale under your cursor.",
        link: None,
    },
    ChangelogEntry {
        date: "2026-06-08",
        title: "Real Price on the item page",
        blurb: "A price estimate that ignores gil-transfer trades between friends, so a single fake 99,999,999 sale stops skewing what an item is worth.",
        link: None,
    },
    ChangelogEntry {
        date: "2026-05-19",
        title: "Market Trends",
        blurb: "See what is selling fastest on your world, and what is climbing or sliding in price.",
        link: Some("/trends"),
    },
    ChangelogEntry {
        date: "2026-05-16",
        title: "A home page worth landing on",
        blurb: "The front page is a dashboard of live market activity for your world instead of a static welcome.",
        link: Some("/"),
    },
    ChangelogEntry {
        date: "2026-05-12",
        title: "Ultros speaks seven languages",
        blurb: "French, German, Japanese, Korean, and Simplified and Traditional Chinese alongside English. Item names follow the language you pick.",
        link: Some("/settings"),
    },
    ChangelogEntry {
        date: "2026-05-12",
        title: "Price alerts, on Discord or in your browser",
        blurb: "Get pinged when an item crosses the price you set, or when someone undercuts your retainer.",
        link: Some("/alerts"),
    },
    ChangelogEntry {
        date: "2026-05-11",
        title: "Shared lists",
        blurb: "Invite people to a shopping list and choose who can read it and who can change it.",
        link: Some("/list"),
    },
    ChangelogEntry {
        date: "2026-05-11",
        title: "A shorter first run",
        blurb: "New here? A quick setup asks for your home world so prices are relevant from the first page you open.",
        link: Some("/welcome"),
    },
];

/// Date of the newest entry, as an ISO-8601 `YYYY-MM-DD` string. Empty when
/// there are no entries.
pub fn latest_changelog_date() -> &'static str {
    CHANGELOG.first().map(|entry| entry.date).unwrap_or("")
}

/// Groups the changelog into one bucket per day, newest day first, preserving
/// the order entries were written in within a day.
///
/// Several changes usually ship on the same day, and giving each of them its
/// own dated card makes the page read as a wall of repeated dates. Grouping
/// relies on [`CHANGELOG`] being sorted newest-first (guarded by
/// `entries_are_sorted_newest_first`), so same-day entries are always
/// adjacent.
pub fn changelog_by_day() -> Vec<(&'static str, Vec<&'static ChangelogEntry>)> {
    let mut days: Vec<(&'static str, Vec<&'static ChangelogEntry>)> = Vec::new();
    for entry in CHANGELOG {
        match days.last_mut() {
            Some((date, entries)) if *date == entry.date => entries.push(entry),
            _ => days.push((entry.date, vec![entry])),
        }
    }
    days
}

#[component]
pub fn Changelog() -> impl IntoView {
    let i18n = use_i18n();
    // Visiting the page is what clears the sidebar dot.
    use_mark_changelog_seen();

    view! {
        <MetaTitle title=t_string!(i18n, changelog_meta_title).to_string() />
        <MetaDescription text=t_string!(i18n, changelog_meta_desc).to_string() />
        <div class="main-content p-2 sm:p-6">
            <div class="container mx-auto max-w-4xl flex flex-col gap-6">
                <section class="panel p-6 sm:p-8 rounded-2xl">
                    <h1 class="text-3xl font-bold text-[color:var(--brand-fg)] mb-3">
                        {t!(i18n, changelog_page_heading)}
                    </h1>
                    <p class="text-lg text-[color:var(--color-text)] max-w-2xl">
                        {t!(i18n, changelog_intro)}
                    </p>
                </section>
                <ol class="flex flex-col gap-6">
                    {changelog_by_day().into_iter().map(|(date, entries)| view! {
                        <li class="flex flex-col gap-3">
                            <time
                                datetime=date
                                class="text-xs font-bold uppercase tracking-wide text-brand-300 tabular-nums"
                            >
                                {date}
                            </time>
                            <ul class="panel rounded-xl divide-y divide-[color:var(--color-outline)]">
                                {entries.into_iter().map(|entry| view! {
                                    <li class="p-5 flex flex-col gap-2">
                                        <h2 class="text-xl font-bold text-[color:var(--brand-fg)]">{entry.title}</h2>
                                        <p class="text-sm text-[color:var(--color-text-muted)]">{entry.blurb}</p>
                                        {entry.link.map(|href| view! {
                                            <AppLink
                                                href=href
                                                attr:class="text-sm text-brand-300 hover:text-[color:var(--brand-fg)] inline-flex items-center gap-1.5 self-start"
                                            >
                                                {t!(i18n, changelog_try_it)}
                                                <Icon icon=i::FaArrowRightSolid width="0.8em" height="0.8em" />
                                            </AppLink>
                                        })}
                                    </li>
                                }).collect_view()}
                            </ul>
                        </li>
                    }).collect_view()}
                </ol>
            </div>
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The what's-new dot reads `CHANGELOG[0]` as "the newest thing we
    /// shipped". Appending to the bottom instead of the top would silently
    /// stop the dot from ever appearing again.
    #[test]
    fn entries_are_sorted_newest_first() {
        for pair in CHANGELOG.windows(2) {
            assert!(
                pair[0].date >= pair[1].date,
                "changelog is out of order: {} ({}) precedes {} ({})",
                pair[0].title,
                pair[0].date,
                pair[1].title,
                pair[1].date,
            );
        }
    }

    /// Dates are compared as strings against the last-seen cookie, which is
    /// only chronological while every date is zero-padded `YYYY-MM-DD`.
    #[test]
    fn dates_are_iso_8601() {
        for entry in CHANGELOG {
            let date = entry.date;
            assert_eq!(date.len(), 10, "not YYYY-MM-DD: {date}");
            assert!(
                date.chars().enumerate().all(|(i, c)| match i {
                    4 | 7 => c == '-',
                    _ => c.is_ascii_digit(),
                }),
                "not YYYY-MM-DD: {date}"
            );
        }
    }

    #[test]
    fn links_are_internal_routes() {
        for entry in CHANGELOG {
            if let Some(link) = entry.link {
                assert!(
                    link.starts_with('/'),
                    "{} links off-site ({link}); the router only handles app routes",
                    entry.title
                );
            }
        }
    }

    #[test]
    fn latest_date_is_the_first_entry() {
        assert_eq!(latest_changelog_date(), CHANGELOG[0].date);
    }

    /// One bucket per day, every entry kept, order preserved.
    #[test]
    fn grouping_by_day_is_lossless_and_ordered() {
        let days = changelog_by_day();
        let flattened: Vec<_> = days
            .iter()
            .flat_map(|(_, entries)| entries.iter().copied())
            .collect();
        assert_eq!(flattened.len(), CHANGELOG.len());
        for (grouped, original) in flattened.iter().zip(CHANGELOG) {
            assert!(**grouped == *original, "{} is out of place", original.title);
        }
        for pair in days.windows(2) {
            assert!(
                pair[0].0 > pair[1].0,
                "days are not strictly newest-first: {} then {}",
                pair[0].0,
                pair[1].0
            );
        }
        for (date, entries) in &days {
            assert!(!entries.is_empty());
            assert!(entries.iter().all(|entry| entry.date == *date));
        }
    }
}
