//! Evergreen share images. Locale and page identity are explicit URL inputs;
//! cookies, geography and live market services never participate in rendering.

use std::{collections::VecDeque, fmt::Write, sync::Arc};

use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode, header},
    response::Response,
};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokio::sync::{Mutex, Semaphore};
use ultros_api_types::world_helper::WorldHelper;
use ultros_app::social_card::{SocialCardHero, SocialCardKind, parse_locale, social_card_content};
use ultros_item_card::{CardContent, CardHero, CardLocale};

use super::error::WebError;

const CACHE_ENTRIES: usize = 64;
const CACHE_BYTES: usize = 16 * 1024 * 1024;
const CACHE_CONTROL: &str = "public, max-age=86400";

// Keep CPU-heavy font rasterization and PNG encoding off the async workers,
// with bounded parallelism even when crawlers discover many new cards at once.
static RENDER_SLOTS: Semaphore = Semaphore::const_new(2);
static CACHE: Mutex<CardCache> = Mutex::const_new(CardCache {
    entries: VecDeque::new(),
    bytes: 0,
});

#[derive(Clone)]
struct CachedCard {
    png: axum::body::Bytes,
    etag: String,
}

struct CardCache {
    entries: VecDeque<(String, CachedCard)>,
    bytes: usize,
}

impl CardCache {
    fn get(&mut self, key: &str) -> Option<CachedCard> {
        let index = self.entries.iter().position(|(stored, _)| stored == key)?;
        let entry = self.entries.remove(index)?;
        let value = entry.1.clone();
        self.entries.push_back(entry);
        Some(value)
    }

    fn insert(&mut self, key: String, card: CachedCard) {
        if card.png.len() > CACHE_BYTES {
            return;
        }
        if let Some(index) = self.entries.iter().position(|(stored, _)| stored == &key)
            && let Some((_, previous)) = self.entries.remove(index)
        {
            self.bytes -= previous.png.len();
        }
        while self.entries.len() >= CACHE_ENTRIES || self.bytes + card.png.len() > CACHE_BYTES {
            if let Some((_, oldest)) = self.entries.pop_front() {
                self.bytes -= oldest.png.len();
            } else {
                break;
            }
        }
        self.bytes += card.png.len();
        self.entries.push_back((key, card));
    }
}

#[derive(Default, Deserialize)]
pub(crate) struct SocialCardQuery {
    pub(crate) world: Option<String>,
}

fn response(card: CachedCard, headers: &HeaderMap) -> Result<Response, WebError> {
    let unchanged = headers
        .get(header::IF_NONE_MATCH)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            value.split(',').any(|tag| {
                let tag = tag.trim();
                tag == "*" || tag.strip_prefix("W/").unwrap_or(tag) == card.etag
            })
        });
    Ok(Response::builder()
        .status(if unchanged {
            StatusCode::NOT_MODIFIED
        } else {
            StatusCode::OK
        })
        .header(header::CONTENT_TYPE, "image/png")
        .header(header::CACHE_CONTROL, CACHE_CONTROL)
        .header(header::ETAG, card.etag)
        .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
        .body(if unchanged {
            Body::empty()
        } else {
            Body::from(card.png)
        })?)
}

pub(crate) async fn social_card(
    Path((locale_code, kind, key)): Path<(String, String, String)>,
    Query(query): Query<SocialCardQuery>,
    State(worlds): State<Arc<WorldHelper>>,
    headers: HeaderMap,
) -> Result<Response, WebError> {
    let locale = parse_locale(&locale_code).ok_or(WebError::NotFound)?;
    let card_locale = CardLocale::from_code(&locale_code).ok_or(WebError::NotFound)?;
    let kind = SocialCardKind::from_parts(&kind, &key).ok_or(WebError::NotFound)?;
    let scope = match query.world {
        Some(world) if matches!(kind, SocialCardKind::Item(_)) => Some(
            worlds
                .lookup_world_by_name(&world)
                .ok_or(WebError::NotFound)?
                .get_name()
                .to_owned(),
        ),
        Some(_) => return Err(WebError::BadRequest),
        None => None,
    };
    let (kind_part, key_part) = kind.parts();
    let cache_key = format!(
        "{locale_code}/{kind_part}/{key_part}/{}",
        scope.as_deref().unwrap_or("")
    );
    if let Some(card) = CACHE.lock().await.get(&cache_key) {
        return response(card, &headers);
    }
    let permit = RENDER_SLOTS
        .acquire()
        .await
        .map_err(|_| WebError::TemporarilyUnavailable)?;
    // A preceding request may have populated this entry while we queued.
    if let Some(card) = CACHE.lock().await.get(&cache_key) {
        return response(card, &headers);
    }
    let card = tokio::task::spawn_blocking(move || {
        // Keep the permit with the actual CPU work if the HTTP request is
        // cancelled: spawn_blocking tasks cannot be cancelled once running.
        let _permit = permit;
        let content =
            social_card_content(locale, &kind, scope.as_deref()).ok_or(WebError::NotFound)?;
        let hero = match content.hero {
            SocialCardHero::Item(id) => CardHero::Item(id),
            SocialCardHero::Job(glyph) => CardHero::Job(glyph),
            SocialCardHero::Currency => CardHero::Currency,
            SocialCardHero::Search => CardHero::Search,
            SocialCardHero::Analyzer => CardHero::Analyzer,
            SocialCardHero::Help => CardHero::Help,
        };
        let png = ultros_item_card::render_card(&CardContent {
            title: &content.title,
            subtitle: &content.subtitle,
            eyebrow: &content.eyebrow,
            footer: &content.footer,
            hero,
            locale: card_locale,
        })?;
        let mut etag = String::with_capacity(66);
        etag.push('"');
        for byte in Sha256::digest(&png) {
            write!(etag, "{byte:02x}").expect("writing to a String cannot fail");
        }
        etag.push('"');
        Ok::<_, WebError>(CachedCard {
            png: png.into(),
            etag,
        })
    })
    .await
    .map_err(|error| anyhow::anyhow!("social card render task failed: {error}"))??;
    CACHE.lock().await.insert(cache_key, card.clone());
    response(card, &headers)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(bytes: usize) -> CachedCard {
        CachedCard {
            png: vec![0; bytes].into(),
            etag: "\"test\"".into(),
        }
    }

    #[test]
    fn cache_evicts_by_bytes_and_refreshes_recent_access() {
        let mut cache = CardCache {
            entries: VecDeque::new(),
            bytes: 0,
        };
        cache.insert("en".into(), card(CACHE_BYTES / 2));
        cache.insert("ja".into(), card(CACHE_BYTES / 2));
        assert!(cache.get("en").is_some());
        cache.insert("fr".into(), card(1));
        assert!(cache.get("ja").is_none());
        assert!(cache.get("en").is_some());
        assert_eq!(cache.bytes, CACHE_BYTES / 2 + 1);
    }

    #[test]
    fn conditional_requests_keep_cache_headers_and_omit_body() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::IF_NONE_MATCH,
            "\"other\", W/\"test\"".parse().unwrap(),
        );
        let result = response(card(4), &headers).unwrap();
        assert_eq!(result.status(), StatusCode::NOT_MODIFIED);
        assert_eq!(result.headers()[header::CACHE_CONTROL], CACHE_CONTROL);
        assert_eq!(result.headers()[header::ETAG], "\"test\"");
    }
}
