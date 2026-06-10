//! ultros_charts — chart rendering for FFXIV market data.
//!
//! Pure-Rust scene-graph core: chart layouts in [`charts`] build a
//! renderer-agnostic [`scene::Scene`], which [`svg::scene_to_svg`] turns
//! into an SVG string (rasterized to PNG by the server via resvg). PR 2
//! adds a Leptos renderer over the same scenes; PR 3 sparklines.

pub mod charts;
pub mod data;
pub mod scale;
pub mod scene;
pub mod svg;
pub mod theme;

#[cfg(feature = "image")]
mod icon;
#[cfg(feature = "image")]
pub use icon::item_icon_data_uri;

#[cfg(test)]
pub(crate) mod test_util;
