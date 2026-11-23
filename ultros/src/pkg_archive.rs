//! N-1 `/pkg/<git-hash>/` asset archive for rolling `ultros-web` deploys.
//!
//! Each build embeds its own `GIT_HASH` into HTML (`<script src="/pkg/<hash>/ultros.js">`).
//! During a Watchtower rolling restart (or whenever Cloudflare serves cached HTML),
//! a browser that received HTML from replica N-1 may fetch `/pkg/<N-1>/ultros.wasm`
//! from replica N.
//!
//! When `ULTROS_PKG_ARCHIVE_DIR` is configured (pointing at a shared volume across
//! rolling `ultros-web` containers):
//! - On startup, each replica atomically publishes its local `./site/pkg` bundle
//!   into `<archive_dir>/<GIT_HASH>/` (safe under concurrent replicas) and prunes
//!   older hashes beyond `ULTROS_PKG_ARCHIVE_RETAIN` (default 3, minimum 2, never
//!   deleting the running build's hash).
//! - Requests for `/pkg/<current-hash>/*` still serve directly from the local
//!   container bundle (fast, zero runtime dependency on the shared volume).
//! - Requests for `/pkg/<other-hash>/*` serve retained N-1 bundles from the archive
//!   with precompressed `.br`/`.gz`, `Vary: accept-encoding`, and `immutable`
//!   caching on `2xx`/`304` responses — and `Cache-Control: no-store` on errors/404s
//!   so Cloudflare never caches a missing asset.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::body::Body;
use axum::http::{HeaderValue, Request, StatusCode, header};
use axum::response::{IntoResponse, Response};
use tower::ServiceExt;
use tower_http::services::ServeDir;
use tracing::{info, warn};

/// Shared directory retaining recent `/pkg/<git-hash>/` bundles across deploys.
pub const PKG_ARCHIVE_DIR_ENV: &str = "ULTROS_PKG_ARCHIVE_DIR";
/// Number of build hashes to keep in the archive (default 3, clamped `>= 2`).
pub const PKG_ARCHIVE_RETAIN_ENV: &str = "ULTROS_PKG_ARCHIVE_RETAIN";

const DEFAULT_RETAIN_COUNT: usize = 3;
const MIN_RETAIN_COUNT: usize = 2;
const PUBLISHED_MARKER: &str = ".published";
const STALE_TMP_AGE: Duration = Duration::from_secs(60 * 60);

#[derive(Debug, Clone)]
pub struct PkgArchiveConfig {
    pub archive_dir: PathBuf,
    pub retain: usize,
}

impl PkgArchiveConfig {
    pub fn from_env() -> Option<Self> {
        let dir = std::env::var_os(PKG_ARCHIVE_DIR_ENV)?;
        if dir.is_empty() {
            return None;
        }
        let retain = std::env::var(PKG_ARCHIVE_RETAIN_ENV)
            .ok()
            .and_then(|v| v.trim().parse::<usize>().ok())
            .unwrap_or(DEFAULT_RETAIN_COUNT)
            .max(MIN_RETAIN_COUNT);
        Some(Self {
            archive_dir: PathBuf::from(dir),
            retain,
        })
    }
}

/// Validate a `/pkg/<hash>/` path segment: non-empty, no leading dot, ASCII
/// alphanumeric plus `-`/`_` only (prevents directory traversal and blocks
/// internal `.tmp-*` / `.published` entries).
pub fn is_valid_pkg_hash(hash: &str) -> bool {
    !hash.is_empty()
        && hash.len() <= 128
        && !hash.starts_with('.')
        && hash
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// Reject any relative asset path that attempts traversal, contains percent-
/// encoding or URI-significant characters, or targets hidden metadata files
/// (such as `.published` or `.tmp-*`).
///
/// Restricting the alphabet to `[A-Za-z0-9._/-]` guarantees that forwarding
/// `/{rel_path}` to `ServeDir` is a fixed point of percent-decoding: no
/// double-encoded sequence (such as `%252epublished`) can ever decode into a
/// hidden file or traversal segment on `ServeDir`'s URI pass.
pub fn is_safe_asset_path(rel_path: &str) -> bool {
    if rel_path.is_empty() || rel_path.starts_with('/') {
        return false;
    }
    if !rel_path
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'/'))
    {
        return false;
    }
    rel_path
        .split('/')
        .all(|segment| !segment.is_empty() && !segment.starts_with('.'))
}

