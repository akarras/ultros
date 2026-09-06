//! A single native scrollport with independently virtualized rows and columns.
mod filter;
pub mod fixture;
pub mod layout;
pub mod metrics;
pub mod query_grid;
pub mod saved_views;
use crate::i18n::*;
use layout::column_range;
pub(crate) use layout::row_range;
pub use layout::{ColumnFilter, GridColumn, GridLayout};
use leptos::leptos_dom::helpers::{
    AnimationFrameRequestHandle, request_animation_frame_with_handle,
};
use leptos::{portal::Portal, prelude::*};
use std::hash::Hash;
use web_sys::wasm_bindgen::JsCast;

pub const GRID_HEADER_HEIGHT: f64 = 56.0;
pub const GRID_OVERSCAN: usize = 4;

#[derive(Clone, Debug)]
pub struct GridChange {
    pub layout: Option<String>,
    pub visibility: Option<(&'static str, bool)>,
    pub reset: bool,
}

#[derive(Clone)]
struct Drag {
    id: &'static str,
    x: f64,
    width: f64,
    resize: bool,
    original: GridLayout,
    target: Option<(&'static str, bool)>,
}

#[derive(Clone, Copy)]
struct Menu {
    id: &'static str,
    x: f64,
    y: f64,
}

#[component]
pub fn VirtualGrid<T, K, KF, H, F, M>(
    #[prop(into)] each: Signal<Vec<T>>,
    #[prop(into)] columns: Signal<Vec<GridColumn>>,
    #[prop(into)] layout: Signal<Option<String>>,
    on_change: Callback<GridChange>,
    #[prop(into)] reset_scroll: Signal<String>,
    #[prop(optional)] visible_range: Option<RwSignal<(usize, usize)>>,
    #[prop(default = 40.0)] row_height: f64,
    key: KF,
    header: H,
    view: F,
    measure: M,
    #[prop(into)] label: String,
    #[prop(into)] id: String,
) -> impl IntoView
where
    T: Clone + PartialEq + Send + Sync + 'static,
    K: Clone + Eq + Hash + Send + Sync + 'static,
    KF: Fn(&T) -> K + Send + Sync + 'static,
    H: Fn(&'static str) -> AnyView + Send + Sync + 'static,
    F: Fn(T, &'static str) -> AnyView + Send + Sync + 'static,
    M: Fn(&T, &'static str) -> (String, f64) + Send + Sync + 'static,
{
    let i18n = use_i18n();
    let filter_query = crate::components::app_link::use_location_or_default().query;
    let grid_id = StoredValue::new(id);
    let key = StoredValue::new(key);
    let header = StoredValue::new(header);
    let view = StoredValue::new(view);
    let measure = StoredValue::new(measure);
    let port = NodeRef::<leptos::html::Div>::new();
    let menu_ref = NodeRef::<leptos::html::Div>::new();
    let state = RwSignal::new(GridLayout::parse(
        layout.get_untracked().as_deref(),
        &columns.get_untracked(),
    ));
    let drag = RwSignal::new(None::<Drag>);
    let menu = RwSignal::new(None::<Menu>);
    let insert_side = RwSignal::new(None::<bool>);
    let search = RwSignal::new(String::new());
    let width_input = RwSignal::new(String::new());
    // Identical on the server and the first client render.
    let viewport = RwSignal::new((0.0f64, 0.0f64, 800.0f64, 600.0f64));
    let active = RwSignal::new((0usize, 0usize)); // row 0 is the heading
    let active_key = StoredValue::new(None::<K>);
    let active_column = StoredValue::new(None::<&'static str>);
    let interacting = RwSignal::new(false);
    let fit_generation = RwSignal::new(0usize);
    on_cleanup(move || {
        fit_generation.update(|n| *n += 1);
    });

    Effect::new(move |_| {
        let raw = layout.get();
        let defs = columns.get();
        if drag.get_untracked().is_none() {
            state.set(GridLayout::parse(raw.as_deref(), &defs));
        }
    });
    let placed = Memo::new(move |_| state.with(|s| s.columns(&columns.get())));
    let total_width = Memo::new(move |_| {
        placed.with(|cols| cols.last().map(|c| c.left + c.width).unwrap_or(0.0))
    });
    let count = Memo::new(move |_| each.with(Vec::len));
    let rows = Memo::new(move |_| {
        let (_, y, _, h) = viewport.get();
        row_range(
            y,
            (h - GRID_HEADER_HEIGHT).max(0.0),
            row_height,
            count.get(),
            GRID_OVERSCAN,
        )
    });
    let cols = Memo::new(move |_| {
        let (x, _, w, _) = viewport.get();
        placed.with(|p| column_range(p, x, w))
    });
    if let Some(range) = visible_range {
        Effect::new(move |_| range.set(rows.get()));
    }
    let commit = move |visibility| {
        on_change.run(GridChange {
            layout: state.with_untracked(|s| s.compact(&columns.get_untracked())),
            visibility,
            reset: false,
        })
    };
    let sync = move || {
        if let Some(el) = port.get_untracked() {
            if let Some(height) = web_sys::window()
                .and_then(|w| w.inner_height().ok())
                .and_then(|v| v.as_f64())
            {
                let available =
                    (height - el.get_bounding_client_rect().top().max(0.0) - 16.0).max(240.0);
                let _ = web_sys::HtmlElement::style(&el)
                    .set_property("--grid-available-height", &format!("{available}px"));
            }
            viewport.set((
                el.scroll_left(),
                el.scroll_top(),
                el.client_width() as f64,
                el.client_height() as f64,
            ));
        }
    };
    let frame = StoredValue::new(None::<AnimationFrameRequestHandle>);
    let schedule = move || {
        if frame.with_value(Option::is_none) {
            frame.set_value(
                request_animation_frame_with_handle(move || {
                    frame.set_value(None);
                    sync();
                })
                .ok(),
            );
        }
    };
    on_cleanup(move || {
        if let Some(frame) = frame.get_value() {
            frame.cancel();
        }
    });
    Effect::new(move |_| {
        if menu.get().is_some()
            && let Some(el) = menu_ref.get()
        {
            let _ = el.focus();
        }
    });
    #[cfg(feature = "hydrate")]
    {
        use web_sys::wasm_bindgen::closure::Closure;
        let observer =
            StoredValue::new_local(None::<(web_sys::ResizeObserver, Closure<dyn FnMut()>)>);
        Effect::new(move |_| {
            if observer.with_value(Option::is_some) {
                return;
            }
            let Some(el) = port.get() else {
                return;
            };
            let cb = Closure::wrap(Box::new(schedule) as Box<dyn FnMut()>);
            if let Ok(ro) = web_sys::ResizeObserver::new(cb.as_ref().unchecked_ref()) {
                ro.observe(&el);
                if let Some(win) = web_sys::window() {
                    let _ =
                        win.add_event_listener_with_callback("resize", cb.as_ref().unchecked_ref());
                }
                observer.set_value(Some((ro, cb)));
            }
            sync();
        });
        on_cleanup(move || {
            observer.update_value(|slot| {
                if let Some((observer, callback)) = slot.take() {
                    observer.disconnect();
                    if let Some(win) = web_sys::window() {
                        let _ = win.remove_event_listener_with_callback(
                            "resize",
                            callback.as_ref().unchecked_ref(),
                        );
                    }
                }
            })
        });
    }
    Effect::new(move |_| {
        let _ = reset_scroll.get();
        if let Some(el) = port.get() {
            el.set_scroll_top(0.0);
            sync();
        }
        active.set((0, 0));
        active_key.set_value(None);
    });
    // Preserve identity across live updates; filtering may remove the active row.
    Effect::new(move |_| {
        each.with(|data| {
            let mut position = active.get_untracked();
            if let Some(wanted) = active_key.get_value() {
                position.0 = data
                    .iter()
                    .position(|r| key.with_value(|k| k(r)) == wanted)
                    .map(|i| i + 1)
                    .unwrap_or(position.0.min(data.len()));
            }
            if active.get_untracked() != position {
                active.set(position);
            }
            active_key.set_value(
                position
                    .0
                    .checked_sub(1)
                    .and_then(|i| data.get(i))
                    .map(|r| key.with_value(|k| k(r))),
            );
        });
    });
    Effect::new(move |_| {
        let definitions = placed.get();
        let mut position = active.get_untracked();
        position.1 = active_column
            .get_value()
            .and_then(|id| definitions.iter().position(|c| c.column.id == id))
            .unwrap_or(position.1.min(definitions.len().saturating_sub(1)));
        if active.get_untracked() != position {
            active.set(position);
        }
        active_column.set_value(definitions.get(position.1).map(|c| c.column.id));
    });
    let activate = move |r: usize, c: usize| {
        let c = c.min(placed.with_untracked(Vec::len).saturating_sub(1));
        let r = r.min(count.get_untracked());
        active.set((r, c));
        active_key.set_value(each.with_untracked(|data| {
            r.checked_sub(1)
                .and_then(|i| data.get(i))
                .map(|row| key.with_value(|f| f(row)))
        }));
        active_column.set_value(placed.with_untracked(|p| p.get(c).map(|c| c.column.id)));
    };
    let reveal = move |r: usize, c: usize| {
        activate(r, c);
        if let Some(el) = port.get_untracked() {
            if let Some(col) = placed.with_untracked(|p| p.get(c).cloned()) {
                let x = el.scroll_left();
                let width = el.client_width() as f64;
                if col.left < x {
                    el.set_scroll_left(col.left);
                } else if col.left + col.width > x + width {
                    el.set_scroll_left(col.left + col.width - width);
                }
            }
            if r > 0 {
                let y = (r - 1) as f64 * row_height;
                let height = (el.client_height() as f64 - GRID_HEADER_HEIGHT).max(row_height);
                if y < el.scroll_top() {
                    el.set_scroll_top(y);
                } else if y + row_height > el.scroll_top() + height {
                    el.set_scroll_top(y + row_height - height);
                }
            } else {
                el.set_scroll_top(0.0);
            }
            sync();
            let _ = el.focus();
        }
    };
    let close_menu = move || {
        menu.set(None);
        insert_side.set(None);
        search.set(String::new());
        if let Some(el) = port.get_untracked() {
            let _ = el.focus();
        }
    };
    let open_menu = move |id: &'static str, x: f64, y: f64| {
        width_input.set(placed.with_untracked(|p| {
            p.iter()
                .find(|c| c.column.id == id)
                .map(|c| c.width.round().to_string())
                .unwrap_or_default()
        }));
        insert_side.set(None);
        search.set(String::new());
        menu.set(Some(Menu { id, x, y }));
    };
    let fit = move |id: &'static str| {
        #[cfg(feature = "hydrate")]
        {
            fit_generation.update(|n| *n += 1);
            let generation = fit_generation.get_untracked();
            let Some(def) =
                columns.with_untracked(|defs| defs.iter().find(|c| c.id == id).cloned())
            else {
                return;
            };
            let data = each.get_untracked();
            let font = port
                .get_untracked()
                .and_then(|el| web_sys::window()?.get_computed_style(&el).ok().flatten())
                .and_then(|style| style.get_property_value("font").ok())
                .unwrap_or_else(|| "14px sans-serif".into());
            leptos::task::spawn_local(async move {
                let Some(canvas) = web_sys::window()
                    .and_then(|w| w.document())
                    .and_then(|d| d.create_element("canvas").ok())
                    .and_then(|c| c.dyn_into::<web_sys::HtmlCanvasElement>().ok())
                else {
                    return;
                };
                let Some(ctx) = canvas
                    .get_context("2d")
                    .ok()
                    .flatten()
                    .and_then(|c| c.dyn_into::<web_sys::CanvasRenderingContext2d>().ok())
                else {
                    return;
                };
                ctx.set_font(&font);
                let mut width = ctx
                    .measure_text(&def.label)
                    .map(|m| m.width() + 64.0)
                    .unwrap_or(def.width);
                let mut cache = std::collections::HashMap::<String, f64>::new();
                for chunk in data.chunks(256) {
                    if fit_generation.try_get_untracked() != Some(generation) {
                        return;
                    }
                    for row in chunk {
                        let (text, adornments) = measure.with_value(|m| m(row, id));
                        let text_width = *cache.entry(text.clone()).or_insert_with(|| {
                            ctx.measure_text(&text).map(|m| m.width()).unwrap_or(0.0)
                        });
                        width = width.max(text_width + adornments);
                    }
                    gloo_timers::future::TimeoutFuture::new(0).await;
                }
                if fit_generation.try_get_untracked() == Some(generation) {
                    state.update(|s| {
                        s.widths.insert(id.to_string(), def.clamp(width.ceil()));
                    });
                    commit(None);
                }
            });
        }
        #[cfg(not(feature = "hydrate"))]
        let _ = (id, measure);
    };
    let render_cols = Memo::new(move |_| {
        let (start, end) = cols.get();
        let focus_col = active.get().1;
        placed.with(|p| {
            p.iter()
                .cloned()
                .enumerate()
                .filter(|(i, _)| (*i >= start && *i < end) || *i == focus_col)
                .collect::<Vec<_>>()
        })
    });
    let render_rows = Memo::new(move |_| {
        let (start, end) = rows.get();
        let focus_row = active.get().0.checked_sub(1);
        each.with(|data| {
            let mut visible = data[start.min(data.len())..end.min(data.len())]
                .iter()
                .enumerate()
                .map(|(i, row)| (start + i, row.clone()))
                .collect::<Vec<_>>();
            if let Some(i) = focus_row.filter(|i| *i < start || *i >= end)
                && let Some(row) = data.get(i)
            {
                visible.push((i, row.clone()));
                visible.sort_by_key(|(i, _)| *i);
            }
            visible
        })
    });
    // Tab enters the grid once; Enter/F2 opts into controls in the active cell.
    Effect::new(move |_| {
        let _ = render_rows.get();
        let _ = render_cols.get();
        if let Some(el) = port.get()
            && let Ok(nodes) = el.query_selector_all("a,button,input,select,[tabindex]")
        {
            for i in 0..nodes.length() {
                if let Some(node) = nodes
                    .item(i)
                    .and_then(|n| n.dyn_into::<web_sys::Element>().ok())
                {
                    let _ = node.set_attribute("tabindex", "-1");
                }
            }
        }
    });
    let on_key = move |e: web_sys::KeyboardEvent| {
        let (r, c) = active.get_untracked();
        if e.key() == "Escape" {
            if let Some(d) = drag.get_untracked() {
                state.set(d.original);
                drag.set(None);
            }
            interacting.set(false);
            close_menu();
            e.prevent_default();
            return;
        }
        if interacting.get_untracked() {
            if e.key() == "Tab"
                && let Some(el) = port.get_untracked()
            {
                let selector = format!("[data-grid-row='{r}'][data-grid-col='{c}']");
                if let Ok(Some(cell)) = el.query_selector(&selector) {
                    cycle_focus(&e, &cell);
                }
            }
            return;
        }
        if e.key() == "ContextMenu" || (e.shift_key() && e.key() == "F10") {
            if let Some(col) = placed.with_untracked(|p| p.get(c).cloned())
                && let Some(el) = port.get_untracked()
            {
                let rect = el.get_bounding_client_rect();
                open_menu(
                    col.column.id,
                    rect.left() + col.left - el.scroll_left(),
                    rect.top() + GRID_HEADER_HEIGHT,
                );
            }
            e.prevent_default();
            return;
        }
        if e.key() == "Enter" || e.key() == "F2" {
            if let Some(el) = port.get_untracked() {
                let selector = format!(
                    "[data-grid-row='{r}'][data-grid-col='{c}'] a,[data-grid-row='{r}'][data-grid-col='{c}'] button"
                );
                if let Ok(Some(target)) = el.query_selector(&selector)
                    && let Ok(target) = target.dyn_into::<web_sys::HtmlElement>()
                {
                    interacting.set(true);
                    let _ = target.focus();
                }
            }
            e.prevent_default();
            return;
        }
        let page =
            ((viewport.get_untracked().3 - GRID_HEADER_HEIGHT) / row_height).max(1.0) as usize;
        let next = match e.key().as_str() {
            "ArrowDown" => (r + 1, c),
            "ArrowUp" => (r.saturating_sub(1), c),
            "ArrowRight" => (r, c + 1),
            "ArrowLeft" => (r, c.saturating_sub(1)),
            "Home" => (if e.ctrl_key() { 0 } else { r }, 0),
            "End" => (
                if e.ctrl_key() {
                    count.get_untracked()
                } else {
                    r
                },
                placed.with_untracked(Vec::len).saturating_sub(1),
            ),
            "PageDown" => (r.saturating_add(page), c),
            "PageUp" => (r.saturating_sub(page), c),
            _ => return,
        };
        e.prevent_default();
        reveal(
            next.0.min(count.get_untracked()),
            next.1
                .min(placed.with_untracked(Vec::len).saturating_sub(1)),
        );
    };
    view! {
        <div class="virtual-grid-shell">
            <div class="virtual-grid" node_ref=port role="grid" tabindex="0" aria-label=label
                aria-activedescendant=move || { let (r,c)=active.get(); format!("{}-r{r}-c{c}",grid_id.get_value()) }
                aria-rowcount=move || count.get() + 1 aria-colcount=move || placed.with(Vec::len)
                on:keydown=on_key
                on:scroll=move |_| { schedule(); menu.set(None); }
                on:pointermove=move |e: web_sys::PointerEvent| {
                    let Some(mut d) = drag.get_untracked() else { return; };
                    if d.resize {
                        if let Some(def) = columns.with_untracked(|p| p.iter().find(|c| c.id == d.id).cloned()) {
                            state.update(|s| { s.widths.insert(d.id.into(), def.clamp(d.width + e.client_x() - d.x)); });
                        }
                    } else if (e.client_x() - d.x).abs() > 5.0 && let Some(el) = port.get_untracked() {
                            let rect = el.get_bounding_client_rect();
                            if e.client_x() > rect.right() - 40.0 { el.set_scroll_left(el.scroll_left() + 24.0); }
                            if e.client_x() < rect.left() + 40.0 { el.set_scroll_left(el.scroll_left() - 24.0); }
                            let x = e.client_x() - rect.left() + el.scroll_left();
                            d.target = placed.with_untracked(|p| p.iter().find(|c| x >= c.left && x < c.left + c.width)
                                .map(|c| (c.column.id, x > c.left + c.width / 2.0)));
                    }
                    drag.set(Some(d));
                }
                on:pointerup=move |e: web_sys::PointerEvent| {
                    if let Some(d) = drag.get_untracked() {
                        if !d.resize && let Some((target, after)) = d.target { state.update(|s| s.move_to(d.id, target, after)); }
                        drag.set(None); if state.get_untracked()!=d.original {commit(None);}
                        if let Some(el) = port.get_untracked() { let _ = el.release_pointer_capture(e.pointer_id()); }
                    }
                }
                on:pointercancel=move |_| { if let Some(d) = drag.get_untracked() { state.set(d.original); drag.set(None); } }
                style=move || format!("--grid-content-height: {}px;", GRID_HEADER_HEIGHT + count.get() as f64 * row_height + 18.0)
            >
                <div class="virtual-grid-canvas" style=move || format!("width:{}px;min-width:100%;height:{}px;", total_width.get(), GRID_HEADER_HEIGHT + count.get() as f64 * row_height)>
                    <div class="virtual-grid-header" role="row" aria-rowindex="1" style=format!("height:{GRID_HEADER_HEIGHT}px;")>
                        <For each=move || render_cols.get() key=|(i,c)| (*i,c.column.id) children=move |(ci,c)| {
                            let id = c.column.id;
                            let title = c.column.label.clone();
                            view! {
                                <div class="virtual-grid-heading" role="columnheader" aria-colindex=ci + 1 aria-sort=move || placed.with(|p|p.iter().find(|c|c.column.id==id).map(|c|c.column.aria_sort).unwrap_or("none"))
                                    id=format!("{}-r0-c{ci}",grid_id.get_value()) data-column=id data-grid-row="0" data-grid-col=ci
                                    class:grid-active=move || active.get() == (0,ci)
                                    class:grid-filter-active=move || columns.with(|defs| defs.iter().find(|c|c.id==id).is_some_and(|c|
                                        c.filters.iter().any(|f|filter_query.with(|q| if f.metric.is_some() {
                                            metrics::parse_filters(q.get("gf").as_deref()).contains_key(f.key)
                                        } else {q.get(f.key).is_some_and(|v|!v.is_empty())}))))
                                    class:grid-insert-before=move || drag.get().is_some_and(|d| d.target == Some((id,false)))
                                    class:grid-insert-after=move || drag.get().is_some_and(|d| d.target == Some((id,true)))
                                    style=move || placed.with(|p| p.iter().find(|c| c.column.id == id).map(|c| format!("left:{}px;width:{}px;",c.left,c.width)).unwrap_or_default())
                                    on:contextmenu=move |e| { e.prevent_default(); activate(0,ci); open_menu(id,e.client_x(),e.client_y()); }
                                    on:click=move |_| activate(0,ci)
                                >
                                    <button type="button" class="grid-drag-handle" aria-label=t_string!(i18n, grid_move_column).to_string() title=t_string!(i18n, grid_move_column).to_string()
                                        on:pointerdown=move |e: web_sys::PointerEvent| {
                                            if e.button()!=0 { return; } e.prevent_default();
                                            activate(0,ci);
                                            drag.set(Some(Drag { id,x:e.client_x(),width:c.width,resize:false,original:state.get_untracked(),target:None }));
                                            capture_pointer(&e);
                                            if let Some(el)=port.get_untracked(){ let _=el.focus(); }
                                        }
                                    >"⠿"</button>
                                    <div class="grid-heading-content">{header.with_value(|f| f(id))}</div>
                                    <button type="button" class="grid-column-menu" aria-label=format!("{}: {title}",t_string!(i18n, grid_column_menu))
                                        on:click=move |e| { e.stop_propagation(); activate(0,ci); open_menu(id,e.client_x(),e.client_y()); }
                                    >"⋮"</button>
                                    <div class="grid-resize-handle" title=t_string!(i18n, grid_resize_hint).to_string()
                                        on:dblclick=move |e| { e.prevent_default(); e.stop_propagation(); fit(id); }
                                        on:pointerdown=move |e: web_sys::PointerEvent| {
                                            if e.button()!=0 { return; } e.prevent_default(); e.stop_propagation();
                                            activate(0,ci);
                                            fit_generation.update(|n| *n += 1);
                                            let width=placed.with_untracked(|p|p.iter().find(|c|c.column.id==id).map(|c|c.width).unwrap_or(c.width));
                                            drag.set(Some(Drag {id,x:e.client_x(),width,resize:true,original:state.get_untracked(),target:None}));
                                            capture_pointer(&e);
                                            if let Some(el)=port.get_untracked(){ let _=el.focus(); }
                                        }
                                    ></div>
                                </div>
                            }
                        }/>
                    </div>
                    <For each=move || render_rows.get() key=move |(ri,r)| (*ri,key.with_value(|k|k(r))) children=move |(ri,_)| {
                        let row=Memo::new(move |_| each.with(|data| data.get(ri).cloned()));
                        view! {
                            <div role="row" class="virtual-grid-row" data-even=ri % 2 == 0 aria-rowindex=ri + 2 style=format!("top:{}px;height:{row_height}px;",GRID_HEADER_HEIGHT + ri as f64*row_height)>
                                <For each=move || render_cols.get() key=|(i,c)|(*i,c.column.id) children=move |(ci,c)| {
                                    let id=c.column.id;
                                    view! {
                                        <div class="virtual-grid-cell" role="gridcell" aria-colindex=ci + 1 id=format!("{}-r{}-c{ci}",grid_id.get_value(),ri+1) data-column=id data-grid-row=ri + 1 data-grid-col=ci
                                            class:grid-active=move || active.get()==(ri+1,ci)
                                            on:click=move |_| activate(ri+1,ci)
                                            style=move || placed.with(|p|p.iter().find(|c|c.column.id==id).map(|c|format!("left:{}px;width:{}px;",c.left,c.width)).unwrap_or_default())
                                        >{move || view.with_value(|v| row.with(|row|row.as_ref().map(|row|v(row.clone(),id)).into_any()))}</div>
                                    }
                                }/>
                            </div>
                        }
                    }/>
                </div>
            </div>
            {move || menu.get().map(move |m| view! {
                <Portal>
                    <div class="grid-menu-backdrop" on:click=move |_| close_menu()></div>
                    <div class="grid-menu-panel" node_ref=menu_ref tabindex="-1" role="dialog" aria-modal="true" aria-label=t_string!(i18n, grid_column_menu).to_string()
                        style=format!("left:clamp(8px,{}px,calc(100vw - 280px));top:clamp(8px,{}px,calc(100dvh - 430px));",m.x,m.y)
                        on:keydown=move |e| {
                            if e.key()=="Escape"{e.prevent_default();close_menu();}
                            else if let Some(el)=menu_ref.get_untracked(){cycle_focus(&e,&el);}
                        }
                    >
                        <strong>{columns.with(|defs|defs.iter().find(|c|c.id==m.id).map(|c|c.label.clone()).unwrap_or_default())}</strong>
                        {columns.with(|defs|defs.iter().any(|c|c.id==m.id&&c.query_sort)).then(||view! {<filter::MetricSortControls column=m.id/>})}
                        {columns.with(|defs|defs.iter().find(|c|c.id==m.id).map(|c|c.filters.clone()).unwrap_or_default()).into_iter()
                            .map(|filter|view! {<filter::ColumnFilterEditor filter/>}).collect_view()}
                        <button type="button" on:click=move |_| {fit(m.id);close_menu();}>{t!(i18n,grid_auto_fit)}</button>
                        <label>{t!(i18n,grid_width)}<input type="number" min="60" max="800" prop:value=move || width_input.get() on:input=move |e|width_input.set(event_target_value(&e))/></label>
                        <button type="button" on:click=move |_| {
                            if let Ok(width)=width_input.get_untracked().parse::<f64>()
                                && let Some(def)=columns.with_untracked(|defs|defs.iter().find(|c|c.id==m.id).cloned()) {
                                    fit_generation.update(|n| *n += 1);
                                    state.update(|s|{s.widths.insert(m.id.into(),def.clamp(width));}); commit(None);close_menu();
                            }
                        }>{t!(i18n,grid_set_width)}</button>
                        <button type="button" on:click=move |_| {fit_generation.update(|n| *n += 1);state.update(|s|{s.widths.remove(m.id);});commit(None);close_menu();}>{t!(i18n,grid_reset_width)}</button>
                        <div class="grid-menu-actions">
                            <button type="button" on:click=move |_| {
                                let p=placed.get_untracked(); if let Some(i)=p.iter().position(|c|c.column.id==m.id) && i>0 {state.update(|s|s.move_to(m.id,p[i-1].column.id,false));commit(None);} close_menu();
                            }>{t!(i18n,grid_move_left)}</button>
                            <button type="button" on:click=move |_| {
                                let p=placed.get_untracked();if let Some(i)=p.iter().position(|c|c.column.id==m.id) && i+1<p.len(){state.update(|s|s.move_to(m.id,p[i+1].column.id,true));commit(None);} close_menu();
                            }>{t!(i18n,grid_move_right)}</button>
                        </div>
                        <button type="button" on:click=move |_| insert_side.set(Some(false))>{t!(i18n,grid_insert_before)}</button>
                        <button type="button" on:click=move |_| insert_side.set(Some(true))>{t!(i18n,grid_insert_after)}</button>
                        {move || insert_side.get().map(move |after| view! {
                            <input type="search" aria-label=t_string!(i18n,grid_search_columns).to_string() placeholder=t_string!(i18n,grid_search_columns).to_string() on:input=move |e|search.set(event_target_value(&e))/>
                            <div class="grid-insert-options">{move || columns.get().into_iter().filter(|c|c.optional&&!c.visible&&c.label.to_lowercase().contains(&search.get().to_lowercase())).map(|c|view! {
                                <button type="button" on:click=move |_| {state.update(|s|s.move_to(c.id,m.id,after));commit(Some((c.id,true)));close_menu();}>{c.label}</button>
                            }).collect_view()}</div>
                        })}
                        {columns.with(|defs|defs.iter().any(|c|c.id==m.id&&c.optional)).then(||view! {
                            <button type="button" on:click=move |_| {commit(Some((m.id,false)));close_menu();}>{t!(i18n,grid_hide_column)}</button>
                        })}
                        <button type="button" on:click=move |_| {fit_generation.update(|n| *n += 1);on_change.run(GridChange{layout:None,visibility:None,reset:true});close_menu();}>{t!(i18n,grid_reset_layout)}</button>
                        <button type="button" on:click=move |_|close_menu()>{t!(i18n,grid_close)}</button>
                    </div>
                </Portal>
            })}
        </div>
    }
}

// Keep click/double-click targeting on the handle while captured movements
// bubble to the grid. Capturing on the grid itself retargets border clicks.
fn capture_pointer(event: &web_sys::PointerEvent) {
    if let Some(target) = event
        .current_target()
        .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
    {
        let _ = target.set_pointer_capture(event.pointer_id());
    }
}

/// Keyboard alternatives remain usable inside a clipped, virtualized grid.
fn cycle_focus(event: &web_sys::KeyboardEvent, root: &web_sys::Element) {
    if event.key() != "Tab" {
        return;
    }
    let Ok(nodes) = root.query_selector_all("a[href],button:not([disabled]),input,select") else {
        return;
    };
    let targets: Vec<_> = (0..nodes.length())
        .filter_map(|i| {
            nodes
                .item(i)
                .and_then(|n| n.dyn_into::<web_sys::HtmlElement>().ok())
        })
        .collect();
    if targets.is_empty() {
        return;
    }
    let active = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.active_element());
    let index = targets
        .iter()
        .position(|el| active.as_ref().is_some_and(|a| a == el.as_ref()));
    let next = if event.shift_key() {
        index
            .filter(|i| *i > 0)
            .map(|i| i - 1)
            .unwrap_or(targets.len() - 1)
    } else {
        index.map(|i| (i + 1) % targets.len()).unwrap_or(0)
    };
    event.prevent_default();
    let _ = targets[next].focus();
}
