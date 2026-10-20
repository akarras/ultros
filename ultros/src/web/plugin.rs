//! Device authorization uses the website cookie only for explicit browser consent.
//! All game APIs use a separate bearer principal; no Discord token leaves the browser.
pub(crate) mod browser;
mod flows;
mod notifications;
mod releases;
pub(crate) fn wake_notifications(owner: i64) {
    notifications::wake(owner);
}
use super::{oauth::AuthDiscordUser, state::WebState};
use axum::{
    Extension, Json, Router,
    extract::{ConnectInfo, DefaultBodyLimit, FromRequestParts, RawForm, State},
    http::{HeaderMap, StatusCode, header, request::Parts},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use axum_extra::extract::{
    PrivateCookieJar,
    cookie::{Cookie, Key, SameSite},
};
use std::{
    net::{IpAddr, SocketAddr},
    sync::Arc,
};
use ultros_api_types::plugin::*;
use ultros_db::{
    UltrosDb,
    plugin::{PluginError, PluginKeys, PluginPrincipal},
};
use url::Url;

const APPROVAL_COOKIE: &str = "ultros_game_approval";
fn parse_trusted_proxy(value: &str) -> anyhow::Result<ipnet::IpNet> {
    if let Ok(ip) = value.parse::<IpAddr>() {
        return Ok(ip.into());
    }
    Ok(value.parse()?)
}

fn forwarded_client(
    mut peer: Option<IpAddr>,
    headers: &HeaderMap,
    proxies: &[ipnet::IpNet],
) -> Option<IpAddr> {
    let trusted = |ip: IpAddr| proxies.iter().any(|network| network.contains(&ip));
    if !peer.is_some_and(trusted) {
        return peer;
    }
    let mut values = headers.get_all("x-forwarded-for").iter();
    let Some(value) = values
        .next()
        .and_then(|v| v.to_str().ok())
        .filter(|v| v.len() <= 512)
    else {
        return peer;
    };
    if values.next().is_some() {
        return peer;
    }
    let Ok(mut chain) = value
        .split(',')
        .map(|part| part.trim().parse::<IpAddr>())
        .collect::<Result<Vec<_>, _>>()
    else {
        return peer;
    };
    while peer.is_some_and(trusted) {
        let Some(next) = chain.pop() else {
            break;
        };
        peer = Some(next);
    }
    peer
}

#[derive(Clone)]
pub(super) struct PluginConfig {
    origin: String,
    keys: PluginKeys,
    local: bool,
    trusted_proxies: Vec<ipnet::IpNet>,
    minimum_version: releases::Version,
    release_root: Option<std::path::PathBuf>,
}

pub(crate) fn loopback_debug_origin(origin: &str) -> bool {
    loopback_debug_address(origin).is_some()
}
pub(crate) fn loopback_debug_address(origin: &str) -> Option<IpAddr> {
    if !cfg!(feature = "plugin-localhost") {
        return None;
    }
    let url = Url::parse(origin).ok()?;
    if !clean_origin(&url) {
        return None;
    }
    match url.host_str()? {
        "localhost" => Some(std::net::Ipv4Addr::LOCALHOST.into()),
        host => host
            .trim_matches(['[', ']'])
            .parse::<IpAddr>()
            .ok()
            .filter(IpAddr::is_loopback),
    }
}
fn clean_origin(u: &Url) -> bool {
    matches!(u.scheme(), "http" | "https")
        && u.username().is_empty()
        && u.password().is_none()
        && u.path() == "/"
        && u.query().is_none()
        && u.fragment().is_none()
        && u.host_str().is_some()
}

impl PluginConfig {
    fn from_env(cookie_key: &Key) -> anyhow::Result<Option<Arc<Self>>> {
        if !std::env::var("ULTROS_PLUGIN_ENABLED").is_ok_and(|v| matches!(v.as_str(), "1" | "true"))
        {
            return Ok(None);
        }
        let raw = std::env::var("HOSTNAME").unwrap_or_else(|_| "https://ultros.app".into());
        let url = Url::parse(&raw)
            .map_err(|_| anyhow::anyhow!("HOSTNAME must be an absolute Ultros origin"))?;
        anyhow::ensure!(
            clean_origin(&url),
            "HOSTNAME must contain an origin without a path, credentials, query or fragment"
        );
        let origin = url.origin().ascii_serialization();
        let local = loopback_debug_origin(&origin);
        anyhow::ensure!(
            url.scheme() == "https" || local,
            "HTTP plugin mode requires the plugin-localhost feature and a loopback HOSTNAME"
        );
        fn deployment_key(name: &str) -> anyhow::Result<Vec<u8>> {
            let value = std::env::var(name).map_err(|_| {
                anyhow::anyhow!("{name} is required when plugin connections are enabled")
            })?;
            anyhow::ensure!(
                value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit()),
                "{name} must contain 64 hexadecimal characters"
            );
            (0..64)
                .step_by(2)
                .map(|i| u8::from_str_radix(&value[i..i + 2], 16).map_err(anyhow::Error::from))
                .collect()
        }
        let keys = if local {
            PluginKeys::new(cookie_key.signing(), cookie_key.encryption(), &origin)
        } else {
            PluginKeys::new(
                &deployment_key("ULTROS_PLUGIN_PAIRING_KEY")?,
                &deployment_key("ULTROS_PLUGIN_IDENTITY_KEY")?,
                &origin,
            )
        }
        .map_err(|_| anyhow::anyhow!("invalid plugin deployment keys"))?;
        let trusted_proxies = std::env::var("ULTROS_PLUGIN_TRUSTED_PROXIES")
            .unwrap_or_default()
            .split(',')
            .filter(|s| !s.trim().is_empty())
            .map(|s| parse_trusted_proxy(s.trim()))
            .collect::<anyhow::Result<Vec<_>>>()?;
        let minimum_version = std::env::var("ULTROS_PLUGIN_MINIMUM_VERSION")
            .unwrap_or_else(|_| releases::MINIMUM_VERSION.to_string())
            .parse::<releases::Version>()
            .map_err(|error| anyhow::anyhow!("invalid ULTROS_PLUGIN_MINIMUM_VERSION: {error}"))?;
        anyhow::ensure!(
            minimum_version >= releases::MINIMUM_VERSION,
            "minimum plugin version must be at least {}",
            releases::MINIMUM_VERSION
        );
        let release_root = std::env::var_os("ULTROS_PLUGIN_RELEASE_DIR")
            .filter(|value| !value.is_empty())
            .map(std::path::PathBuf::from)
            .or_else(|| local.then(|| "artifacts/dalamud/releases".into()));
        Ok(Some(Arc::new(Self {
            origin,
            keys,
            local,
            trusted_proxies,
            minimum_version,
            release_root,
        })))
    }
}

