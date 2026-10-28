//! Public, immutable release downloads and the Dalamud custom repository.
use super::*;
use axum::{body::Body, extract::Path};
use serde::{Deserialize, Serialize};
use std::{fmt, path::PathBuf, str::FromStr};
use tokio::io::AsyncReadExt;
use tower::ServiceExt;
use tower_http::services::ServeFile;

pub(super) const MINIMUM_VERSION: Version = Version([1, 0, 0, 0]);
const MAX_PACKAGE: u64 = 128 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct Version([u16; 4]);
impl FromStr for Version {
    type Err = &'static str;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let parts = value.split('.').collect::<Vec<_>>();
        if parts.len() != 4 || value.len() > 23 {
            return Err("use four numeric version components");
        }
        let mut version = [0; 4];
        for (index, part) in parts.iter().enumerate() {
            if part.is_empty()
                || !part.bytes().all(|b| b.is_ascii_digit())
                || (part.len() > 1 && part.starts_with('0'))
            {
                return Err("use canonical numeric version components");
            }
            version[index] = part
                .parse::<u16>()
                .map_err(|_| "version component too large")?;
            if version[index] == u16::MAX {
                return Err("version component too large");
            }
        }
        Ok(Self(version))
    }
}
impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}.{}", self.0[0], self.0[1], self.0[2], self.0[3])
    }
}

#[derive(Clone, Deserialize, Serialize)]
pub(super) struct Release {
    version: String,
    protocol: u32,
    native_abi: u32,
    dalamud_api_level: u32,
    published_at: i64,
    sha256: String,
    size: u64,
    commit: String,
    changelog: String,
}
struct Bundle {
    release: Release,
    zip: PathBuf,
}

async fn bundle(config: &PluginConfig, requested: Option<Version>) -> Option<Bundle> {
    let root = config.release_root.as_ref()?;
    let index = match requested {
        Some(version) => root.join(version.to_string()).join("release.json"),
        None => root.join("current.json"),
    };
    let mut bytes = Vec::new();
    tokio::fs::File::open(index)
        .await
        .ok()?
        .take(8193)
        .read_to_end(&mut bytes)
        .await
        .ok()?;
    if bytes.len() > 8192 {
        return None;
    }
    let release: Release = serde_json::from_slice(&bytes).ok()?;
    let version = release.version.parse::<Version>().ok()?;
    if requested.is_some_and(|v| v != version)
        || (requested.is_none() && version < config.minimum_version)
        || release.protocol != PROTOCOL_VERSION
        || release.native_abi != 2
        || !(15..=99).contains(&release.dalamud_api_level)
        || release.published_at <= 0
        || release.sha256.len() != 64
        || !release.sha256.bytes().all(|b| b.is_ascii_hexdigit())
        || !(1..=MAX_PACKAGE).contains(&release.size)
        || release.changelog.len() > 4096
        || !matches!(release.commit.len(), 40 | 64)
        || !release.commit.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return None;
    }
    // Versions are parsed before constructing a path; clients never supply a file name.
    let zip = root.join(version.to_string()).join("Ultros.zip");
    let metadata = tokio::fs::metadata(&zip).await.ok()?;
    if !metadata.is_file() || metadata.len() != release.size {
        return None;
    }
    Some(Bundle { release, zip })
}

fn download_path(version: &str) -> String {
    format!("/api/v1/plugin/releases/{version}/download")
}

pub(super) async fn website_release(config: &PluginConfig) -> Option<(String, String)> {
    let current = bundle(config, None).await?;
    Some((
        current.release.version.clone(),
        download_path(&current.release.version),
    ))
}

pub(super) async fn capabilities(
    Extension(config): Extension<Arc<PluginConfig>>,
) -> Json<serde_json::Value> {
    let current = bundle(&config, None).await;
    Json(serde_json::json!({
        "protocol": PROTOCOL_VERSION,
        "features": ["device-authorization","lists-2","private-retainers","game-notifications","notification-stream","list-stream","retainer-preferences"],
        "minimum_client_version": config.minimum_version.to_string(),
        "latest_client_version": current.map(|c| c.release.version),
        "update_url": format!("{}/plugin", config.origin),
    }))
}

