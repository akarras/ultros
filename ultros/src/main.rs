#![feature(trivial_bounds)]
#![recursion_limit = "256"]
pub(crate) mod alerts;
pub(crate) mod analyzer_service;
pub(crate) mod character_claim;
mod discord;
pub(crate) mod event;
mod ingest_health;
mod item_update_service;
pub mod leptos;
#[cfg(feature = "profiling")]
pub mod profiling;
pub(crate) mod resale_eligibility;
pub(crate) mod search_service;
pub(crate) mod trend_candidates;
pub(crate) mod utils;
mod web;
mod web_metrics;

use crate::item_update_service::UpdateService;
#[cfg(feature = "profiling")]
use crate::profiling::start_profiling_server;
use crate::search_service::SearchService;
use crate::web::WebState;
use ::leptos::config::get_configuration;
use analyzer_service::AnalyzerService;
use anyhow::Result;
use axum_extra::extract::cookie::Key;
use character_claim::CharacterClaimService;
use discord::start_discord;
use dotenvy::dotenv;
use event::{EventProducer, EventType, create_event_busses};
use std::collections::HashSet;
use std::sync::Arc;
#[cfg(all(not(target_env = "msvc"), feature = "jemalloc"))]
use tikv_jemallocator::Jemalloc;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use ultros_api_types::websocket::{ListingEventData, SaleEventData};
use ultros_api_types::world::WorldData;
use ultros_api_types::world_helper::WorldHelper;
use ultros_db::UltrosDb;
use ultros_db::world_data::world_cache::WorldCache;
use universalis::websocket::SocketRx;
use universalis::websocket::event_types::{EventChannel, SubscribeMode, WSMessage};
use universalis::{DataCentersView, UniversalisClient, WebsocketClient, WorldId, WorldsView};
use web::oauth::{AuthUserCache, DiscordAuthConfig, OAuthScope};
#[cfg(all(not(target_env = "msvc"), feature = "jemalloc"))]
#[global_allocator]
static GLOBAL: Jemalloc = Jemalloc;
#[cfg(feature = "profiling")]
#[allow(non_upper_case_globals)]
#[export_name = "malloc_conf"]
pub static malloc_conf: &[u8] = b"prof:true,prof_active:true,lg_prof_sample:19\0";

/// User-Agent sent on every Universalis request (REST + websocket), per their
/// guidance that scripted consumers identify themselves with version + contact.
pub(crate) const UNIVERSALIS_USER_AGENT: &str = concat!(
    "ultros/",
    env!("CARGO_PKG_VERSION"),
    " (+https://ultros.app)"
);

#[derive(Debug, serde::Deserialize, Clone)]
struct Config {
    hostname: String,
    discord_client_id: String,
    discord_client_secret: String,
    key: String,
    discord_token: String,
}

