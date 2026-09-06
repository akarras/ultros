//! URL persistence shared by all analyzer tables. Existing `cols` and JSON
//! `layout` links remain readable; new layouts use a small `l` delta.
use super::{GridChange, GridColumn, VirtualGrid};
use crate::components::app_link::use_location_or_default;
use leptos::prelude::*;
use std::{collections::HashSet, hash::Hash};

#[component]
pub fn QueryGrid<T, K, KF, H, F, M>(
    #[prop(into)] each: Signal<Vec<T>>,
    #[prop(into)] columns: Signal<Vec<GridColumn>>,
    key: KF,
    header: H,
    view: F,
    measure: M,
    #[prop(default = 40.0)] row_height: f64,
    #[prop(optional)] visible_range: Option<RwSignal<(usize, usize)>>,
    #[prop(into)] id: String,
    #[prop(into)] label: String,
) -> impl IntoView
where
    T: Clone + PartialEq + Send + Sync + 'static,
    K: Clone + Eq + Hash + Send + Sync + 'static,
    KF: Fn(&T) -> K + Send + Sync + 'static,
    H: Fn(&'static str) -> AnyView + Send + Sync + 'static,
    F: Fn(T, &'static str) -> AnyView + Send + Sync + 'static,
    M: Fn(&T, &'static str) -> (String, f64) + Send + Sync + 'static,
{
    let location = use_location_or_default();
    let query = location.query;
    let layout = Signal::derive(move || query.with(|q| q.get("l").or_else(|| q.get("layout"))));
    let resolved = Memo::new(move |_| {
        let mut defs = columns.get();
        if let Some(raw) = query.with(|q| q.get("cols")) {
            let visible: HashSet<_> = raw.split(',').collect();
            for col in &mut defs {
                if col.optional {
                    col.visible = visible.contains(col.id);
                }
            }
        }
        defs
    });
    let reset = Memo::new(move |_| {
        let mut q = query.get();
        for key in ["l", "layout", "cols"] {
            q.remove(key);
        }
        q.to_query_string()
    });
    #[cfg(feature = "hydrate")]
    let navigate = leptos_router::hooks::use_navigate();
    let on_change = Callback::new(move |change: GridChange| {
        let mut q = query.get_untracked();
        q.remove("l");
        q.remove("layout");
        if let Some(layout) = change.layout {
            q.insert("l", layout);
        }
        if change.reset {
            q.remove("cols");
        }
        if let Some((id, visible)) = change.visibility {
            let mut defs = resolved.get_untracked();
            if let Some(col) = defs.iter_mut().find(|c| c.id == id) {
                col.visible = visible;
            }
            q.remove("cols");
            q.insert(
                "cols",
                defs.iter()
                    .filter(|c| c.optional && c.visible)
                    .map(|c| c.id)
                    .collect::<Vec<_>>()
                    .join(","),
            );
        }
        #[cfg(feature = "hydrate")]
        navigate(
            &format!(
                "{}{}",
                location.pathname.get_untracked(),
                q.to_query_string()
            ),
            leptos_router::NavigateOptions {
                scroll: false,
                ..Default::default()
            },
        );
    });
    let range = visible_range.unwrap_or_else(|| RwSignal::new((0, 0)));
    view! {
        <VirtualGrid each columns=resolved layout on_change reset_scroll=reset visible_range=range
            key header view measure row_height id label/>
    }
}
