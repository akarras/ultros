//! Pre-compressed immutable response slabs for cached bulk JSON endpoints.
//!
//! Each cached payload stores its raw UTF-8/JSON [`Bytes`] alongside lazily
//! populated `zstd`, `br`, and `gzip` representations. Concurrent first hits
//! for the same `(generation, encoding)` coalesce behind a single detached
//! compression task owned by the [`Arc<EncodedBody>`] (so cancelling an HTTP
//! request never wastes or duplicates compression), bounded globally by
//! [`MAX_CONCURRENT_COMPRESSIONS`] and offloaded onto Tokio's blocking pool
//! above [`BLOCKING_COMPRESS_THRESHOLD`].
//!
//! Subsequent requests hit the lock-free [`OnceLock`] fast path and serve a
//! zero-copy `Bytes::clone()` with `Content-Encoding` and
//! `Vary: accept-encoding` already set so the outer
//! `tower_http::CompressionLayer` passes the response through unchanged.

use std::{
    io::Write,
    panic::AssertUnwindSafe,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicUsize, Ordering},
    },
};

use axum::{
    body::Bytes,
    http::{
        HeaderMap, HeaderName, HeaderValue, StatusCode,
        header::{ACCEPT_ENCODING, CACHE_CONTROL, CONTENT_ENCODING, CONTENT_TYPE, VARY},
    },
    response::{IntoResponse, Response},
};
use futures::FutureExt;
use tokio::sync::{Semaphore, watch};

/// Matches `tower_http::CompressionLayer`'s `SizeAbove::new(256)` threshold:
/// tiny payloads gain nothing from compression and are served raw whenever
/// `identity` is acceptable to the client.
const MIN_COMPRESS_BYTES: usize = 256;

/// Offload cold compression onto Tokio's blocking pool above this size so
/// compressing a ~1.5 MB bulk market payload never stalls async worker threads.
const BLOCKING_COMPRESS_THRESHOLD: usize = 4 * 1024;

/// Maximum concurrent off-reactor compression jobs across all cached bodies.
const MAX_CONCURRENT_COMPRESSIONS: usize = 4;

static COMPRESSION_SEMAPHORE: OnceLock<Arc<Semaphore>> = OnceLock::new();

fn default_compression_semaphore() -> Arc<Semaphore> {
    COMPRESSION_SEMAPHORE
        .get_or_init(|| Arc::new(Semaphore::new(MAX_CONCURRENT_COMPRESSIONS)))
        .clone()
}

/// Callback invoked once whenever a compressed variant of `bytes` length is
/// installed into an [`EncodedBody`], letting enclosing caches charge variant
/// memory against their hard byte budgets and evict if necessary.
pub(crate) type VariantAccountant = Arc<dyn Fn(usize) + Send + Sync>;

type CompressorFn = fn(&[u8], ContentEncoding) -> Option<Bytes>;

#[cfg(test)]
type BoxedCompressor = Arc<dyn Fn(&[u8], ContentEncoding) -> Option<Bytes> + Send + Sync>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ContentEncoding {
    Identity,
    Gzip,
    Br,
    Zstd,
}

impl ContentEncoding {
    pub(crate) fn header_value(self) -> Option<HeaderValue> {
        match self {
            Self::Identity => None,
            Self::Gzip => Some(HeaderValue::from_static("gzip")),
            Self::Br => Some(HeaderValue::from_static("br")),
            Self::Zstd => Some(HeaderValue::from_static("zstd")),
        }
    }
}

/// Returned by [`EncodedBody::select`] when `Accept-Encoding` forbids every
/// representation we can produce (e.g. `identity;q=0` or `*;q=0`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct NotAcceptable;

/// Parsed RFC 9110 `Accept-Encoding` preference order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct NegotiatedEncodings {
    /// Acceptable encodings we can produce, ranked by highest q-value with
    /// `zstd > br > gzip > identity` on ties. Empty when none is acceptable.
    pub(crate) ordered: Vec<ContentEncoding>,
    /// Whether `identity` has effective quality > 0.
    pub(crate) identity_allowed: bool,
}