pub(super) async fn current(Extension(config): Extension<Arc<PluginConfig>>) -> Response {
    match bundle(&config, None).await {
        Some(bundle) => (
            [(header::CACHE_CONTROL, "public, max-age=60")],
            Json(bundle.release),
        )
            .into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

pub(super) async fn repository(Extension(config): Extension<Arc<PluginConfig>>) -> Response {
    let entries = match bundle(&config, None).await {
        Some(bundle) => {
            let release = bundle.release;
            let download = format!("{}{}", config.origin, download_path(&release.version));
            vec![serde_json::json!({
                "Author":"Ultros", "Name":"Ultros", "InternalName":"Ultros",
                "AssemblyVersion":release.version, "DalamudApiLevel":release.dalamud_api_level,
                "ApplicableVersion":"any", "IsHide":false, "IsTestingExclusive":false,
                "Punchline":"Market prices, shared lists, retainer listings and game alerts.",
                "Description":"Connect to Ultros for shared shopping lists, private retainer uploads, market prices and notifications. Open with /ultros.",
                "RepoUrl":"https://github.com/akarras/ultros/tree/main/plugins/dalamud",
                "IconUrl":format!("{}/static/android-chrome-512x512.png", config.origin),
                "Tags":["market","shopping","lists","retainers","alerts"],
                "DownloadLinkInstall":download, "DownloadLinkUpdate":download,
                "LastUpdate":release.published_at, "Changelog":release.changelog,
            })]
        }
        None => vec![],
    };
    (
        [(header::CACHE_CONTROL, "public, max-age=60")],
        Json(entries),
    )
        .into_response()
}

pub(super) async fn latest_download(Extension(config): Extension<Arc<PluginConfig>>) -> Response {
    match bundle(&config, None).await {
        Some(bundle) => {
            axum::response::Redirect::temporary(&download_path(&bundle.release.version))
                .into_response()
        }
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

pub(super) async fn download(
    Extension(config): Extension<Arc<PluginConfig>>,
    Path(version): Path<String>,
    request: axum::http::Request<Body>,
) -> Response {
    let Ok(version) = version.parse() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Some(bundle) = bundle(&config, Some(version)).await else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let etag = format!("\"{}\"", bundle.release.sha256);
    let unchanged = request
        .headers()
        .get_all(header::IF_NONE_MATCH)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .any(|value| {
            let value = value.trim().strip_prefix("W/").unwrap_or(value.trim());
            value == etag || value == "*"
        });
    let mut response = if unchanged {
        StatusCode::NOT_MODIFIED.into_response()
    } else {
        ServeFile::new(bundle.zip)
            .oneshot(request)
            .await
            .unwrap()
            .into_response()
    };
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, "application/zip".parse().unwrap());
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        format!("attachment; filename=Ultros-{}.zip", bundle.release.version)
            .parse()
            .unwrap(),
    );
    let cache = if response.status().is_success() || response.status() == StatusCode::NOT_MODIFIED {
        "public, max-age=31536000, immutable"
    } else {
        "no-store"
    };
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, cache.parse().unwrap());
    response
        .headers_mut()
        .insert(header::ETAG, etag.parse().unwrap());
    response
}

pub(super) fn is_game_api(path: &str) -> bool {
    path.starts_with("/api/v1/plugin/")
        && !path.starts_with("/api/v1/plugin/browser/")
        && !path.starts_with("/api/v1/plugin/releases/")
        && !matches!(
            path,
            "/api/v1/plugin/capabilities"
                | "/api/v1/plugin/release"
                | "/api/v1/plugin/repository.json"
                | "/api/v1/plugin/download"
                | "/api/v1/plugin/session/revoke"
                | "/api/v1/plugin/pairings/select"
                | "/api/v1/plugin/pairings/approve"
        )
}
pub(super) fn is_public_release(path: &str) -> bool {
    matches!(
        path,
        "/api/v1/plugin/release" | "/api/v1/plugin/repository.json" | "/api/v1/plugin/download"
    ) || path.starts_with("/api/v1/plugin/releases/")
}
pub(super) fn supported(headers: &HeaderMap, minimum: Version) -> bool {
    fn single<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
        if headers.get_all(name).iter().count() != 1 {
            return None;
        }
        headers.get(name)?.to_str().ok()
    }
    single(headers, "x-ultros-protocol-version") == Some("1")
        && single(headers, "x-ultros-client-version")
            .and_then(|v| v.parse::<Version>().ok())
            .is_some_and(|v| v >= minimum)
}
pub(super) fn update_required(config: &PluginConfig) -> Response {
    (StatusCode::UPGRADE_REQUIRED, [(header::CACHE_CONTROL,"no-store")], Json(serde_json::json!({
        "error":"client_update_required", "minimum_client_version":config.minimum_version.to_string(),
        "protocol":PROTOCOL_VERSION,"update_url":format!("{}/plugin",config.origin),
    }))).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn release_metadata_is_bounded_and_old_downloads_stay_immutable() {
        let temporary = tempfile::tempdir().unwrap();
        let mut config = PluginConfig {
            origin: "https://ultros.example".into(),
            keys: PluginKeys::new(&[7; 32], &[8; 32], "https://ultros.example").unwrap(),
            local: false,
            trusted_proxies: vec![],
            minimum_version: MINIMUM_VERSION,
            release_root: Some(temporary.path().to_path_buf()),
        };
        assert!(bundle(&config, None).await.is_none());
        let release = Release {
            version: "1.0.0.0".into(),
            protocol: 1,
            native_abi: 2,
            dalamud_api_level: 15,
            published_at: 1,
            sha256: "a".repeat(64),
            size: 4,
            commit: "b".repeat(40),
            changelog: "First release".into(),
        };
        let folder = temporary.path().join("1.0.0.0");
        std::fs::create_dir(&folder).unwrap();
        std::fs::write(folder.join("Ultros.zip"), b"test").unwrap();
        let index = serde_json::to_vec(&release).unwrap();
        std::fs::write(folder.join("release.json"), &index).unwrap();
        std::fs::write(temporary.path().join("current.json"), &index).unwrap();
        assert!(bundle(&config, None).await.is_some());
        let request = axum::http::Request::builder()
            .header(header::IF_NONE_MATCH, format!("W/\"{}\"", release.sha256))
            .body(Body::empty())
            .unwrap();
        let response = download(
            Extension(Arc::new(config.clone())),
            Path(release.version.clone()),
            request,
        )
        .await;
        assert_eq!(response.status(), StatusCode::NOT_MODIFIED);
        assert_eq!(
            response.headers()[header::CACHE_CONTROL],
            "public, max-age=31536000, immutable"
        );
        config.minimum_version = "1.1.0.0".parse().unwrap();
        assert!(
            bundle(&config, None).await.is_none(),
            "do not advertise a release below the required minimum"
        );
        assert!(
            bundle(&config, Some(MINIMUM_VERSION)).await.is_some(),
            "historical version URLs remain downloadable"
        );
        assert!(
            bundle(&config, Some("1.2.0.0".parse().unwrap()))
                .await
                .is_none()
        );
        config.minimum_version = MINIMUM_VERSION;
        std::fs::write(folder.join("Ultros.zip"), b"truncated").unwrap();
        assert!(bundle(&config, None).await.is_none());
        std::fs::write(folder.join("Ultros.zip"), b"test").unwrap();
        for (key, value) in [
            ("version", serde_json::json!("../../outside")),
            ("protocol", serde_json::json!(2)),
            ("native_abi", serde_json::json!(3)),
            ("sha256", serde_json::json!("not-a-digest")),
            ("size", serde_json::json!(MAX_PACKAGE + 1)),
        ] {
            let mut bad = serde_json::to_value(&release).unwrap();
            bad[key] = value;
            std::fs::write(
                temporary.path().join("current.json"),
                serde_json::to_vec(&bad).unwrap(),
            )
            .unwrap();
            assert!(bundle(&config, None).await.is_none());
        }
        std::fs::write(temporary.path().join("current.json"), vec![b' '; 8193]).unwrap();
        assert!(bundle(&config, None).await.is_none());
    }
    #[test]
    fn versions_are_canonical_and_cannot_be_used_as_paths() {
        for bad in [
            "1.0",
            "1.0.0",
            "1.0.0.0.0",
            "01.0.0.0",
            "+1.0.0.0",
            "1.0.0.65535",
            "1.0.0.-1",
            "../../file",
            "1.0.0.0/file",
            "1.0.0.0?x=1",
            "1.0.0.0 ",
        ] {
            assert!(bad.parse::<Version>().is_err(), "{bad}");
        }
        assert!("1.10.0.0".parse::<Version>().unwrap() > "1.9.9.9".parse().unwrap());
        assert_eq!(MINIMUM_VERSION.to_string(), "1.0.0.0");
    }
    #[test]
    fn every_game_transport_requires_one_supported_version() {
        let mut headers = HeaderMap::new();
        assert!(!supported(&headers, MINIMUM_VERSION));
        headers.insert("x-ultros-protocol-version", "1".parse().unwrap());
        for version in ["0.2.0.0", "garbage", "01.0.0.0"] {
            headers.insert("x-ultros-client-version", version.parse().unwrap());
            assert!(!supported(&headers, MINIMUM_VERSION));
        }
        headers.insert("x-ultros-client-version", "1.0.0.0".parse().unwrap());
        assert!(supported(&headers, MINIMUM_VERSION));
        headers.append("x-ultros-client-version", "1.0.0.0".parse().unwrap());
        assert!(!supported(&headers, MINIMUM_VERSION));
        for path in [
            "/api/v1/plugin/pairings",
            "/api/v1/plugin/pairings/poll",
            "/api/v1/plugin/session/refresh",
            "/api/v1/plugin/notifications/stream",
            "/api/v1/plugin/lists/1/sync",
            "/api/v1/plugin/retainers/1/listings",
        ] {
            assert!(is_game_api(path));
        }
        for path in [
            "/api/v1/plugin/browser/approval",
            "/api/v1/plugin/capabilities",
            "/api/v1/plugin/repository.json",
            "/api/v1/plugin/releases/1.0.0.0/download",
            "/api/v1/plugin/session/revoke",
        ] {
            assert!(!is_game_api(path));
        }
    }
}
