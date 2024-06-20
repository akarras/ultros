use crate::{
    components::{cheapest_price::*, item_icon::*},
    global_state::home_world::get_price_zone,
};
use leptos::*;
use sublime_fuzzy::{best_match, Match};
use xiv_gen::ItemId;

/// Leptos version of sublime_fuzzy::format_simple
#[component]
pub fn MatchFormatter(m: Match, target: String) -> impl IntoView {
    let mut pieces = Vec::new();

    let mut last_end = 0;

    for c in m.continuous_matches() {
        // Piece between last match and this match
        pieces.push(
            view! { {target.chars().skip(last_end).take(c.start() - last_end).collect::<String>()} }
            .into_view(),
        );

        // This match
        pieces.push(
            view! { <b>{target.chars().skip(c.start()).take(c.len()).collect::<String>()}</b> }
                .into_view(),
        );

        last_end = c.start() + c.len();
    }

    // Leftover chars
    if last_end != target.len() {
        pieces.push(
            target
                .chars()
                .skip(last_end)
                .collect::<String>()
                .into_view(),
        );
    }

    pieces
}

#[component]
pub fn ItemSearchResult(
    item_id: i32,
    search: ReadSignal<String>,
    set_search: WriteSignal<String>,
    // matches: Match,
) -> impl IntoView {
    let data = xiv_gen_db::data();
    let categories = &data.item_ui_categorys;
    let items = &data.items;
    let item = items.get(&ItemId(item_id));
    let (price_zone, _) = get_price_zone();
    view! {
        {if let Some(item) = item {
            view! {
                <a
                    on:click=move |_| set_search("".to_string())
                    href=move || {
                        let zone = price_zone();
                        let price_zone = zone
                            .as_ref()
                            .map(|z| z.get_name())
                            .unwrap_or("North-America");
                        format!("/item/{price_zone}/{item_id}")
                    }
                >
                    // this needs to be updated to be able to point to any region
                    <div class="search-result">
                        <ItemIcon item_id icon_size=IconSize::Small/>
                        <div class="search-result-details">
                            <div class="flex-row flex-space" style="height: 20px; overflow: clip">
                                <span class="item-name">
                                    {move || {
                                        let item_name = items
                                            .get(&ItemId(item_id))
                                            .as_ref()
                                            .map(|item| item.name.as_str())
                                            .unwrap_or_default();
                                        if let Some(m) = best_match(&search(), item_name) {
                                            view! { <MatchFormatter m target=item_name.to_string()/> }
                                                .into_view()
                                        } else {
                                            item_name.into_view()
                                        }
                                    }}

                                </span>
                                <div
                                    class="flex-row"
                                    style="align-items: start; flex-wrap: nowrap; justify-content:end;"
                                >
                                    <CheapestPrice item_id=item.key_id/>
                                </div>
                            </div>
                            <div class="flex-row flex-space" style="height: 20px; overflow: clip">
                                <span class="item-type">
                                    {categories
                                        .get(&item.item_ui_category)
                                        .map(|i| i.name.as_str())
                                        .unwrap_or_default()}
                                </span>

                                {(item.level_item.0 != 0)
                                    .then(|| {
                                        view! {
                                            <span class="item-ilvl">"ILVL " {item.level_item.0}</span>
                                        }
                                    })}

                            </div>
                        </div>
                    </div>
                </a>
            }
        } else {
            view! {
                // this needs to be updated to be able to point to any region

                <a class="search-result">"Invalid result"</a>
            }
        }}
    }
}