#[derive(Default)]
struct EncodingCell {
    /// Lock-free fast path once compression finishes.
    ///
    /// A failed compression (or blocking-pool join error) stores `None` for
    /// this generation so requests do not retry a broken encode on every hit;
    /// callers fall back to the next acceptable coding or let the outer
    /// `CompressionLayer` handle it when `identity` is acceptable.
    ready: OnceLock<Option<Bytes>>,
    /// Coalesces concurrent cold first hits behind one detached task owned by
    /// the [`Arc<EncodedBody>`].
    in_flight: Mutex<Option<watch::Receiver<Option<Option<Bytes>>>>>,
}

/// Immutable response body with lazy, coalesced, once-per-encoding compressed
/// slabs and optional byte-budget accounting.
pub(crate) struct EncodedBody {
    raw: Bytes,
    zstd: EncodingCell,
    br: EncodingCell,
    gzip: EncodingCell,
    variant_bytes: AtomicUsize,
    accountant: OnceLock<VariantAccountant>,
    semaphore: Arc<Semaphore>,
    #[cfg(not(test))]
    compressor: CompressorFn,
    #[cfg(test)]
    compressor: Option<BoxedCompressor>,
    #[cfg(test)]
    default_compressor: CompressorFn,
}

impl EncodedBody {
    pub(crate) fn new(raw: impl Into<Bytes>) -> Arc<Self> {
        Arc::new(Self {
            raw: raw.into(),
            zstd: EncodingCell::default(),
            br: EncodingCell::default(),
            gzip: EncodingCell::default(),
            variant_bytes: AtomicUsize::new(0),
            accountant: OnceLock::new(),
            semaphore: default_compression_semaphore(),
            #[cfg(not(test))]
            compressor: compress_bytes,
            #[cfg(test)]
            compressor: None,
            #[cfg(test)]
            default_compressor: compress_bytes,
        })
    }

    #[cfg(test)]
    fn with_test_compressor(
        raw: impl Into<Bytes>,
        semaphore: Arc<Semaphore>,
        compressor: BoxedCompressor,
    ) -> Arc<Self> {
        Arc::new(Self {
            raw: raw.into(),
            zstd: EncodingCell::default(),
            br: EncodingCell::default(),
            gzip: EncodingCell::default(),
            variant_bytes: AtomicUsize::new(0),
            accountant: OnceLock::new(),
            semaphore,
            compressor: Some(compressor),
            default_compressor: compress_bytes,
        })
    }

    /// Attach a cache byte-budget accountant before publishing this body to
    /// readers. Invoked at most once per compressed variant installed.
    pub(crate) fn attach_accountant(&self, accountant: VariantAccountant) {
        let _ = self.accountant.set(accountant);
    }

    pub(crate) fn raw(&self) -> &Bytes {
        &self.raw
    }

    pub(crate) fn raw_len(&self) -> usize {
        self.raw.len()
    }

    #[cfg(test)]
    pub(crate) fn retained_bytes(&self) -> usize {
        self.raw.len() + self.variant_bytes.load(Ordering::Acquire)
    }

    fn cell(&self, encoding: ContentEncoding) -> Option<&EncodingCell> {
        match encoding {
            ContentEncoding::Identity => None,
            ContentEncoding::Zstd => Some(&self.zstd),
            ContentEncoding::Br => Some(&self.br),
            ContentEncoding::Gzip => Some(&self.gzip),
        }
    }

    fn run_compress(&self, encoding: ContentEncoding) -> Option<Bytes> {
        #[cfg(not(test))]
        {
            (self.compressor)(&self.raw, encoding)
        }
        #[cfg(test)]
        {
            if let Some(custom) = &self.compressor {
                custom(&self.raw, encoding)
            } else {
                (self.default_compressor)(&self.raw, encoding)
            }
        }
    }

