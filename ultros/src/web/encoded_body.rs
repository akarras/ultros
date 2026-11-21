//! Pre-compressed immutable response slabs for cached bulk JSON endpoints.
//!
//! Each cached payload stores its raw UTF-8/JSON [`Bytes`] alongside lazily
//! populated `zstd`, `br`, and `gzip` representations in [`OnceLock`]s. Each
//! encoding is compressed at most once per cached generation — off the Tokio
//! reactor thread for payloads larger than [`BLOCKING_COMPRESS_THRESHOLD`] —
//! and every subsequent request serves a zero-copy `Bytes::clone()` with
//! `Content-Encoding` and `Vary: Accept-Encoding` already set so the outer
//! `tower_http::CompressionLayer` skips re-compressing it.

use std::{
    io::Write,
    sync::{Arc, OnceLock},
};

use axum::{
    body::Bytes,
    http::{
        HeaderMap, HeaderName, HeaderValue,
        header::{ACCEPT_ENCODING, CACHE_CONTROL, CONTENT_ENCODING, CONTENT_TYPE, VARY},
    },
    response::{IntoResponse, Response},
};

/// Matches `tower_http::CompressionLayer`'s `SizeAbove::new(256)` threshold:
/// tiny payloads gain nothing from compression and are always served raw.
const MIN_COMPRESS_BYTES: usize = 256;

/// Offload cold compression onto Tokio's blocking pool above this size so
/// compressing a ~1.5 MB bulk market payload never stalls async worker threads.
const BLOCKING_COMPRESS_THRESHOLD: usize = 4 * 1024;

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

/// Immutable response body with lazy, once-per-encoding compressed slabs.
pub(crate) struct EncodedBody {
    raw: Bytes,
    zstd: OnceLock<Option<Bytes>>,
    br: OnceLock<Option<Bytes>>,
    gzip: OnceLock<Option<Bytes>>,
}

impl EncodedBody {
    pub(crate) fn new(raw: impl Into<Bytes>) -> Arc<Self> {
        Arc::new(Self {
            raw: raw.into(),
            zstd: OnceLock::new(),
            br: OnceLock::new(),
            gzip: OnceLock::new(),
        })
    }

    pub(crate) fn raw(&self) -> &Bytes {
        &self.raw
    }

    pub(crate) fn raw_len(&self) -> usize {
        self.raw.len()
    }

    fn cell(&self, encoding: ContentEncoding) -> Option<&OnceLock<Option<Bytes>>> {
        match encoding {
            ContentEncoding::Identity => None,
            ContentEncoding::Zstd => Some(&self.zstd),
            ContentEncoding::Br => Some(&self.br),
            ContentEncoding::Gzip => Some(&self.gzip),
        }
    }

    /// Return the negotiated wire bytes + chosen [`ContentEncoding`], running
    /// cold compression on Tokio's blocking pool for payloads larger than
    /// [`BLOCKING_COMPRESS_THRESHOLD`].
    pub(crate) async fn select(self: &Arc<Self>, headers: &HeaderMap) -> (Bytes, ContentEncoding) {
        if self.raw.len() <= MIN_COMPRESS_BYTES {
            return (self.raw.clone(), ContentEncoding::Identity);
        }
        let requested = negotiate_encoding(headers);
        let Some(cell) = self.cell(requested) else {
            return (self.raw.clone(), ContentEncoding::Identity);
        };

        // Fast path: already compressed for this cache generation.
        if let Some(cached) = cell.get() {
            return match cached {
                Some(bytes) => (bytes.clone(), requested),
                None => (self.raw.clone(), ContentEncoding::Identity),
            };
        }

        let compressed = if self.raw.len() > BLOCKING_COMPRESS_THRESHOLD {
            let raw = self.raw.clone();
            tokio::task::spawn_blocking(move || compress_bytes(&raw, requested))
                .await
                .ok()
                .flatten()
        } else {
            compress_bytes(&self.raw, requested)
        };

        // Never ship a compressed payload that is larger than raw.
        let stored = compressed.filter(|b| b.len() < self.raw.len());
        let chosen = cell.get_or_init(|| stored);
        match chosen {
            Some(bytes) => (bytes.clone(), requested),
            None => (self.raw.clone(), ContentEncoding::Identity),
        }
    }

