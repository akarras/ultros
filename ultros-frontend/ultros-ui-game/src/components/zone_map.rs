//! A pannable, zoomable in-game map with pins.
//!
//! The image is `/static/map/<map id>.webp`, served out of the map pack that
//! `game-data-pack` extracts from the client. Pins are placed in map
//! coordinates (the `X: 11.8` a player reads in game); [`xiv_gen::Map::fraction`]
//! turns those into a position across the image, so the same pin lands on
//! the same spot whatever size the image is shown at.
//!
//! Geometry is kept in fractions of the viewport rather than pixels: the
//! image is `scale` viewports wide and its top-left corner sits at
//! `(ox, oy)` viewports from the viewport's own corner. That makes the
//! server-rendered starting view (zoomed in on the first pin) identical to
//! the hydrated one, and needs no layout measurement except while dragging.

use crate::i18n::*;
use leptos::prelude::*;
use web_sys::wasm_bindgen::JsCast;
use xiv_gen::MapId;

/// Starting zoom, in viewports of image width. Enough to read a city map's
/// street names without hiding where in the zone the pin is.
const INITIAL_SCALE: f64 = 2.5;
const MIN_SCALE: f64 = 1.0;
const MAX_SCALE: f64 = 8.0;
const BUTTON_ZOOM_STEP: f64 = 1.5;

/// A pin on the map, positioned by map coordinates.
#[derive(Debug, Clone, PartialEq)]
pub struct MapPin {
    pub x: f32,
    pub y: f32,
    /// Shown as the pin's tooltip.
    pub label: String,
}

/// The URL the map pack serves `map_id` at.
pub fn map_image_href(map_id: MapId) -> String {
    format!("/static/map/{}.webp", map_id.0)
}

/// Viewport-fraction view state: image width in viewports, and where its
/// top-left corner sits.
#[derive(Debug, Clone, Copy, PartialEq)]
struct View {
    scale: f64,
    ox: f64,
    oy: f64,
}

impl View {
    /// A view of `scale` centred on image fraction `(fx, fy)`.
    fn centred_on(scale: f64, fx: f64, fy: f64) -> View {
        View {
            scale,
            ox: 0.5 - fx * scale,
            oy: 0.5 - fy * scale,
        }
    }

    /// Zoom by `factor` about viewport point `(px, py)`, keeping the image
    /// point under it fixed.
    fn zoom_about(self, factor: f64, px: f64, py: f64) -> View {
        let scale = (self.scale * factor).clamp(MIN_SCALE, MAX_SCALE);
        let ratio = scale / self.scale;
        View {
            scale,
            ox: px - (px - self.ox) * ratio,
            oy: py - (py - self.oy) * ratio,
        }
        .clamped()
    }

    fn panned(self, dx: f64, dy: f64) -> View {
        View {
            ox: self.ox + dx,
            oy: self.oy + dy,
            ..self
        }
        .clamped()
    }

    /// Keep the image covering the viewport: no blank space past an edge.
    fn clamped(self) -> View {
        let min = 1.0 - self.scale;
        View {
            scale: self.scale,
            ox: self.ox.clamp(min, 0.0),
            oy: self.oy.clamp(min, 0.0),
        }
    }
}