    async fn get_or_compress(self: &Arc<Self>, encoding: ContentEncoding) -> Option<Bytes> {
        let cell = self.cell(encoding)?;

        // Fast path: already compressed for this cache generation.
        if let Some(cached) = cell.ready.get() {
            return cached.clone();
        }

        let mut rx = {
            let mut in_flight = cell.in_flight.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(cached) = cell.ready.get() {
                return cached.clone();
            }
            if let Some(existing) = in_flight.as_ref() {
                existing.clone()
            } else {
                let (tx, rx) = watch::channel(None);
                *in_flight = Some(rx.clone());
                let body = Arc::clone(self);
                tokio::spawn(async move {
                    let compressed = body.compress_bounded(encoding).await;
                    let installed = body
                        .cell(encoding)
                        .expect("non-identity cell")
                        .ready
                        .set(compressed.clone())
                        .is_ok();
                    if installed && let Some(bytes) = &compressed {
                        let len = bytes.len();
                        body.variant_bytes.fetch_add(len, Ordering::AcqRel);
                        if let Some(accountant) = body.accountant.get() {
                            accountant(len);
                        }
                    }
                    {
                        let mut guard = body
                            .cell(encoding)
                            .expect("non-identity cell")
                            .in_flight
                            .lock()
                            .unwrap_or_else(|e| e.into_inner());
                        *guard = None;
                    }
                    let _ = tx.send(Some(compressed));
                });
                rx
            }
        };

        loop {
            if let Some(result) = rx.borrow_and_update().clone() {
                return result;
            }
            if rx.changed().await.is_err() {
                return cell.ready.get().cloned().flatten();
            }
        }
    }

    async fn compress_bounded(self: &Arc<Self>, encoding: ContentEncoding) -> Option<Bytes> {
        let _permit = self.semaphore.acquire().await.ok()?;
        let work = async {
            if self.raw.len() > BLOCKING_COMPRESS_THRESHOLD {
                let body = Arc::clone(self);
                tokio::task::spawn_blocking(move || body.run_compress(encoding))
                    .await
                    .ok()
                    .flatten()
            } else {
                self.run_compress(encoding)
            }
        };
        AssertUnwindSafe(work).catch_unwind().await.ok().flatten()
    }

    /// Negotiate `Accept-Encoding` and return the matching wire bytes +
    /// [`ContentEncoding`], or [`NotAcceptable`] when the client excluded
    /// every representation we can produce.
    pub(crate) async fn select(
        self: &Arc<Self>,
        headers: &HeaderMap,
    ) -> Result<(Bytes, ContentEncoding), NotAcceptable> {
        let negotiated = negotiate_encodings(headers);
        if negotiated.ordered.is_empty() {
            return Err(NotAcceptable);
        }

        for encoding in negotiated.ordered {
            if encoding == ContentEncoding::Identity {
                return Ok((self.raw.clone(), ContentEncoding::Identity));
            }
            // Only apply the tiny-body shortcut when `identity` is acceptable
            // to the client; an explicit `identity;q=0` or `*;q=0` must be
            // honored by returning a requested compressed coding.
            if self.raw.len() <= MIN_COMPRESS_BYTES && negotiated.identity_allowed {
                return Ok((self.raw.clone(), ContentEncoding::Identity));
            }
            if let Some(bytes) = self.get_or_compress(encoding).await {
                if bytes.len() < self.raw.len() || !negotiated.identity_allowed {
                    return Ok((bytes, encoding));
                }
                return Ok((self.raw.clone(), ContentEncoding::Identity));
            }
        }

        if negotiated.identity_allowed {
            Ok((self.raw.clone(), ContentEncoding::Identity))
        } else {
            Err(NotAcceptable)
        }
    }

    /// Build a JSON HTTP response using pre-compressed wire bytes matching
    /// `headers`'s `Accept-Encoding`. Always appends `Vary: accept-encoding`
    /// and returns `406 Not Acceptable` if no acceptable encoding exists.
    pub(crate) async fn into_json_response(
        self: &Arc<Self>,
        headers: &HeaderMap,
        cache_control: HeaderValue,
        extra_headers: &[(HeaderName, HeaderValue)],
    ) -> Response {
        let (body, encoding) = match self.select(headers).await {
            Ok(selected) => selected,
            Err(NotAcceptable) => {
                let mut response = StatusCode::NOT_ACCEPTABLE.into_response();
                let resp_headers = response.headers_mut();
                resp_headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
                resp_headers.append(VARY, HeaderValue::from_static("accept-encoding"));
                return response;
            }
        };
        let mut response = body.into_response();
        let resp_headers = response.headers_mut();
        resp_headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        resp_headers.insert(CACHE_CONTROL, cache_control);
        resp_headers.append(VARY, HeaderValue::from_static("accept-encoding"));
        if let Some(enc) = encoding.header_value() {
            resp_headers.insert(CONTENT_ENCODING, enc);
        }
        for (name, value) in extra_headers {
            resp_headers.insert(name.clone(), value.clone());
        }
        response
    }
}

