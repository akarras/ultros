//! Deterministic browser fixture. The route and component are absent in release builds.
#[leptos::component(transparent)]
pub fn GridFixtureRoutes() -> impl leptos_router::MatchNestedRoutes + Clone + Send {
    #[cfg(debug_assertions)]
    {
        use leptos::prelude::*;
        use leptos_router::{components::Route, path};
        view! { <Route path=path!("__test/virtual-grid") view=GridFixture/> }.into_inner()
    }
    #[cfg(not(debug_assertions))]
    {}
}

#[cfg(debug_assertions)]
mod development {
    use super::super::query_grid::QueryGrid;
    use super::super::*;
    use leptos_router::hooks::*;

    const IDS: &[&str] = &[
        "c00", "c01", "c02", "c03", "c04", "c05", "c06", "c07", "c08", "c09", "c10", "c11", "c12",
        "c13", "c14", "c15", "c16", "c17", "c18", "c19", "c20", "c21", "c22", "c23", "c24", "c25",
        "c26", "c27", "c28", "c29", "c30", "c31", "c32", "c33", "c34", "c35", "c36", "c37", "c38",
        "c39", "c40", "c41", "c42", "c43", "c44", "c45", "c46", "c47", "c48", "c49", "c50", "c51",
        "c52", "c53", "c54", "c55", "c56", "c57", "c58", "c59", "c60", "c61", "c62", "c63",
    ];

    #[component]
    pub fn GridFixture() -> impl IntoView {
        let query = use_query_map();
        let tick = RwSignal::new(0u32);
        let sorts = RwSignal::new(0u32);
        let size = RwSignal::new(10_000usize);
        let rows = Memo::new(move |_| (0..size.get()).map(|i| (i, tick.get())).collect::<Vec<_>>());
        let columns = Signal::derive(move || {
            IDS.iter()
                .enumerate()
                .map(|(i, id)| {
                    let enabled =
                        query.with(|q| q.get("cols").map(|s| s.split(',').any(|v| v == *id)));
                    GridColumn::new(
                        id,
                        format!("Column {i}"),
                        120.0,
                        i > 0,
                        i == 0 || enabled.unwrap_or(i < 60),
                    )
                })
                .collect::<Vec<_>>()
        });
        let text = |(row, tick): &(usize, u32), id: &str| {
            if *row == 9000 && id == "c00" {
                "A deliberately long value outside the rendered window for auto-fit".to_string()
            } else {
                format!("Row {row} / {id} / {tick}")
            }
        };
        view! {
            <h1>"Virtual grid fixture"</h1>
            <button id="fixture-update" on:click=move |_|tick.update(|t|*t+=1)>"Update values"</button>
            <button id="fixture-empty" on:click=move |_|size.set(0)>"Empty results"</button>
            <button id="fixture-restore" on:click=move |_|size.set(10_000)>"Restore results"</button>
            <span id="fixture-sorts">{move || sorts.get()}</span>
            <QueryGrid id="fixture-grid" label="Grid fixture" each=rows columns
                key=|r:&(usize,u32)|r.0
                header=move |id|view!{<button on:click=move |_|sorts.update(|s|*s+=1)>{id}</button>}.into_any()
                view=move |row,id|view!{<div>{text(&row,id)}</div>}.into_any()
                measure=move |row,id|(text(row,id),32.0)
            />
        }
    }
}
#[cfg(debug_assertions)]
use development::GridFixture;