#[derive(Debug)]
pub(super) struct PluginApiError(PluginError);
impl From<PluginError> for PluginApiError {
    fn from(e: PluginError) -> Self {
        Self(e)
    }
}
impl IntoResponse for PluginApiError {
    fn into_response(self) -> Response {
        let status = match &self.0 {
            PluginError::Unauthorized => StatusCode::UNAUTHORIZED,
            PluginError::Forbidden | PluginError::Denied => StatusCode::FORBIDDEN,
            PluginError::RateLimited | PluginError::SlowDown(_) => StatusCode::TOO_MANY_REQUESTS,
            PluginError::Storage => StatusCode::SERVICE_UNAVAILABLE,
            PluginError::Conflict(_) => StatusCode::CONFLICT,
            _ => StatusCode::BAD_REQUEST,
        };
        let retry = match &self.0 {
            PluginError::SlowDown(n) => Some(*n),
            PluginError::RateLimited => Some(60),
            _ => None,
        };
        let mut response = (
            status,
            Json(serde_json::json!({"error":self.0.to_string(),"retry_after":retry})),
        )
            .into_response();
        if let Some(retry) = retry {
            response.headers_mut().insert(
                header::RETRY_AFTER,
                retry.to_string().parse().expect("numeric retry interval"),
            );
        }
        response
    }
}