fn raw_uri_path_has_suspicious_encoding(req: &Request<Body>) -> bool {
    let path = req.uri().path();
    path.contains('%') || path.contains('\\') || path.contains('\0')
}

/// Copy `bundle_dir` into `<archive_dir>/<git_hash>` atomically and prune older
/// hashes down to `retain` (clamped `>= 2`), never removing `git_hash`.
pub fn sync_pkg_archive(
    bundle_dir: &Path,
    archive_dir: &Path,
    git_hash: &str,
    retain: usize,
) -> std::io::Result<()> {
    if !is_valid_pkg_hash(git_hash) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("invalid pkg git hash: {git_hash:?}"),
        ));
    }
    if !bundle_dir.is_dir() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("bundle directory not found: {}", bundle_dir.display()),
        ));
    }
    std::fs::create_dir_all(archive_dir)?;

    let target_dir = archive_dir.join(git_hash);
    if !target_dir.exists() {
        let nonce = unique_nonce();
        let tmp_dir = archive_dir.join(format!(".tmp-{git_hash}-{}-{nonce}", std::process::id()));
        if let Err(err) = copy_dir_recursive(bundle_dir, &tmp_dir) {
            let _ = std::fs::remove_dir_all(&tmp_dir);
            return Err(err);
        }
        write_published_marker(&tmp_dir)?;
        match std::fs::rename(&tmp_dir, &target_dir) {
            Ok(()) => {
                info!(
                    git_hash,
                    archive = %archive_dir.display(),
                    "published pkg bundle to N-1 archive"
                );
            }
            Err(err) => {
                let _ = std::fs::remove_dir_all(&tmp_dir);
                // Another replica of the same build may have won the race.
                if !target_dir.is_dir() {
                    return Err(err);
                }
            }
        }
    } else {
        // Refresh the marker so re-deploying or restarting the active hash
        // keeps it at the top of the retention window.
        let _ = write_published_marker(&target_dir);
    }

    prune_pkg_archive(archive_dir, git_hash, retain.max(MIN_RETAIN_COUNT))?;
    Ok(())
}

fn unique_nonce() -> u64 {
    let mut bytes = [0u8; 8];
    if getrandom::fill(&mut bytes).is_ok() {
        u64::from_le_bytes(bytes)
    } else {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0)
    }
}

fn write_published_marker(dir: &Path) -> std::io::Result<()> {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let marker = dir.join(PUBLISHED_MARKER);
    std::fs::write(marker, nanos.to_string())
}