async fn run_socket_listener(
    db: UltrosDb,
    listings_tx: EventProducer<ListingEventData>,
    sales_tx: EventProducer<SaleEventData>,
    token: CancellationToken,
) {
    let mut socket = WebsocketClient::connect(UNIVERSALIS_USER_AGENT).await;
    socket
        .update_subscription(SubscribeMode::Subscribe, EventChannel::ListingsAdd, None)
        .await;
    socket
        .update_subscription(SubscribeMode::Subscribe, EventChannel::ListingsRemove, None)
        .await;
    socket
        .update_subscription(SubscribeMode::Subscribe, EventChannel::SalesAdd, None)
        .await;
    let receiver = socket.get_receiver();
    loop {
        tokio::select! {
            _ = token.cancelled() => {
                info!("socket listener cancelled");
                break;
            }
            msg = receiver.recv() => {
                if let Some(msg) = msg {
                    // create a new task for each message
                    let db = db.clone();
            // hopefully this is cheap to clone
            let listings_tx = listings_tx.clone();
            let sales_tx = sales_tx.clone();
            if let SocketRx::Event(Ok(e)) = &msg {
                let world_id = WorldId::from(e);
                metrics::counter!("ultros_websocket_rx", "WorldId" => world_id.0.to_string())
                    .increment(1);
            }
            tokio::spawn(async move {
                let db = &db;
                match msg {
                    SocketRx::Event(Ok(WSMessage::ListingsAdd {
                        item,
                        world,
                        listings,
                    // `listings/add` is a DELTA — only the listings that newly
                    // appeared, median one per event — so it must be applied
                    // insert-only. It used to go through `update_listings`, which
                    // deletes every row absent from its input and so truncated the
                    // world's board down to the delta on every event. Removals
                    // arrive on `listings/remove` below.
                    })) => match db.add_listings(listings.clone(), item, world).await {
                        Ok(added) => {
                            let added = Arc::new(ListingEventData {
                                item_id: item.0,
                                world_id: world.0,
                                listings: added,
                            });
                            match listings_tx.send(EventType::Add(added)) {
                                Ok(o) => info!(remaining_slack = o, "added listings"),
                                Err(e) => error!(error = ?e, "Error adding listings"),
                            };
                        }
                        Err(e) => error!(error = ?e, listings = ?listings, "Listing add failed"),
                    },
                    SocketRx::Event(Ok(WSMessage::ListingsRemove {
                        item,
                        world,
                        listings,
                    })) => match db.remove_listings(listings.clone(), item, world).await {
                        Ok(listings) => {
                            info!(?listings, ?item, ?world, "Removed listings");
                            if let Err(e) = listings_tx.send(EventType::removed(ListingEventData {
                                item_id: item.0,
                                world_id: world.0,
                                listings,
                            })) {
                                error!(error = ?e, "Error sending remove listings");
                            }
                        }
                        Err(e) => {
                            error!(error = ?e, ?listings, ?item, ?world, "Error removing listings. Listings set")
                        }
                    },
                    SocketRx::Event(Ok(WSMessage::SalesAdd { item, world, sales })) => {
                        match db.update_sales(sales.clone(), item, world).await {
                            Ok(added_sales) => {
                                info!(?added_sales, ?item, ?world, "Stored sale data");
                                match sales_tx
                                    .send(EventType::added(SaleEventData { sales: added_sales }))
                                {
                                    Ok(o) => info!(slack_remaining = o, "Sent sale"),
                                    Err(e) => error!(error = ?e, "Error sending sale update"),
                                }
                            }
                            Err(e) => {
                                error!(error = ?e, ?sales, ?item, ?world, "Error inserting sale.")
                            }
                        }
                    }
                    SocketRx::Event(Ok(WSMessage::SalesRemove { item, world, sales })) => {
                        info!(?item, ?world, ?sales, "sales removed");
                    }
                    SocketRx::Event(Err(e)) => {
                        error!(error = ?e, "Error");
                    }
                }
            });
                }
            }
        }
    }
}

async fn init_db(
    db: &UltrosDb,
    worlds_view: Result<WorldsView, universalis::Error>,
    datacenters: Result<DataCentersView, universalis::Error>,
) -> Result<()> {
    info!("db starting");

    db.insert_default_retainer_cities().await.unwrap();
    info!("DB connected & ffxiv world data primed");
    {
        if let (Ok(worlds), Ok(datacenters)) = (worlds_view, datacenters) {
            db.update_datacenters(&datacenters, &worlds).await?;
        }
    }
    Ok(())
}

/// Whether Glitchtip/Sentry error reporting should be suppressed for this
/// process based on the configured environment.
///
/// We deliberately *never* ship events from a `development` environment to the
/// shared production Glitchtip: a local dev box that has `GLITCHTIP_DSN` set
/// would otherwise pollute prod with cold-start noise (see the init site in
/// `main`). This is an allow-list-shaped check — only the exact `development`
/// value is suppressed, so production (`production`) and any unset/other value
/// still report, which fails safe if `GLITCHTIP_ENVIRONMENT` is ever
/// misconfigured on the real deployment.
fn error_reporting_disabled(environment: Option<&str>) -> bool {
    environment == Some("development")
}