pub(super) struct PluginUser(pub PluginPrincipal);
impl FromRequestParts<WebState> for PluginUser {
    type Rejection = PluginApiError;
    async fn from_request_parts(
        parts: &mut Parts,
        state: &WebState,
    ) -> Result<Self, Self::Rejection> {
        let token = bearer(&parts.headers)?;
        Ok(Self(state.db.authenticate_plugin(token).await?))
    }
}
fn bearer(headers: &HeaderMap) -> Result<&str, PluginApiError> {
    if headers.get_all(header::AUTHORIZATION).iter().count() != 1 {
        return Err(PluginError::Unauthorized.into());
    }
    let value = headers
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .ok_or(PluginError::Unauthorized)?;
    let (scheme, token) = value.split_once(' ').ok_or(PluginError::Unauthorized)?;
    if !scheme.eq_ignore_ascii_case("Bearer")
        || token.is_empty()
        || token.len() > 128
        || token.bytes().any(|b| b.is_ascii_whitespace())
    {
        return Err(PluginError::Unauthorized.into());
    }
    Ok(token)
}

#[derive(Clone)]
struct ClientIp(String);
async fn protect(
    Extension(config): Extension<Arc<PluginConfig>>,
    mut request: axum::extract::Request,
    next: Next,
) -> Response {
    let game_api = releases::is_game_api(request.uri().path());
    let public_release = releases::is_public_release(request.uri().path());
    let browser = request.uri().path().starts_with("/api/v1/plugin/browser/")
        || matches!(
            request.uri().path(),
            "/api/v1/plugin/pairings/select" | "/api/v1/plugin/pairings/approve"
        );
    let expected = Url::parse(&config.origin)
        .expect("validated origin")
        .authority()
        .to_string();
    let mut permitted = request
        .headers()
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .is_some_and(|host| host.eq_ignore_ascii_case(&expected));
    permitted &= request.uri().query().is_none();
    if browser {
        permitted &= !request.headers().contains_key(header::AUTHORIZATION);
        if request.method() != axum::http::Method::GET {
            permitted &= request
                .headers()
                .get(header::ORIGIN)
                .and_then(|h| h.to_str().ok())
                == Some(config.origin.as_str());
        }
    }
    let peer = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|c| c.0.ip());
    let peer = forwarded_client(peer, request.headers(), &config.trusted_proxies);
    request.extensions_mut().insert(ClientIp(
        peer.map(|ip| ip.to_string())
            .unwrap_or_else(|| "internal".into()),
    ));
    let mut response = if !permitted {
        PluginApiError(PluginError::Denied).into_response()
    } else if game_api && !releases::supported(request.headers(), config.minimum_version) {
        releases::update_required(&config)
    } else {
        next.run(request).await
    };
    let headers = response.headers_mut();
    if public_release && permitted {
        headers
            .entry(header::CACHE_CONTROL)
            .or_insert("public, max-age=60".parse().unwrap());
    } else {
        headers.insert(header::CACHE_CONTROL, "private, no-store".parse().unwrap());
    }
    // Chromium serializes a form POST's Origin as "null" under no-referrer.
    // Preserve an exact Origin for CSRF checks while suppressing referrers to
    // other origins. Codes and consent nonces are never placed in URLs.
    headers.insert(
        header::REFERRER_POLICY,
        if browser {
            "same-origin"
        } else {
            "no-referrer"
        }
        .parse()
        .unwrap(),
    );
    headers.insert(header::CONTENT_SECURITY_POLICY,"default-src 'none'; style-src 'unsafe-inline'; form-action 'self'; base-uri 'none'; frame-ancestors 'none'".parse().unwrap());
    headers.insert(header::X_CONTENT_TYPE_OPTIONS, "nosniff".parse().unwrap());
    response
}