/// Renders the map for `map_id` with `pins` on it. `size_factor` is the
/// map's `Map.size_factor`, which fixes how coordinates spread across it.
#[component]
pub fn ZoneMap(map_id: MapId, size_factor: i32, pins: Vec<MapPin>) -> impl IntoView {
    let i18n = use_i18n();
    let map = xiv_gen::Map {
        key_id: map_id,
        id: String::new(),
        size_factor,
        place_name_region: 0,
        place_name: 0,
        place_name_sub: 0,
        territory_type: 0,
        offset_x: 0,
        offset_y: 0,
    };
    let located: Vec<(f64, f64, String)> = pins
        .into_iter()
        .map(|pin| {
            (
                map.fraction(pin.x) as f64,
                map.fraction(pin.y) as f64,
                pin.label,
            )
        })
        .collect();
    let (fx, fy) = located
        .first()
        .map(|(x, y, _)| (*x, *y))
        .unwrap_or((0.5, 0.5));
    let initial = View::centred_on(INITIAL_SCALE, fx, fy).clamped();
    let view = RwSignal::new(initial);
    // Pointer id and last position (viewport px) of an active drag.
    let drag: RwSignal<Option<(i32, f64, f64)>> = RwSignal::new(None);
    let viewport = NodeRef::<leptos::html::Div>::new();

    let viewport_size = move || {
        viewport
            .get_untracked()
            .map(|el| {
                let rect = el.get_bounding_client_rect();
                (rect.width().max(1.0), rect.height().max(1.0))
            })
            .unwrap_or((1.0, 1.0))
    };
    // Viewport-fraction coordinates of a pointer event.
    let pointer_fraction = move |client_x: f64, client_y: f64| {
        viewport
            .get_untracked()
            .map(|el| {
                let rect = el.get_bounding_client_rect();
                (
                    (client_x - rect.left()) / rect.width().max(1.0),
                    (client_y - rect.top()) / rect.height().max(1.0),
                )
            })
            .unwrap_or((0.5, 0.5))
    };

    let on_wheel = move |e: web_sys::WheelEvent| {
        e.prevent_default();
        let factor = if e.delta_y() < 0.0 { 1.2 } else { 1.0 / 1.2 };
        let (px, py) = pointer_fraction(e.client_x(), e.client_y());
        view.update(|v| *v = v.zoom_about(factor, px, py));
    };
    let on_pointer_down = move |e: web_sys::PointerEvent| {
        if e.button() != 0 {
            return;
        }
        e.prevent_default();
        if let Some(target) = e
            .current_target()
            .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
        {
            let _ = target.set_pointer_capture(e.pointer_id());
        }
        drag.set(Some((e.pointer_id(), e.client_x(), e.client_y())));
    };
    let on_pointer_move = move |e: web_sys::PointerEvent| {
        let Some((id, last_x, last_y)) = drag.get_untracked() else {
            return;
        };
        if id != e.pointer_id() {
            return;
        }
        let (w, h) = viewport_size();
        let (x, y) = (e.client_x(), e.client_y());
        view.update(|v| *v = v.panned((x - last_x) / w, (y - last_y) / h));
        drag.set(Some((id, x, y)));
    };
    let end_drag = move |e: web_sys::PointerEvent| {
        if let Some(target) = e
            .current_target()
            .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
        {
            let _ = target.release_pointer_capture(e.pointer_id());
        }
        drag.set(None);
    };
    let zoom_button = move |factor: f64| {
        view.update(|v| *v = v.zoom_about(factor, 0.5, 0.5));
    };

    let image_style = move || {
        let v = view.get();
        format!(
            "width:{}%;left:{}%;top:{}%",
            v.scale * 100.0,
            v.ox * 100.0,
            v.oy * 100.0
        )
    };
    let pin_views = located
        .iter()
        .map(|(x, y, label)| {
            view! {
                <div
                    class="zone-map-pin"
                    style=format!("left:{}%;top:{}%", x * 100.0, y * 100.0)
                    title=label.clone()
                >
                    <span class="zone-map-pin-dot"></span>
                </div>
            }
        })
        .collect_view();

    view! {
        <div
            node_ref=viewport
            data-zone-map=map_id.0
            class="zone-map relative aspect-square w-full overflow-hidden rounded-lg border border-[color:var(--color-outline)] bg-black/40 touch-none select-none cursor-grab active:cursor-grabbing"
            on:wheel=on_wheel
            on:pointerdown=on_pointer_down
            on:pointermove=on_pointer_move
            on:pointerup=end_drag
            on:pointercancel=end_drag
        >
            <div class="absolute" style=image_style>
                <img
                    src=map_image_href(map_id)
                    alt=""
                    draggable="false"
                    class="block w-full h-auto pointer-events-none"
                    loading="lazy"
                />
                {pin_views}
            </div>
            <div class="absolute right-2 top-2 flex flex-col gap-1">
                <button
                    type="button"
                    class="btn-secondary h-8 w-8 p-0 text-lg leading-none"
                    aria-label=move || t_string!(i18n, npc_map_zoom_in).to_string()
                    on:click=move |_| zoom_button(BUTTON_ZOOM_STEP)
                >
                    "+"
                </button>
                <button
                    type="button"
                    class="btn-secondary h-8 w-8 p-0 text-lg leading-none"
                    aria-label=move || t_string!(i18n, npc_map_zoom_out).to_string()
                    on:click=move |_| zoom_button(1.0 / BUTTON_ZOOM_STEP)
                >
                    "−"
                </button>
                <button
                    type="button"
                    class="btn-secondary h-8 w-8 p-0 text-xs"
                    aria-label=move || t_string!(i18n, npc_map_reset).to_string()
                    on:click=move |_| view.set(initial)
                >
                    "⟲"
                </button>
            </div>
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn centred_view_puts_the_pin_in_the_middle() {
        let v = View::centred_on(2.0, 0.25, 0.75);
        // Image point (0.25, 0.75) maps to viewport ox + 0.25 * 2 = 0.5.
        assert!((v.ox + 0.25 * v.scale - 0.5).abs() < 1e-9);
        assert!((v.oy + 0.75 * v.scale - 0.5).abs() < 1e-9);
    }

    #[test]
    fn zoom_keeps_the_point_under_the_pointer_fixed() {
        let v = View::centred_on(2.0, 0.5, 0.5);
        let (px, py) = (0.2, 0.7);
        // Image fraction under the pointer before and after must agree.
        let before = ((px - v.ox) / v.scale, (py - v.oy) / v.scale);
        let z = v.zoom_about(1.5, px, py);
        let after = ((px - z.ox) / z.scale, (py - z.oy) / z.scale);
        assert!((before.0 - after.0).abs() < 1e-9 && (before.1 - after.1).abs() < 1e-9);
        assert!((z.scale - 3.0).abs() < 1e-9);
    }

    #[test]
    fn view_never_shows_past_the_image_edge() {
        let v = View {
            scale: 2.0,
            ox: 0.3,
            oy: -5.0,
        }
        .clamped();
        assert_eq!((v.ox, v.oy), (0.0, -1.0));
        // Zooming out to 1x collapses to the whole image, corner at origin.
        let z = View::centred_on(1.0, 0.9, 0.1).clamped();
        assert_eq!((z.ox, z.oy), (0.0, 0.0));
        let z = View::centred_on(4.0, 0.5, 0.5).zoom_about(0.01, 0.5, 0.5);
        assert_eq!(z.scale, MIN_SCALE);
    }

    #[test]
    fn ssr_renders_the_image_and_a_pin_per_location() {
        let _ = any_spawner::Executor::init_futures_executor();
        Owner::new().with(|| {
            provide_context(leptos_i18n::context::init_i18n_context::<Locale>());
            let html = view! {
                <ZoneMap
                    map_id=MapId(2)
                    size_factor=200
                    pins=vec![
                        MapPin { x: 11.8, y: 13.4, label: "Gontrant".into() },
                        MapPin { x: 1.0, y: 1.0, label: "Corner".into() },
                    ]
                />
            }
            .to_html();
            assert!(html.contains("/static/map/2.webp"));
            assert_eq!(html.matches("zone-map-pin-dot").count(), 2);
            assert!(html.contains("title=\"Gontrant\""));
            // The corner pin sits at the image origin.
            assert!(html.contains("left:0%;top:0%"));
            assert!(html.contains("Zoom in"));
        });
    }
}