/// Format a panic's `#[track_caller]` source location as `file:line:column`.
///
/// Mirrors the wasm-side reporter in `ultros-client/src/lib.rs`, so a server
/// panic and a client panic read identically in Glitchtip.
fn panic_location_string(info: &std::panic::PanicHookInfo<'_>) -> Option<String> {
    info.location()
        .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
}

/// Attach the panicking source location to every Sentry panic event.
///
/// `sentry-panic` 0.49 builds its event purely from the panic *payload* and a
/// runtime backtrace — it never reads `PanicHookInfo::location()`
/// (`sentry_panic::PanicIntegration::event_from_panic_info`). Our release
/// binary ships without symbols, so every frame Glitchtip receives is an
/// `<unknown>` at a bare instruction address, and the addresses are ASLR'd,
/// so they differ between events for the *same* panic. The result is what the
/// server backlog actually looks like today: `culprit: <unknown>`, no tags,
/// and a single un-actionable issue per panic *message* — 34k events under
/// "called `Option::unwrap()` on a `None` value" (Glitchtip #6876) with no way
/// to tell which of the ~hundreds of `unwrap()`s in the tree produced them.
///
/// `location()` needs no symbols at all: `file:line:column` is baked in at
/// compile time by `#[track_caller]` and is already sitting on the
/// `PanicHookInfo` we are handed. Wrapping the hook that `sentry::init`
/// installed (rather than adding a second `PanicIntegration`, which the
/// upstream docs say is unsupported — extractors are ignored when the
/// integration is registered twice) records it on:
///
/// * tag `panic.location` — searchable/filterable in Glitchtip, and
/// * context `rust_panic.location` — the exact shape the wasm reporter uses.
///
/// It also fingerprints by location, so panics split into one issue per
/// panic *site* instead of per message. This is the same trick the client-side
/// reporter uses ("a stable per-location fingerprint, so it collapses to one
/// issue per panic site" — `ultros-app/src/error_filter.js`). Expect existing
/// server panic issues to go quiet and be replaced by per-site ones on deploy.
///
/// Must be called *after* `sentry::init`, since that is what installs the hook
/// being wrapped.
fn attach_panic_location_to_sentry() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let Some(location) = panic_location_string(info) else {
            previous(info);
            return;
        };
        sentry::with_scope(
            |scope| {
                scope.set_tag("panic.location", &location);
                scope.set_context(
                    "rust_panic",
                    sentry::protocol::Context::Other(
                        std::iter::once(("location".to_string(), location.clone().into()))
                            .collect(),
                    ),
                );
                scope.set_fingerprint(Some(["panic", location.as_str()].as_slice()));
            },
            || previous(info),
        );
    }));
}