pub(crate) fn router(
    key: &Key,
    db: UltrosDb,
    token: tokio_util::sync::CancellationToken,
) -> anyhow::Result<Router<WebState>> {
    let Some(config) = PluginConfig::from_env(key)? else {
        return Ok(Router::new()
            .route(
                "/api/v1/plugin/capabilities",
                get(|| async { StatusCode::SERVICE_UNAVAILABLE }),
            )
            .route(
                "/api/v1/plugin/browser/info",
                get(|| async {
                    Json(PluginWebsiteInfo {
                        enabled: false,
                        localhost: false,
                        origin: String::new(),
                        download_url: None,
                        repository_url: None,
                        latest_version: None,
                        minimum_version: None,
                    })
                }),
            ));
    };
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(300));
        loop {
            tokio::select! { _=token.cancelled()=>break,_=interval.tick()=>{ if db.cleanup_plugin_data().await.is_err() { tracing::warn!("plugin retention cleanup unavailable"); } } }
        }
    });
    let auth = Router::new()
        .route("/api/v1/plugin/capabilities", get(releases::capabilities))
        .route("/api/v1/plugin/release", get(releases::current))
        .route("/api/v1/plugin/repository.json", get(releases::repository))
        .route(
            "/api/v1/plugin/releases/{version}/download",
            get(releases::download),
        )
        .route("/api/v1/plugin/pairings", post(create_pairing))
        .route("/api/v1/plugin/pairings/poll", post(poll_pairing))
        .route("/api/v1/plugin/session/refresh", post(refresh))
        .route("/api/v1/plugin/session/revoke", post(disconnect))
        .route("/api/v1/plugin/session/reauthorize", post(reauthorize))
        .route("/api/v1/plugin/session", get(current_session))
        .route("/api/v1/plugin/browser/info", get(browser::info))
        .route("/api/v1/plugin/browser/approval", get(browser::approval))
        .route("/api/v1/plugin/browser/link", post(browser::capture_link))
        .route(
            "/api/v1/plugin/browser/link/select",
            post(browser::select_link),
        )
        .route("/api/v1/plugin/download", get(releases::latest_download))
        .route("/api/v1/plugin/pairings/select", post(browser::select))
        .route("/api/v1/plugin/pairings/approve", post(browser::approve))
        .route("/api/v1/plugin/browser/devices", get(browser::devices))
        .route(
            "/api/v1/plugin/browser/devices/revoke",
            post(browser::revoke),
        )
        .layer(DefaultBodyLimit::max(8192));
    Ok(auth
        .merge(flows::router())
        .layer(middleware::from_fn(protect))
        .layer(Extension(config)))
}

async fn create_pairing(
    State(db): State<UltrosDb>,
    Extension(config): Extension<Arc<PluginConfig>>,
    Extension(ip): Extension<ClientIp>,
    Json(request): Json<CreatePairing>,
) -> Result<Json<PairingChallenge>, PluginApiError> {
    let (device_code, user_code, verification) = db
        .create_plugin_pairing(&config.keys, &request, &ip.0, None)
        .await?;
    Ok(Json(PairingChallenge {
        device_code,
        user_code,
        verification_uri: format!("{}/plugin/verify", config.origin),
        verification_uri_complete: Some(format!(
            "{}/plugin/verify#pair={verification}",
            config.origin
        )),
        expires_in: 300,
        interval: 5,
    }))
}
async fn reauthorize(
    State(db): State<UltrosDb>,
    Extension(config): Extension<Arc<PluginConfig>>,
    Extension(ip): Extension<ClientIp>,
    PluginUser(user): PluginUser,
    Json(request): Json<CreatePairing>,
) -> Result<Json<PairingChallenge>, PluginApiError> {
    let (device_code, user_code, verification) = db
        .create_plugin_pairing(&config.keys, &request, &ip.0, Some(&user))
        .await?;
    Ok(Json(PairingChallenge {
        device_code,
        user_code,
        verification_uri: format!("{}/plugin/verify", config.origin),
        verification_uri_complete: Some(format!(
            "{}/plugin/verify#pair={verification}",
            config.origin
        )),
        expires_in: 300,
        interval: 5,
    }))
}
async fn poll_pairing(
    State(db): State<UltrosDb>,
    Json(request): Json<PollPairing>,
) -> Result<Json<TokenPair>, PluginApiError> {
    Ok(Json(db.poll_plugin_pairing(&request.device_code).await?))
}
async fn refresh(
    State(db): State<UltrosDb>,
    Json(request): Json<RefreshSession>,
) -> Result<Json<TokenPair>, PluginApiError> {
    Ok(Json(
        db.refresh_plugin_session(&request.refresh_token).await?,
    ))
}
async fn disconnect(
    State(db): State<UltrosDb>,
    Json(request): Json<RefreshSession>,
) -> Result<StatusCode, PluginApiError> {
    db.revoke_plugin_credential(&request.refresh_token).await?;
    Ok(StatusCode::NO_CONTENT)
}
async fn current_session(
    State(db): State<UltrosDb>,
    PluginUser(user): PluginUser,
) -> Result<Json<DeviceSession>, PluginApiError> {
    Ok(Json(db.plugin_session(&user).await?))
}

