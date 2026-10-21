//! Cookie-authenticated browser endpoints for the website's Leptos plugin area.
use super::*;
use axum::body::Body;

const LINK_COOKIE: &str = "ultros_game_link";

fn clear_link_cookie() -> Cookie<'static> {
    Cookie::build((LINK_COOKIE, "")).path("/").build()
}

/// The fragment credential is exchanged before sign-in. It can select a
/// pairing only; browser authentication and explicit consent remain required.
pub(super) async fn capture_link(
    State(db): State<UltrosDb>,
    Extension(config): Extension<Arc<PluginConfig>>,
    Extension(ip): Extension<ClientIp>,
    jar: PrivateCookieJar,
    form: RawForm,
) -> Response {
    let result = async {
        let fields = fields(form);
        let token = field(&fields, "token")?;
        db.validate_plugin_pairing_link(&config.keys, token, &ip.0)
            .await?;
        Ok::<_, PluginApiError>(token.to_owned())
    }
    .await;
    match result {
        Ok(token) => {
            let mut cookie = approval_cookie(token, config.local);
            cookie.set_name(LINK_COOKIE);
            (jar.add(cookie), StatusCode::NO_CONTENT).into_response()
        }
        Err(error) => (jar.remove(clear_link_cookie()), error).into_response(),
    }
}

pub(super) async fn info(
    Extension(config): Extension<Arc<PluginConfig>>,
) -> Json<PluginWebsiteInfo> {
    let release = releases::website_release(&config).await;
    Json(PluginWebsiteInfo {
        enabled: true,
        localhost: config.local,
        origin: config.origin.clone(),
        download_url: release.as_ref().map(|(_, path)| path.clone()),
        latest_version: release.map(|(version, _)| version),
        minimum_version: Some(config.minimum_version.to_string()),
        repository_url: Some(format!("{}/api/v1/plugin/repository.json", config.origin)),
    })
}

pub(super) async fn approval(
    State(db): State<UltrosDb>,
    Extension(config): Extension<Arc<PluginConfig>>,
    jar: PrivateCookieJar,
    user: AuthDiscordUser,
) -> Result<impl IntoResponse, PluginApiError> {
    let (cookie, csrf) = db.create_plugin_approval(user.id as i64).await?;
    let has_pending_link = jar.get(LINK_COOKIE).is_some();
    Ok((
        jar.add(approval_cookie(cookie, config.local)),
        Json(BrowserApproval {
            account_name: user.name,
            csrf,
            has_pending_link,
        }),
    ))
}

pub(super) async fn select_link(
    State(db): State<UltrosDb>,
    Extension(config): Extension<Arc<PluginConfig>>,
    Extension(ip): Extension<ClientIp>,
    jar: PrivateCookieJar,
    user: AuthDiscordUser,
    form: RawForm,
) -> Response {
    let result = async {
        let fields = fields(form);
        let token = jar
            .get(LINK_COOKIE)
            .ok_or(PluginError::Invalid("invalid_or_expired_link"))?;
        let selected = db
            .select_plugin_pairing_link(
                &config.keys,
                user.id as i64,
                &session_cookie(&jar)?,
                field(&fields, "csrf")?,
                token.value(),
                &ip.0,
            )
            .await?;
        Ok::<_, PluginApiError>(Json(BrowserSelection {
            account_name: user.name,
            code: selected.code,
            label: selected.label,
            scopes: selected.scopes,
            csrf: selected.csrf,
            nonce: selected.nonce,
        }))
    }
    .await;
    (jar.remove(clear_link_cookie()), result).into_response()
}

pub(super) async fn select(
    State(db): State<UltrosDb>,
    Extension(config): Extension<Arc<PluginConfig>>,
    Extension(ip): Extension<ClientIp>,
    jar: PrivateCookieJar,
    user: AuthDiscordUser,
    form: RawForm,
) -> Result<Json<BrowserSelection>, PluginApiError> {
    let fields = fields(form);
    let selected = db
        .select_plugin_pairing(
            &config.keys,
            user.id as i64,
            &session_cookie(&jar)?,
            field(&fields, "csrf")?,
            field(&fields, "user_code")?,
            &ip.0,
        )
        .await?;
    Ok(Json(BrowserSelection {
        account_name: user.name,
        code: selected.code,
        label: selected.label,
        scopes: selected.scopes,
        csrf: selected.csrf,
        nonce: selected.nonce,
    }))
}

