use crate::{
    components::{cheapest_price::*, item_icon::*},
    global_state::home_world::get_price_zone,
};
use leptos::{either::Either, prelude::*};
use sublime_fuzzy::{best_match, Match};
use xiv_gen::ItemId;

/// Leptos version of sublime_fuzzy::format_simple
#[component]
pub fn MatchFormatter(m: Match, target: String) -> impl IntoView {
    let mut pieces = Vec::new();
    let mut last_end = 0;

    for c in m.continuous_matches() {
        // Piece between last match and this match
        pieces.push(Either::Left(
            target
                .chars()
                .skip(last_end)
                .take(c.start() - last_end)
                .collect::<String>(),
        ));

        // This match
        pieces.push(Either::Right(view! {
            <span class="font-medium text-[color:var(--brand-fg)]">
                {target.chars().skip(c.start()).take(c.len()).collect::<String>()}
            </span>
        }));

        last_end = c.start() + c.len();
    }

    // Leftover chars
    if last_end != target.len() {
        pieces.push(Either::Left(
            target.chars().skip(last_end).collect::<String>(),
        ));
    }

    pieces
}

#[component]
pub fn ItemSearchResult(
    item_id: i32,
    search: ReadSignal<String>,
    set_search: WriteSignal<String>,
) -> impl IntoView {
    let data = xiv_gen_db::data();
    let categories = &data.item_ui_categorys;
    let items = &data.items;
    let item = items.get(&ItemId(item_id));
    let (price_zone, _) = get_price_zone();

    if let Some(item) = item {
        let left_view = view! {
            <a
                class="w-full"
                on:click=move |_| set_search("".to_string())
                href=move || {
                    let zone = price_zone();
                    let price_zone = zone.as_ref().map(|z| z.get_name()).unwrap_or("North-America");
                    format!("/item/{price_zone}/{item_id}")
                }
            >
                <div class="flex flex-row items-center px-3 py-2 hover:bg-[color:color-mix(in_srgb,var(--brand-ring)_14%,transparent)]
                transition-colors duration-200 group gap-3 w-full">
                    <div class="flex-shrink-0">
                        <ItemIcon item_id icon_size=IconSize::Small />
                    </div>

                    <div class="flex flex-col min-w-0 flex-1">
                        <div class="flex items-center gap-2">
                            <span class="text-[color:var(--color-text)] font-medium truncate">
                                {move || {
                                    let item_name = items
                                        .get(&ItemId(item_id))
                                        .as_ref()
                                        .map(|item| item.name.as_str())
                                        .unwrap_or_default();
                                    if let Some(m) = best_match(&search(), item_name) {
                                        Either::Left(
                                            view! { <MatchFormatter m target=item_name.to_string() /> },
                                        )
                                    } else {
                                        Either::Right(item_name)
                                    }
                                }}
                            </span>
                        </div>

                        <div class="flex items-center justify-between text-sm">
                            <span class="text-[color:var(--color-text-muted)] truncate">
                                {categories
                                    .get(&item.item_ui_category)
                                    .map(|i| i.name.as_str())
                                    .unwrap_or_default()}
                            </span>

                            {(item.level_item.0 != 0)
                                .then(|| {
                                    view! {
                                        <span class="text-[color:var(--color-text-muted)]">
                                            "iLvl " {item.level_item.0}
                                        </span>
                                    }
                                })}
                        </div>
                    </div>

                    <div class="flex flex-shrink-0 pl-4 text-right min-w-[100px]
                    text-[color:var(--brand-fg)] font-medium">
                        <CheapestPrice item_id=item.key_id />
                    </div>
                </div>
            </a>
        };
        Either::Left(left_view)
    } else {
        Either::Right(view! {
            <a class="block px-3 py-2 text-[color:var(--color-text-muted)] text-center hover:bg-[color:color-mix(in_srgb,_var(--brand-ring)_12%,_transparent)] transition-colors">
                "Invalid result"
            </a>
        })
    }.into_any()
}