// Bolt: Switched to multi-threaded runtime for better performance on multi-core systems
#[tokio::main]
async fn main() -> Result<()> {
    // Load environment variables from `.env` file, if present
    dotenv().ok();

    // Install a process-level rustls CryptoProvider. Multiple transitive deps
    // (serenity/poise, reqwest 0.12/0.13, sqlx, tokio-rustls, sentry) unify on
    // rustls 0.23 with BOTH `aws-lc-rs` and `ring` features active, so
    // `ClientConfig::builder()` panics on first TLS connect ("Could not
    // automatically determine the process-level CryptoProvider"). Install
    // once at startup before any TLS handshake. Ignore the result because a
    // double-install only fails if some upstream beat us to it, which is fine.
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

    // Glitchtip / Sentry error reporting. No-op when GLITCHTIP_DSN is unset, so
    // local dev runs without it. The guard must be held for the duration of
    // main() so the background transport can flush on shutdown.
    //
    // We also skip init entirely when GLITCHTIP_ENVIRONMENT=development, even if
    // GLITCHTIP_DSN is set. A local dev box that has the DSN configured (e.g.
    // copied from the prod .env) would otherwise flood the *shared production*
    // Glitchtip with cold-start noise: sqlx "Connection pool timed out" errors
    // surfaced through the sentry-tracing layer below, which showed up in prod
    // as GlitchTip #2214/#2215/#2216/#2217 (hundreds of events from `Bahamut`).
    // The comment above already documents the intent — "local dev runs without
    // it" — so enforce it by environment, not just by whether a DSN happens to
    // be present.
    //
    // GLITCHTIP_TRACES_SAMPLE_RATE controls performance/transaction sampling:
    // 0.0 disables (default — matches prior behavior), 1.0 sends every request.
    // Glitchtip 4.x and Sentry both accept transaction envelopes.
    let environment = std::env::var("GLITCHTIP_ENVIRONMENT").ok();
    let _sentry_guard = std::env::var("GLITCHTIP_DSN")
        .ok()
        .filter(|_| !error_reporting_disabled(environment.as_deref()))
        .map(|dsn| {
            // Clamped rather than passed through: 0.48 took this as a plain
            // struct field and tolerated anything, but 0.49's setter *panics*
            // outside [0.0, 1.0]. Since this is operator-supplied, the natural
            // typo for a "sample rate" — `=50`, meaning 50% — would otherwise
            // take the server down at boot, before the DB is even reachable.
            // `is_finite` first because "NaN"/"inf" parse successfully as f32
            // and would panic just the same.
            let traces_sample_rate = std::env::var("GLITCHTIP_TRACES_SAMPLE_RATE")
                .ok()
                .and_then(|v| v.parse::<f32>().ok())
                .filter(|rate| rate.is_finite())
                .map(|rate| rate.clamp(0.0, 1.0))
                .unwrap_or(0.0);
            // sentry 0.49 made `ClientOptions` `#[non_exhaustive]` and dropped
            // `traces_sample_rate` as a public field in favor of a builder
            // setter, so we can no longer use struct-literal + `..Default`.
            // The remaining fields are still public and assignable directly.
            let mut options = sentry::ClientOptions::new().traces_sample_rate(traces_sample_rate);
            options.release = sentry::release_name!();
            options.environment = environment.clone().map(Into::into);
            options.attach_stacktrace = true;
            options.send_default_pii = false;
            sentry::init((dsn, options))
        });

    // Wrap the panic hook `sentry::init` just installed so panic events carry
    // their source location. Only meaningful when reporting is on, and the
    // wrap must happen after init — see `attach_panic_location_to_sentry`.
    if _sentry_guard.is_some() {
        attach_panic_location_to_sentry();
    }

    // Create the db before we proceed
    let filter: EnvFilter =
        EnvFilter::try_from_default_env().unwrap_or("warn,ultros=info,ultros-app=info".into());
    tracing_subscriber::registry()
        .with(filter)
        .with(
            tracing_subscriber::fmt::layer()
                .with_file(true)
                .with_line_number(true)
                .pretty(),
        )
        .with(sentry_tracing::layer())
        .init();
    // Install the Prometheus recorder before any service spawns. `metrics::`
    // macros are no-ops against the default NoopRecorder, so anything emitted
    // before installation vanishes — and the analyzer records its
    // snapshot-age / snapshot-rejection samples during startup, well before
    // `start_web` (which only *serves* the handle on /metrics) ever runs.
    let prometheus_handle = web_metrics::setup_metrics_recorder();
    #[cfg(feature = "profiling")]
    tokio::spawn(async move { start_profiling_server().await });
    info!("Ultros starting!");
    info!("Connecting DB");
    let db = UltrosDb::connect().await?;
    info!("Fetching datacenters/worlds from universalis");
    let universalis_client = UniversalisClient::new(UNIVERSALIS_USER_AGENT);
    let startup_client = universalis_client.clone();
    let init = db.clone();
    let (senders, receivers) = create_event_busses();
    let listings_sender = senders.listings.clone();
    let history_sender = senders.history.clone();
    let token = CancellationToken::new();
    let socket_token = token.clone();
    tokio::spawn(async move {
        let (datacenters, worlds) = futures::future::join(
            startup_client.get_data_centers(),
            startup_client.get_worlds(),
        )
        .await;
        info!("Initializing database with worlds/datacenters");
        init_db(&init, worlds, datacenters)
            .await
            .expect("Unable to populate worlds datacenters- is universalis down?");
        info!("starting websocket");
        run_socket_listener(init, listings_sender, history_sender, socket_token).await;
    });
    // on first run, the world cache may be empty
    let world_cache = Arc::new(WorldCache::new(&db).await);
    let world_helper = Arc::new(WorldHelper::new(WorldData::from(world_cache.as_ref())));

    // ClickHouse: analytical store. Migration is idempotent — re-running it on
    // every startup is fine. We log-and-continue on failure because PG is the
    // source of truth and the analyzer's RAM caches keep the snappy tools
    // alive even if CH is unreachable. When migrate fails we wire a disabled
    // writer (silently drops rows) and skip the rollup scheduler — otherwise
    // the flush task would fire `ClickHouse flush failed` every 5s and the
    // sentry-tracing layer would report each one as a separate issue (see
    // GlitchTip #5080, ~1k events from a dev box without CH running).
    let ch_client = ultros_clickhouse::ClickHouseClient::from_env();
    let ch_writer = match ch_client.migrate().await {
        Ok(()) => {
            let writer = ultros_clickhouse::writer::Writer::spawn(ch_client.clone(), token.clone());
            // Background scheduler that keeps item_stats_window +
            // item_quality_score fresh. Runs an immediate seed pass on startup,
            // then on independent cadences (1d every 15min, 7d hourly,
            // 30d/90d every 6h, quality hourly).
            ultros_clickhouse::rollups::spawn_scheduler(ch_client.clone(), token.clone());
            writer
        }
        Err(e) => {
            warn!("ClickHouse migrate failed; continuing without analytics writes: {e:?}");
            ultros_clickhouse::writer::Writer::disabled()
        }
    };

    let analyzer_service = AnalyzerService::start_analyzer(
        db.clone(),
        receivers.clone(),
        world_cache.clone(),
        ch_writer,
        ch_client.clone(),
        token.clone(),
    )
    .await;
    let update_service = Arc::new(UpdateService {
        db: db.clone(),
        world_cache: world_cache.clone(),
        universalis: universalis_client.clone(),
        listings: senders.listings.clone(),
        sales: senders.history.clone(),
        full_sweep_cooldowns: Default::default(),
        uncovered_worlds: Default::default(),
    });
    UpdateService::start_service(update_service.clone(), token.clone());
    // Exports `ultros_world_ingest_staleness_seconds`. Every silent ingest
    // failure looks like a healthy process serving frozen numbers, so this gauge
    // is the only thing that makes one visible from outside.
    ingest_health::spawn_staleness_gauge(db.clone(), world_cache.clone(), token.clone());
    // begin listening to universalis events
    // load configuration from environment
    let config = envy::from_env::<Config>()?;
    let Config {
        hostname,
        discord_client_id,
        discord_client_secret,
        key,
        discord_token,
    } = config;

    // Web Push (VAPID) bootstrap: env vars are optional — push is feature-gated
    // at runtime. Keys must be generated offline (see docs/push.md); rotating
    // them invalidates every active browser subscription, so we explicitly never
    // generate them at startup.
    match (
        std::env::var("VAPID_PUBLIC_KEY")
            .ok()
            .filter(|s| !s.is_empty()),
        std::env::var("VAPID_PRIVATE_KEY")
            .ok()
            .filter(|s| !s.is_empty()),
        std::env::var("VAPID_CONTACT_EMAIL")
            .ok()
            .filter(|s| !s.is_empty()),
    ) {
        (Some(public_key_b64url), Some(private_key_pem), Some(contact_email)) => {
            crate::alerts::delivery::set_web_push_config(crate::alerts::delivery::WebPushConfig {
                public_key_b64url,
                private_key_pem,
                contact_email,
            });
            info!("Web Push enabled");
        }
        _ => {
            warn!(
                "Web Push disabled (set VAPID_PUBLIC_KEY, VAPID_PRIVATE_KEY, VAPID_CONTACT_EMAIL to enable)"
            );
        }
    }

    tokio::spawn(start_discord(
        db.clone(),
        senders.clone(),
        receivers.clone(),
        analyzer_service.clone(),
        world_cache.clone(),
        world_helper.clone(),
        update_service,
        discord_token,
        token.clone(),
        ch_client.clone(),
    ));

    let character_claim = CharacterClaimService {
        client: reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .unwrap(),
        db: db.clone(),
        world_cache: world_cache.clone(),
    };
    let search_service = SearchService::new()?;
    let conf = get_configuration(Some("Cargo.toml")).unwrap();
    let mut leptos_options = conf.leptos_options;
    let git_hash = env!("GIT_HASH");
    leptos_options.site_pkg_dir = Arc::from(["pkg/", git_hash].concat());
    let web_state = WebState {
        analyzer_service,
        db,
        key: Key::from(key.as_bytes()),
        character_claim,
        oauth_config: DiscordAuthConfig::new(
            discord_client_id,
            discord_client_secret,
            format!("{}/redirect", hostname.trim_end_matches('/')),
            HashSet::from_iter([OAuthScope::Identify]),
        ),
        user_cache: AuthUserCache::new(),
        event_receivers: receivers,
        event_senders: senders,
        world_cache,
        world_helper,
        leptos_options,
        search_service,
        token: token.clone(),
        ch_client,
        universalis: universalis_client,
        price_series_cache: Default::default(),
    };
    let web_task = tokio::spawn(web::start_web(web_state, prometheus_handle));
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            info!("ctrl-c received");
        }
        _ = web_task => {
            info!("web task finished");
        }
    }
    token.cancel();
    info!("Exiting");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::error_reporting_disabled;

    #[test]
    fn development_environment_suppresses_reporting() {
        // A dev box (GLITCHTIP_ENVIRONMENT=development) must never reach the
        // shared production Glitchtip, even with GLITCHTIP_DSN set. Regression
        // guard for the #2214-#2217 cold-start pool-timeout flood.
        assert!(error_reporting_disabled(Some("development")));
    }

    #[test]
    fn production_and_other_environments_still_report() {
        // Fail safe: anything that isn't exactly "development" reports, so a
        // misconfigured / unset GLITCHTIP_ENVIRONMENT on the real deploy never
        // silently drops production errors.
        assert!(!error_reporting_disabled(Some("production")));
        assert!(!error_reporting_disabled(Some("staging")));
        assert!(!error_reporting_disabled(None));
    }

    /// `panic_location_string` is what makes a server panic attributable at
    /// all: the release binary is unsymbolicated, so `file:line:column` from
    /// `#[track_caller]` is the only source information Glitchtip ever gets.
    /// The panic hook is a `Box<dyn Fn>` we cannot call directly from a test,
    /// so exercise the formatter through a real panic.
    #[test]
    fn panic_location_is_file_line_column() {
        use std::sync::{Arc, Mutex};

        let captured: Arc<Mutex<Option<String>>> = Arc::default();
        let sink = captured.clone();

        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            *sink.lock().unwrap() = super::panic_location_string(info);
        }));
        let _ = std::panic::catch_unwind(|| panic!("boom"));
        std::panic::set_hook(previous);

        let location = captured
            .lock()
            .unwrap()
            .clone()
            .expect("panics carry a location");
        // `main.rs:<line>:<col>` — the shape the wasm reporter already sends as
        // `contexts.rust_panic.location`.
        let (file, line_col) = location
            .rsplit_once(':')
            .and_then(|(rest, col)| {
                rest.rsplit_once(':')
                    .map(|(file, line)| (file, (line, col)))
            })
            .expect("location is file:line:column");
        assert!(file.ends_with("main.rs"), "unexpected file in {location}");
        assert!(
            line_col.0.parse::<u32>().is_ok(),
            "unexpected line in {location}"
        );
        assert!(
            line_col.1.parse::<u32>().is_ok(),
            "unexpected column in {location}"
        );
    }
}