pub(super) async fn approve(
    State(db): State<UltrosDb>,
    jar: PrivateCookieJar,
    user: AuthDiscordUser,
    form: RawForm,
) -> Result<Json<BrowserDecision>, PluginApiError> {
    let fields = fields(form);
    let decision = field(&fields, "decision")?;
    if !matches!(decision, "approve" | "deny") {
        return Err(PluginError::Denied.into());
    }
    let scopes = fields
        .iter()
        .filter(|(key, _)| key == "scope")
        .flat_map(|(_, value)| {
            if value == "notifications" {
                vec![
                    "notifications:read".into(),
                    "notifications:ack".into(),
                    "notifications:mark-read".into(),
                ]
            } else {
                vec![value.clone()]
            }
        })
        .collect::<Vec<_>>();
    db.approve_plugin_pairing(
        user.id as i64,
        &session_cookie(&jar)?,
        field(&fields, "csrf")?,
        field(&fields, "nonce")?,
        &scopes,
        decision == "deny",
    )
    .await?;
    Ok(Json(BrowserDecision {
        approved: decision == "approve",
    }))
}

pub(super) async fn devices(
    State(db): State<UltrosDb>,
    Extension(config): Extension<Arc<PluginConfig>>,
    jar: PrivateCookieJar,
    user: AuthDiscordUser,
) -> Result<impl IntoResponse, PluginApiError> {
    let sessions = db.plugin_devices_for_browser(user.id as i64).await?;
    let (cookie, csrf) = db.create_plugin_approval(user.id as i64).await?;
    Ok((
        jar.add(approval_cookie(cookie, config.local)),
        Json(BrowserDevices {
            account_name: user.name,
            csrf,
            sessions,
        }),
    ))
}

pub(super) async fn revoke(
    State(db): State<UltrosDb>,
    jar: PrivateCookieJar,
    user: AuthDiscordUser,
    form: RawForm,
) -> Result<Json<bool>, PluginApiError> {
    let fields = fields(form);
    db.revoke_plugin_device_for_browser(
        user.id as i64,
        field(&fields, "device")?,
        &session_cookie(&jar)?,
        field(&fields, "csrf")?,
    )
    .await?;
    Ok(Json(true))
}

pub(crate) fn is_page(path: &str) -> bool {
    path == "/plugin" || path.starts_with("/plugin/")
}

pub(crate) fn valid_page_request(request: &axum::http::Request<Body>) -> bool {
    !request.headers().contains_key(header::AUTHORIZATION)
        && request.uri().query().is_none_or(|query| {
            let fields = url::form_urlencoded::parse(query.as_bytes()).collect::<Vec<_>>();
            fields.len() == 1
                && fields[0].0 == "lang"
                && matches!(
                    fields[0].1.as_ref(),
                    "en" | "de" | "fr" | "ja" | "cn" | "ko" | "tc"
                )
        })
}

pub(crate) fn page_headers(response: &mut Response) {
    let headers = response.headers_mut();
    headers.insert(header::CACHE_CONTROL, "private, no-store".parse().unwrap());
    headers.insert(header::REFERRER_POLICY, "same-origin".parse().unwrap());
    headers.insert(header::X_FRAME_OPTIONS, "DENY".parse().unwrap());
    headers.insert(header::X_CONTENT_TYPE_OPTIONS, "nosniff".parse().unwrap());
    // The real website needs its own scripts and WASM, but no third-party code
    // or connections execute while a consent form contains a code/nonce.
    headers.insert(header::CONTENT_SECURITY_POLICY, "default-src 'self'; script-src 'self' 'unsafe-inline' 'wasm-unsafe-eval'; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; font-src 'self' data:; connect-src 'self'; frame-src 'none'; object-src 'none'; base-uri 'self'; form-action 'self'; frame-ancestors 'none'".parse().unwrap());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn website_pages_reject_credentials_and_codes_in_urls() {
        for uri in [
            "/plugin",
            "/plugin/",
            "/plugin/verify",
            "/plugin/devices?lang=ja",
        ] {
            assert!(valid_page_request(
                &axum::http::Request::builder()
                    .uri(uri)
                    .body(Body::empty())
                    .unwrap()
            ));
        }
        for uri in [
            "/plugin/verify?code=ABCD-EFGH",
            "/plugin?lang=en&code=ABCD-EFGH",
            "/plugin?lang=unexpected",
            "/plugin?access_token=secret",
        ] {
            assert!(!valid_page_request(
                &axum::http::Request::builder()
                    .uri(uri)
                    .body(Body::empty())
                    .unwrap()
            ));
        }
        assert!(!valid_page_request(
            &axum::http::Request::builder()
                .uri("/plugin/verify")
                .header(header::AUTHORIZATION, "Bearer a-game-grant")
                .body(Body::empty())
                .unwrap()
        ));
    }

    #[test]
    fn website_policy_allows_own_hydration_and_blocks_external_content() {
        let mut response = StatusCode::OK.into_response();
        page_headers(&mut response);
        let headers = response.headers();
        assert_eq!(headers[header::CACHE_CONTROL], "private, no-store");
        assert_eq!(headers[header::REFERRER_POLICY], "same-origin");
        let csp = headers[header::CONTENT_SECURITY_POLICY].to_str().unwrap();
        assert!(csp.contains("script-src 'self' 'unsafe-inline' 'wasm-unsafe-eval'"));
        assert!(csp.contains("connect-src 'self'"));
        assert!(csp.contains("frame-ancestors 'none'"));
    }
}