fn compress_bytes(raw: &[u8], encoding: ContentEncoding) -> Option<Bytes> {
    match encoding {
        ContentEncoding::Identity => None,
        ContentEncoding::Zstd => zstd::stream::encode_all(raw, 3).ok().map(Bytes::from),
        ContentEncoding::Br => {
            let mut out = Vec::with_capacity(raw.len() / 4);
            {
                let mut writer = brotli::CompressorWriter::new(&mut out, 4096, 4, 22);
                writer.write_all(raw).ok()?;
                writer.flush().ok()?;
            }
            Some(Bytes::from(out))
        }
        ContentEncoding::Gzip => {
            let mut encoder = flate2::write::GzEncoder::new(
                Vec::with_capacity(raw.len() / 4),
                flate2::Compression::fast(),
            );
            encoder.write_all(raw).ok()?;
            encoder.finish().ok().map(Bytes::from)
        }
    }
}

/// Parse RFC 9110 `Accept-Encoding` header(s) into an ordered list of
/// acceptable codings we can produce (`zstd` > `br` > `gzip` > `identity` on
/// ties) alongside whether `identity` is permitted.
pub(crate) fn negotiate_encodings(headers: &HeaderMap) -> NegotiatedEncodings {
    let mut zstd_q: Option<u16> = None;
    let mut br_q: Option<u16> = None;
    let mut gzip_q: Option<u16> = None;
    let mut identity_q: Option<u16> = None;
    let mut star_q: Option<u16> = None;

    for raw_val in headers.get_all(ACCEPT_ENCODING) {
        let Ok(header_str) = raw_val.to_str() else {
            continue;
        };
        for item in header_str.split(',') {
            let mut parts = item.split(';');
            let coding = parts.next().unwrap_or("").trim().to_ascii_lowercase();
            if coding.is_empty() {
                continue;
            }
            let mut q: u16 = 1000;
            let mut valid = true;
            for param in parts {
                let param = param.trim();
                if let Some((k, v)) = param.split_once('=')
                    && k.trim().eq_ignore_ascii_case("q")
                {
                    match parse_qvalue(v.trim()) {
                        Some(parsed) => q = parsed,
                        None => {
                            valid = false;
                            break;
                        }
                    }
                }
            }
            if !valid {
                continue;
            }
            let slot = match coding.as_str() {
                "zstd" => &mut zstd_q,
                "br" => &mut br_q,
                "gzip" | "x-gzip" => &mut gzip_q,
                "identity" => &mut identity_q,
                "*" => &mut star_q,
                _ => continue,
            };
            // An explicit `q=0` forbids the coding; otherwise keep the highest
            // quality listed across repeated tokens/headers.
            *slot = match *slot {
                Some(0) => Some(0),
                Some(prev) if q > 0 => Some(prev.max(q)),
                _ => Some(q),
            };
        }
    }

    let eff = |explicit: Option<u16>, default_if_star: bool| -> u16 {
        match explicit {
            Some(q) => q,
            None if default_if_star => star_q.unwrap_or(0),
            None => star_q.unwrap_or(1),
        }
    };

    let identity_eff = eff(identity_q, false);
    let mut candidates: Vec<(ContentEncoding, u16, u8)> = [
        (ContentEncoding::Zstd, eff(zstd_q, true), 0),
        (ContentEncoding::Br, eff(br_q, true), 1),
        (ContentEncoding::Gzip, eff(gzip_q, true), 2),
        (ContentEncoding::Identity, identity_eff, 3),
    ]
    .into_iter()
    .filter(|(_, q, _)| *q > 0)
    .collect();

    // Highest q-value first; break ties by server preference (rank ascending).
    candidates.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.2.cmp(&b.2)));

    NegotiatedEncodings {
        ordered: candidates.into_iter().map(|(enc, _, _)| enc).collect(),
        identity_allowed: identity_eff > 0,
    }
}