fn approval_cookie(value: String, local: bool) -> Cookie<'static> {
    let mut cookie = Cookie::new(APPROVAL_COOKIE, value);
    cookie.set_path("/");
    cookie.set_secure(!local);
    cookie.set_http_only(true);
    cookie.set_same_site(SameSite::Lax);
    cookie.set_max_age(cookie::time::Duration::minutes(5));
    cookie
}
fn fields(form: RawForm) -> Vec<(String, String)> {
    url::form_urlencoded::parse(&form.0)
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect()
}
fn field<'a>(fields: &'a [(String, String)], name: &str) -> Result<&'a str, PluginApiError> {
    let mut found = fields.iter().filter(|(key, _)| key == name);
    let value = found.next().ok_or(PluginError::Denied)?;
    if found.next().is_some() {
        return Err(PluginError::Denied.into());
    }
    Ok(&value.1)
}
fn session_cookie(jar: &PrivateCookieJar) -> Result<String, PluginApiError> {
    Ok(jar
        .get(APPROVAL_COOKIE)
        .ok_or(PluginError::Denied)?
        .value()
        .to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn forwarded_addresses_cross_only_explicit_trusted_hops() {
        let proxies = ["127.0.0.1", "173.245.48.0/20", "2001:db8:1234::/48"]
            .map(|v| parse_trusted_proxy(v).unwrap());
        let peer = Some("127.0.0.1".parse().unwrap());
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-forwarded-for",
            "198.51.100.111, 203.0.113.7, 173.245.48.9".parse().unwrap(),
        );
        assert_eq!(
            forwarded_client(peer, &headers, &proxies),
            Some("203.0.113.7".parse().unwrap())
        );
        let direct = Some("192.0.2.3".parse().unwrap());
        assert_eq!(
            forwarded_client(direct, &headers, &proxies),
            direct,
            "an untrusted caller cannot supply its own identity"
        );
        headers.insert(
            "x-forwarded-for",
            "2001:db8:abcd::7, 2001:db8:1234::9".parse().unwrap(),
        );
        assert_eq!(
            forwarded_client(peer, &headers, &proxies),
            Some("2001:db8:abcd::7".parse().unwrap())
        );
        headers.insert("x-forwarded-for", "invalid, 173.245.48.9".parse().unwrap());
        assert_eq!(forwarded_client(peer, &headers, &proxies), peer);
        headers.insert("x-forwarded-for", "203.0.113.7".parse().unwrap());
        headers.append("x-forwarded-for", "198.51.100.111".parse().unwrap());
        assert_eq!(forwarded_client(peer, &headers, &proxies), peer);
        assert!(parse_trusted_proxy("not-a-network").is_err());
    }
    #[test]
    fn origins_and_tokens_are_never_accepted_by_appearance_alone() {
        for origin in [
            "http://localhost:8080",
            "http://127.0.0.1:8080",
            "http://[::1]:8080",
        ] {
            assert_eq!(
                loopback_debug_address(origin).is_some_and(|ip| ip.is_loopback()),
                cfg!(feature = "plugin-localhost")
            );
        }
        assert!(loopback_debug_address("http://192.168.1.1:8080").is_none());
        assert!(loopback_debug_address("https://ultros.app").is_none());
        for invalid in [
            "https://user:pass@ultros.app/",
            "https://ultros.app/?code=secret",
            "https://ultros.app/path",
            "file:///C:/x",
        ] {
            assert!(!clean_origin(&Url::parse(invalid).unwrap()));
        }
        let mut headers = HeaderMap::new();
        assert!(bearer(&headers).is_err());
        headers.insert(header::AUTHORIZATION, "Basic abc".parse().unwrap());
        assert!(bearer(&headers).is_err());
        headers.insert(header::AUTHORIZATION, "bearer abc".parse().unwrap());
        assert_eq!(bearer(&headers).unwrap(), "abc");
        headers.append(header::AUTHORIZATION, "Bearer other".parse().unwrap());
        assert!(bearer(&headers).is_err());
    }
}
