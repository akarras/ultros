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
    {
        DisabledFixtureRoutes
    }
}

// `()` is a route that matches without consuming the URL, not an empty route
// list. As the first child of <Routes>, it shadows every real page in release
// builds. A disabled fixture must both decline matches and generate no routes.
#[cfg(any(not(debug_assertions), test))]
#[derive(Clone)]
struct DisabledFixtureRoutes;

#[cfg(any(not(debug_assertions), test))]
impl leptos_router::MatchNestedRoutes for DisabledFixtureRoutes {
    type Data = ();
    type Match = ();

    fn optional(&self) -> bool {
        false
    }

    fn match_nested<'a>(
        &'a self,
        path: &'a str,
    ) -> (Option<(leptos_router::RouteMatchId, Self::Match)>, &'a str) {
        (None, path)
    }

    fn generate_routes(&self) -> impl IntoIterator<Item = leptos_router::GeneratedRouteData> + '_ {
        std::iter::empty()
    }
}

#[cfg(debug_assertions)]
mod development {
    use super::super::*;
    use leptos_router::{NavigateOptions, hooks::*};

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
        let nav = use_navigate();
        let location = use_location();
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
        let layout = Signal::derive(move || query.with(|q| q.get("layout")));
        let change = Callback::new(move |change: GridChange| {
            let mut q = query.get_untracked();
            q.remove("layout");
            if let Some(layout) = change.layout {
                q.insert("layout", layout);
            }
            if change.reset {
                q.remove("cols");
            }
            if let Some((id, show)) = change.visibility {
                let mut ids: Vec<_> = columns
                    .get_untracked()
                    .iter()
                    .filter(|c| c.visible)
                    .map(|c| c.id)
                    .collect();
                ids.retain(|v| *v != id);
                if show {
                    ids.push(id);
                }
                q.insert("cols", ids.join(","));
            }
            nav(
                &format!(
                    "{}{}",
                    location.pathname.get_untracked(),
                    q.to_query_string()
                ),
                NavigateOptions {
                    scroll: false,
                    ..Default::default()
                },
            );
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
            <VirtualGrid id="fixture-grid" label="Grid fixture" each=rows columns layout on_change=change reset_scroll=Signal::derive(String::new)
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

#[cfg(test)]
mod tests {
    use super::*;
    use leptos::prelude::*;
    use leptos_router::{MatchNestedRoutes, components::Route, path};

    #[test]
    fn disabled_fixture_never_matches_or_registers_a_route() {
        assert_eq!(
            DisabledFixtureRoutes.generate_routes().into_iter().count(),
            0
        );
        for path in [
            "",
            "/",
            "/flip-finder",
            "/recipe-analyzer",
            "/__test/virtual-grid",
        ] {
            let (matched, remaining) = DisabledFixtureRoutes.match_nested(path);
            assert!(matched.is_none(), "disabled fixture intercepted {path}");
            assert_eq!(remaining, path);
        }
    }

    #[test]
    fn disabled_fixture_allows_following_pages_to_match() {
        let routes = (
            DisabledFixtureRoutes,
            view! { <Route path=path!("") view=|| "home"/> }.into_inner(),
            view! { <Route path=path!("flip-finder") view=|| "flips"/> }.into_inner(),
            view! { <Route path=path!("recipe-analyzer") view=|| "recipes"/> }.into_inner(),
        );
        for path in ["/", "/flip-finder", "/recipe-analyzer"] {
            let (matched, remaining) = routes.match_nested(path);
            assert!(matched.is_some(), "page did not match: {path}");
            assert!(remaining.is_empty() || remaining == "/", "{remaining}");
        }
        assert!(routes.match_nested("/__test/virtual-grid").0.is_none());
        assert_eq!(routes.generate_routes().into_iter().count(), 3);
    }

    #[test]
    fn fixture_registration_matches_the_build_profile() {
        let routes = GridFixtureRoutes();
        assert_eq!(
            routes.generate_routes().into_iter().count(),
            usize::from(cfg!(debug_assertions))
        );
        assert_eq!(
            routes.match_nested("/__test/virtual-grid").0.is_some(),
            cfg!(debug_assertions)
        );
        assert!(routes.match_nested("/flip-finder").0.is_none());
    }
}