/// Parse an RFC 9110 qvalue (`0`..=`1` with up to 3 decimal places) into
/// thousandths (`0..=1000`).
fn parse_qvalue(raw: &str) -> Option<u16> {
    let val: f32 = raw.parse().ok()?;
    if !(0.0..=1.0).contains(&val) || val.is_nan() {
        return None;
    }
    Some((val * 1000.0).round() as u16)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Router, body::Body, http::Request, routing::get};
    use http_body_util::BodyExt;
    use std::io::Read;
    use tower::ServiceExt;
    use tower_http::compression::{
        CompressionLayer,
        predicate::{NotForContentType, Predicate, SizeAbove},
    };

    fn headers_with(val: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(ACCEPT_ENCODING, HeaderValue::from_str(val).unwrap());
        h
    }

    #[test]
    fn negotiates_encoding_by_qvalue_and_server_preference() {
        assert_eq!(
            negotiate_encodings(&HeaderMap::new()).ordered,
            vec![ContentEncoding::Identity]
        );
        assert_eq!(
            negotiate_encodings(&headers_with("gzip, deflate, br, zstd")).ordered[0],
            ContentEncoding::Zstd
        );
        assert_eq!(
            negotiate_encodings(&headers_with("gzip, deflate, br")).ordered[0],
            ContentEncoding::Br
        );
        assert_eq!(
            negotiate_encodings(&headers_with("gzip")).ordered[0],
            ContentEncoding::Gzip
        );
        assert_eq!(
            negotiate_encodings(&headers_with("zstd;q=0.5, br;q=0.9, gzip;q=1.0")).ordered,
            vec![
                ContentEncoding::Gzip,
                ContentEncoding::Br,
                ContentEncoding::Zstd,
                ContentEncoding::Identity,
            ]
        );
        assert_eq!(
            negotiate_encodings(&headers_with("zstd;q=0, br;q=0, gzip")).ordered,
            vec![ContentEncoding::Gzip, ContentEncoding::Identity]
        );
        assert_eq!(
            negotiate_encodings(&headers_with("x-gzip;q=0.8")).ordered[0],
            ContentEncoding::Gzip
        );
        assert_eq!(
            negotiate_encodings(&headers_with("*")).ordered[0],
            ContentEncoding::Zstd
        );
        // Invalid qvalue is skipped without rejecting valid siblings.
        assert_eq!(
            negotiate_encodings(&headers_with("gzip;q=abc, br;q=0.5")).ordered,
            vec![ContentEncoding::Br, ContentEncoding::Identity]
        );
        // Multiple Accept-Encoding header lines are combined.
        let mut multi = HeaderMap::new();
        multi.append(ACCEPT_ENCODING, HeaderValue::from_static("gzip;q=0.5"));
        multi.append(ACCEPT_ENCODING, HeaderValue::from_static("zstd;q=0.9"));
        assert_eq!(
            negotiate_encodings(&multi).ordered[0],
            ContentEncoding::Zstd
        );
        // Wildcard and identity exclusions.
        let excl_star = negotiate_encodings(&headers_with("*;q=0"));
        assert!(excl_star.ordered.is_empty());
        assert!(!excl_star.identity_allowed);
        let excl_id = negotiate_encodings(&headers_with("identity;q=0"));
        assert!(excl_id.ordered.is_empty());
        assert!(!excl_id.identity_allowed);
        let gzip_only = negotiate_encodings(&headers_with("gzip, identity;q=0"));
        assert_eq!(gzip_only.ordered, vec![ContentEncoding::Gzip]);
        assert!(!gzip_only.identity_allowed);
    }

    #[tokio::test]
    async fn round_trips_zstd_br_and_gzip_and_reuses_cached_bytes() {
        let payload = serde_json::json!({
            "items": (0..200).map(|i| serde_json::json!({"id": i, "price": i * 100, "world": 54})).collect::<Vec<_>>()
        })
        .to_string();
        let body = EncodedBody::new(payload.clone());

        // Zstd round-trip + pointer reuse on second call
        let (z1, enc1) = body.select(&headers_with("zstd, br, gzip")).await.unwrap();
        assert_eq!(enc1, ContentEncoding::Zstd);
        assert!(z1.len() < payload.len());
        let decoded_zstd = zstd::stream::decode_all(&z1[..]).unwrap();
        assert_eq!(decoded_zstd, payload.as_bytes());
        let (z2, _) = body.select(&headers_with("zstd")).await.unwrap();
        assert_eq!(z1.as_ptr(), z2.as_ptr(), "cached zstd bytes must be reused");

        // Brotli round-trip
        let (b1, enc_b) = body.select(&headers_with("br, gzip")).await.unwrap();
        assert_eq!(enc_b, ContentEncoding::Br);
        assert!(b1.len() < payload.len());
        let mut decoded_br = Vec::new();
        brotli::Decompressor::new(&b1[..], 4096)
            .read_to_end(&mut decoded_br)
            .unwrap();
        assert_eq!(decoded_br, payload.as_bytes());

        // Gzip round-trip
        let (g1, enc_g) = body.select(&headers_with("gzip")).await.unwrap();
        assert_eq!(enc_g, ContentEncoding::Gzip);
        assert!(g1.len() < payload.len());
        let mut decoded_gz = Vec::new();
        flate2::read::GzDecoder::new(&g1[..])
            .read_to_end(&mut decoded_gz)
            .unwrap();
        assert_eq!(decoded_gz, payload.as_bytes());
    }

    #[tokio::test]
    async fn concurrent_first_hits_compress_once_and_respect_concurrency_bound() {
        let payload = "a".repeat(8 * 1024);
        let calls = Arc::new(AtomicUsize::new(0));
        let in_flight = Arc::new(AtomicUsize::new(0));
        let peak_in_flight = Arc::new(AtomicUsize::new(0));
        let sem = Arc::new(Semaphore::new(2));

        let make_compressor = |calls: Arc<AtomicUsize>,
                               in_flight: Arc<AtomicUsize>,
                               peak: Arc<AtomicUsize>|
         -> BoxedCompressor {
            Arc::new(move |raw: &[u8], enc: ContentEncoding| {
                calls.fetch_add(1, Ordering::SeqCst);
                let cur = in_flight.fetch_add(1, Ordering::SeqCst) + 1;
                peak.fetch_max(cur, Ordering::SeqCst);
                std::thread::sleep(std::time::Duration::from_millis(25));
                in_flight.fetch_sub(1, Ordering::SeqCst);
                compress_bytes(raw, enc)
            })
        };

        let body = EncodedBody::with_test_compressor(
            payload.clone(),
            sem.clone(),
            make_compressor(calls.clone(), in_flight.clone(), peak_in_flight.clone()),
        );

        let mut handles = Vec::new();
        for _ in 0..16 {
            let b = body.clone();
            handles.push(tokio::spawn(async move {
                b.select(&headers_with("zstd")).await.unwrap()
            }));
        }
        let mut ptrs = Vec::new();
        for h in handles {
            let (bytes, enc) = h.await.unwrap();
            assert_eq!(enc, ContentEncoding::Zstd);
            ptrs.push(bytes.as_ptr());
        }
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "16 concurrent requests for one generation + coding must compress exactly once"
        );
        assert!(ptrs.iter().all(|&p| p == ptrs[0]));

        // Dropping the first caller mid-compression must still install the
        // variant for a concurrent follower without running a second encode.
        let drop_calls = Arc::new(AtomicUsize::new(0));
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let started_tx = Arc::new(Mutex::new(Some(started_tx)));
        let body2 = EncodedBody::with_test_compressor(
            payload.clone(),
            sem.clone(),
            Arc::new({
                let drop_calls = drop_calls.clone();
                let started_tx = started_tx.clone();
                move |raw, enc| {
                    drop_calls.fetch_add(1, Ordering::SeqCst);
                    if let Some(tx) = started_tx.lock().unwrap().take() {
                        let _ = tx.send(());
                    }
                    std::thread::sleep(std::time::Duration::from_millis(30));
                    compress_bytes(raw, enc)
                }
            }),
        );
        let leader = tokio::spawn({
            let b = body2.clone();
            async move { b.select(&headers_with("gzip")).await }
        });
        started_rx.await.unwrap();
        leader.abort();
        let _ = leader.await;
        let (bytes, enc) = body2.select(&headers_with("gzip")).await.unwrap();
        assert_eq!(enc, ContentEncoding::Gzip);
        assert!(!bytes.is_empty());
        assert_eq!(drop_calls.load(Ordering::SeqCst), 1);

        // Concurrency bound across distinct bodies is enforced by the semaphore.
        let multi_in_flight = Arc::new(AtomicUsize::new(0));
        let multi_peak = Arc::new(AtomicUsize::new(0));
        let mut multi_handles = Vec::new();
        for _ in 0..6 {
            let b = EncodedBody::with_test_compressor(
                payload.clone(),
                sem.clone(),
                make_compressor(
                    Arc::new(AtomicUsize::new(0)),
                    multi_in_flight.clone(),
                    multi_peak.clone(),
                ),
            );
            multi_handles.push(tokio::spawn(async move {
                b.select(&headers_with("gzip")).await.unwrap()
            }));
        }
        for h in multi_handles {
            h.await.unwrap();
        }
        assert!(
            multi_peak.load(Ordering::SeqCst) <= 2,
            "concurrent compressions exceeded semaphore permit limit"
        );
    }

    #[tokio::test]
    async fn production_compression_layer_honors_precompressed_tiny_identity_exclusion_and_406() {
        let large_json = serde_json::json!({
            "rows": (0..100).map(|i| serde_json::json!({"item": i, "price": i * 10})).collect::<Vec<_>>()
        })
        .to_string();
        let large_body = EncodedBody::new(large_json.clone());
        let tiny_body = EncodedBody::new(r#"{"ok":true}"#);

        let app = Router::new()
            .route(
                "/large",
                get({
                    let body = large_body.clone();
                    move |headers: HeaderMap| {
                        let body = body.clone();
                        async move {
                            body.into_json_response(
                                &headers,
                                HeaderValue::from_static("public, max-age=30"),
                                &[],
                            )
                            .await
                        }
                    }
                }),
            )
            .route(
                "/tiny",
                get({
                    let body = tiny_body.clone();
                    move |headers: HeaderMap| {
                        let body = body.clone();
                        async move {
                            body.into_json_response(
                                &headers,
                                HeaderValue::from_static("public, max-age=30"),
                                &[],
                            )
                            .await
                        }
                    }
                }),
            )
            .layer(
                CompressionLayer::new().compress_when(
                    SizeAbove::new(256)
                        .and(NotForContentType::IMAGES)
                        .and(NotForContentType::const_new("application/octet-stream")),
                ),
            );

        // 1. Precompressed large response passes through CompressionLayer
        // unchanged with a single `Vary: accept-encoding`.
        let (expected_zstd, _) = large_body.select(&headers_with("zstd")).await.unwrap();
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/large")
                    .header(ACCEPT_ENCODING, "zstd, br, gzip")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(resp.headers().get(CONTENT_ENCODING).unwrap(), "zstd");
        let vary_values: Vec<&str> = resp
            .headers()
            .get_all(VARY)
            .iter()
            .filter_map(|v| v.to_str().ok())
            .collect();
        assert_eq!(vary_values, vec!["accept-encoding"]);
        let wire = resp.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(wire, expected_zstd);

        // 2. Tiny body with `gzip, identity;q=0` must be gzipped, never
        // served raw identity through the size shortcut.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/tiny")
                    .header(ACCEPT_ENCODING, "gzip, identity;q=0")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(resp.headers().get(CONTENT_ENCODING).unwrap(), "gzip");
        let wire = resp.into_body().collect().await.unwrap().to_bytes();
        let mut decoded = String::new();
        flate2::read::GzDecoder::new(&wire[..])
            .read_to_string(&mut decoded)
            .unwrap();
        assert_eq!(decoded, r#"{"ok":true}"#);

        // 3. Tiny body with normal `gzip` (identity allowed) still takes the
        // size shortcut and serves raw identity.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/tiny")
                    .header(ACCEPT_ENCODING, "gzip")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert!(resp.headers().get(CONTENT_ENCODING).is_none());
        let wire = resp.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(wire.as_ref(), br#"{"ok":true}"#);

        // 4. Wildcard and identity exclusions return 406 Not Acceptable.
        for header in ["*;q=0", "identity;q=0", "deflate, identity;q=0"] {
            let resp = app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri("/tiny")
                        .header(ACCEPT_ENCODING, header)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                resp.status(),
                StatusCode::NOT_ACCEPTABLE,
                "expected 406 for Accept-Encoding: {header}"
            );
            assert!(resp.headers().get(VARY).is_some());
        }
    }
}