fn read_published_timestamp(dir: &Path) -> u128 {
    let marker = dir.join(PUBLISHED_MARKER);
    if let Ok(raw) = std::fs::read_to_string(&marker)
        && let Ok(ts) = raw.trim().parse::<u128>()
    {
        return ts;
    }
    std::fs::metadata(dir)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let name = entry.file_name();
        if name.to_string_lossy().starts_with('.') {
            continue;
        }
        let file_type = entry.file_type()?;
        let src_path = entry.path();
        let dst_path = dst.join(name);
        if file_type.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else if file_type.is_file() {
            std::fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

/// Prune archived `/pkg/<hash>/` directories down to `retain` entries (always
/// preserving `current_git_hash`) and reap orphaned `.tmp-*` staging directories
/// older than one hour.
pub fn prune_pkg_archive(
    archive_dir: &Path,
    current_git_hash: &str,
    retain: usize,
) -> std::io::Result<()> {
    let retain = retain.max(MIN_RETAIN_COUNT);
    let now = SystemTime::now();
    let mut others: Vec<(u128, PathBuf)> = Vec::new();

    for entry in std::fs::read_dir(archive_dir)? {
        let entry = entry?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        let path = entry.path();
        if !entry.file_type()?.is_dir() {
            continue;
        }
        if name_str.starts_with(".tmp-") {
            let stale = entry
                .metadata()
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|m| now.duration_since(m).ok())
                .is_some_and(|age| age >= STALE_TMP_AGE);
            if stale {
                let _ = std::fs::remove_dir_all(&path);
            }
            continue;
        }
        if !is_valid_pkg_hash(&name_str) || name_str == current_git_hash {
            continue;
        }
        let ts = read_published_timestamp(&path);
        others.push((ts, path));
    }

    // Keep newest `retain - 1` hashes alongside `current_git_hash`.
    let keep_others = retain.saturating_sub(1);
    others.sort_by_key(|a| std::cmp::Reverse(a.0));
    for (_, stale_dir) in others.into_iter().skip(keep_others) {
        if let Err(err) = std::fs::remove_dir_all(&stale_dir) {
            warn!(
                ?err,
                path = %stale_dir.display(),
                "failed to prune expired pkg archive directory"
            );
        }
    }
    Ok(())
}

/// Apply `/pkg/` caching headers to a `ServeDir` response:
/// - `2xx` and `304 Not Modified`: append `Vary: accept-encoding` and
///   `Cache-Control: public, max-age=31536000, immutable` (or `no-cache` in
///   debug builds).
/// - All other statuses (such as `404 Not Found`): set `Cache-Control: no-store`
///   so Cloudflare never caches a missing asset under an immutable TTL.
pub fn apply_pkg_cache_headers(response: &mut Response) {
    let status = response.status();
    if status.is_success() || status == StatusCode::NOT_MODIFIED {
        response
            .headers_mut()
            .append(header::VARY, HeaderValue::from_static("accept-encoding"));
        let cache_control = if cfg!(debug_assertions) {
            "no-cache"
        } else {
            "public, max-age=31536000, immutable"
        };
        response.headers_mut().insert(
            header::CACHE_CONTROL,
            HeaderValue::from_static(cache_control),
        );
    } else {
        response
            .headers_mut()
            .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    }
}

/// Serve a single file from a precompressed pkg directory (`ServeDir` with
/// `.br`/`.gz` negotiation + safe cache headers).
pub async fn serve_pkg_file(dir: &Path, mut req: Request<Body>) -> Response {
    // Strip any leading slash on the inner request path? `ServeDir` expects a
    // leading `/`.
    if !req.uri().path().starts_with('/')
        && let Ok(uri) = format!("/{}", req.uri().path()).parse()
    {
        *req.uri_mut() = uri;
    }
    let service = ServeDir::new(dir).precompressed_br().precompressed_gzip();
    let mut response = match service.oneshot(req).await {
        Ok(res) => res.into_response(),
        Err(err) => match err {},
    };
    apply_pkg_cache_headers(&mut response);
    response
}

/// Serve a file from the local running container's `/pkg/<current-hash>/`
/// directory after validating the relative path.
pub async fn serve_local_pkg(
    bundle_dir: &Path,
    rel_path: &str,
    mut req: Request<Body>,
) -> Response {
    if raw_uri_path_has_suspicious_encoding(&req) || !is_safe_asset_path(rel_path) {
        let mut resp = StatusCode::NOT_FOUND.into_response();
        apply_pkg_cache_headers(&mut resp);
        return resp;
    }
    let inner_uri = format!("/{rel_path}");
    let Ok(parsed_uri) = inner_uri.parse() else {
        let mut resp = StatusCode::NOT_FOUND.into_response();
        apply_pkg_cache_headers(&mut resp);
        return resp;
    };
    *req.uri_mut() = parsed_uri;
    serve_pkg_file(bundle_dir, req).await
}

/// Serve an archived N-1 `/pkg/{hash}/{*path}` request after strict hash and
/// relative-path validation.
pub async fn serve_archived_pkg(
    archive_dir: &Path,
    hash: &str,
    rel_path: &str,
    mut req: Request<Body>,
) -> Response {
    if raw_uri_path_has_suspicious_encoding(&req)
        || !is_valid_pkg_hash(hash)
        || !is_safe_asset_path(rel_path)
    {
        let mut resp = StatusCode::NOT_FOUND.into_response();
        apply_pkg_cache_headers(&mut resp);
        return resp;
    }
    let hash_dir = archive_dir.join(hash);
    if !hash_dir.is_dir() {
        let mut resp = StatusCode::NOT_FOUND.into_response();
        apply_pkg_cache_headers(&mut resp);
        return resp;
    }
    let inner_uri = format!("/{rel_path}");
    let Ok(parsed_uri) = inner_uri.parse() else {
        let mut resp = StatusCode::NOT_FOUND.into_response();
        apply_pkg_cache_headers(&mut resp);
        return resp;
    };
    *req.uri_mut() = parsed_uri;
    serve_pkg_file(&hash_dir, req).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use http_body_util::BodyExt;

    fn write_bundle(dir: &Path, payload: &[u8]) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join("ultros.wasm"), payload).unwrap();
        std::fs::write(dir.join("ultros.wasm.br"), [b"br:", payload].concat()).unwrap();
        std::fs::write(dir.join("ultros.wasm.gz"), [b"gz:", payload].concat()).unwrap();
    }

    #[test]
    fn hash_and_asset_path_validation_rejects_traversal_and_dotfiles() {
        assert!(is_valid_pkg_hash("abc123def"));
        assert!(is_valid_pkg_hash("2026-11-21_rev1"));
        assert!(!is_valid_pkg_hash(""));
        assert!(!is_valid_pkg_hash(".tmp-abc"));
        assert!(!is_valid_pkg_hash(".."));
        assert!(!is_valid_pkg_hash("abc/def"));

        assert!(is_safe_asset_path("ultros.wasm"));
        assert!(is_safe_asset_path("snippets/foo/bar.js"));
        assert!(!is_safe_asset_path(""));
        assert!(!is_safe_asset_path(".published"));
        assert!(!is_safe_asset_path("sub/.hidden"));
        assert!(!is_safe_asset_path("../secret"));
        assert!(!is_safe_asset_path("a/../../b"));
        assert!(!is_safe_asset_path("a\\b"));
    }

    #[tokio::test]
    async fn archive_publishes_serves_n_minus_1_and_prunes_oldest() {
        let root = tempfile::tempdir().unwrap();
        let archive = root.path().join("archive");
        let b1 = root.path().join("b1");
        let b2 = root.path().join("b2");
        let b3 = root.path().join("b3");
        write_bundle(&b1, b"v1");
        write_bundle(&b2, b"v2");
        write_bundle(&b3, b"v3");

        sync_pkg_archive(&b1, &archive, "hash1", 2).unwrap();
        std::thread::sleep(Duration::from_millis(5));
        sync_pkg_archive(&b2, &archive, "hash2", 2).unwrap();
        assert!(archive.join("hash1").is_dir());
        assert!(archive.join("hash2").is_dir());

        std::thread::sleep(Duration::from_millis(5));
        sync_pkg_archive(&b3, &archive, "hash3", 2).unwrap();
        assert!(
            !archive.join("hash1").exists(),
            "oldest hash should be pruned"
        );
        assert!(archive.join("hash2").is_dir(), "N-1 hash must be retained");
        assert!(
            archive.join("hash3").is_dir(),
            "current hash must be retained"
        );

        // Serve N-1 brotli asset from archive.
        let req = Request::builder()
            .uri("/pkg/hash2/ultros.wasm")
            .header(header::ACCEPT_ENCODING, "gzip, br")
            .body(Body::empty())
            .unwrap();
        let resp = serve_archived_pkg(&archive, "hash2", "ultros.wasm", req).await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(resp.headers()[header::CONTENT_ENCODING], "br");
        assert!(
            resp.headers()
                .get_all(header::VARY)
                .iter()
                .any(|v| v.to_str().unwrap().eq_ignore_ascii_case("accept-encoding"))
        );
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(&body[..], b"br:v2");

        // Missing/pruned hash must return 404 with Cache-Control: no-store.
        let req = Request::builder()
            .uri("/pkg/hash1/ultros.wasm")
            .body(Body::empty())
            .unwrap();
        let resp = serve_archived_pkg(&archive, "hash1", "ultros.wasm", req).await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        assert_eq!(resp.headers()[header::CACHE_CONTROL], "no-store");

        // Dotfile marker inside a valid hash must never be served.
        let req = Request::builder()
            .uri("/pkg/hash2/.published")
            .body(Body::empty())
            .unwrap();
        let resp = serve_archived_pkg(&archive, "hash2", ".published", req).await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        assert_eq!(resp.headers()[header::CACHE_CONTROL], "no-store");
    }
}