    /// Build a JSON HTTP response using pre-compressed wire bytes matching
    /// `headers`'s `Accept-Encoding`. Always appends `Vary: Accept-Encoding`.
    pub(crate) async fn into_json_response(
        self: &Arc<Self>,
        headers: &HeaderMap,
        cache_control: HeaderValue,
        extra_headers: &[(HeaderName, HeaderValue)],
    ) -> Response {
        let (body, encoding) = self.select(headers).await;
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

/// Parse RFC 9110 `Accept-Encoding` header(s) and select the highest-q coding
/// we precompress (`zstd` > `br` > `gzip` > `identity` on ties).
pub(crate) fn negotiate_encoding(headers: &HeaderMap) -> ContentEncoding {
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

    let candidates = [
        (ContentEncoding::Zstd, eff(zstd_q, true)),
        (ContentEncoding::Br, eff(br_q, true)),
        (ContentEncoding::Gzip, eff(gzip_q, true)),
        (ContentEncoding::Identity, eff(identity_q, false)),
    ];

    candidates
        .into_iter()
        .filter(|(_, q)| *q > 0)
        // `max_by` returns the LAST max on ties; reverse tie priority so
        // earlier items (`zstd` > `br` > `gzip` > `identity`) win.
        .reduce(|best, cand| if cand.1 > best.1 { cand } else { best })
        .map_or(ContentEncoding::Identity, |(enc, _)| enc)
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
    use std::io::Read;

    fn headers_with(val: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(ACCEPT_ENCODING, HeaderValue::from_str(val).unwrap());
        h
    }

    #[test]
    fn negotiates_encoding_by_qvalue_and_server_preference() {
        assert_eq!(
            negotiate_encoding(&HeaderMap::new()),
            ContentEncoding::Identity
        );
        assert_eq!(
            negotiate_encoding(&headers_with("gzip, deflate, br, zstd")),
            ContentEncoding::Zstd
        );
        assert_eq!(
            negotiate_encoding(&headers_with("gzip, deflate, br")),
            ContentEncoding::Br
        );
        assert_eq!(
            negotiate_encoding(&headers_with("gzip")),
            ContentEncoding::Gzip
        );
        assert_eq!(
            negotiate_encoding(&headers_with("zstd;q=0.5, br;q=0.9, gzip;q=1.0")),
            ContentEncoding::Gzip
        );
        assert_eq!(
            negotiate_encoding(&headers_with("zstd;q=0, br;q=0, gzip")),
            ContentEncoding::Gzip
        );
        assert_eq!(
            negotiate_encoding(&headers_with("br;q=0, *")),
            ContentEncoding::Zstd
        );
        assert_eq!(
            negotiate_encoding(&headers_with("zstd;q=0, br;q=0, gzip;q=0")),
            ContentEncoding::Identity
        );
    }

    #[tokio::test]
    async fn precompressed_variants_round_trip_and_share_allocation() {
        let payload = format!("{{\"items\":[{}]}}", "\"sample_item_entry\",".repeat(400));
        let encoded = EncodedBody::new(payload.clone());

        // zstd round-trip + pointer sharing across repeated hits
        let (z1, e1) = encoded.select(&headers_with("zstd, br, gzip")).await;
        let (z2, e2) = encoded.select(&headers_with("zstd")).await;
        assert_eq!(e1, ContentEncoding::Zstd);
        assert_eq!(e2, ContentEncoding::Zstd);
        assert_eq!(
            z1.as_ptr(),
            z2.as_ptr(),
            "cached zstd bytes must be zero-copy shared"
        );
        assert!(z1.len() < payload.len());
        let decoded_zstd = zstd::stream::decode_all(z1.as_ref()).unwrap();
        assert_eq!(decoded_zstd, payload.as_bytes());

        // br round-trip
        let (b1, eb) = encoded.select(&headers_with("br, gzip")).await;
        assert_eq!(eb, ContentEncoding::Br);
        assert!(b1.len() < payload.len());
        let mut decoded_br = Vec::new();
        brotli::Decompressor::new(b1.as_ref(), 4096)
            .read_to_end(&mut decoded_br)
            .unwrap();
        assert_eq!(decoded_br, payload.as_bytes());

        // gzip round-trip
        let (g1, eg) = encoded.select(&headers_with("gzip")).await;
        assert_eq!(eg, ContentEncoding::Gzip);
        assert!(g1.len() < payload.len());
        let mut decoded_gz = Vec::new();
        flate2::read::GzDecoder::new(g1.as_ref())
            .read_to_end(&mut decoded_gz)
            .unwrap();
        assert_eq!(decoded_gz, payload.as_bytes());

        // identity response always carries Vary: accept-encoding and no Content-Encoding
        let resp = encoded
            .into_json_response(
                &HeaderMap::new(),
                HeaderValue::from_static("public, max-age=60"),
                &[],
            )
            .await;
        assert!(resp.headers().get(CONTENT_ENCODING).is_none());
        assert_eq!(resp.headers().get(VARY).unwrap(), "accept-encoding");
    }

    #[tokio::test]
    async fn tiny_payloads_skip_compression() {
        let encoded = EncodedBody::new(r#"{"ok":true}"#);
        let (body, enc) = encoded.select(&headers_with("zstd, br, gzip")).await;
        assert_eq!(enc, ContentEncoding::Identity);
        assert_eq!(body.as_ref(), br#"{"ok":true}"#);
    }
}
