use crate::item_icon::*;
use leptos::*;
use sublime_fuzzy::Match;
use xiv_gen::ItemId;

/// Leptos version of sublime_fuzzy::format_simple
#[component]
fn MatchFormatter(cx: Scope, m: Match, target: String) -> impl IntoView {
    let mut pieces = Vec::new();

    let mut last_end = 0;

    for c in m.continuous_matches() {
        // Piece between last match and this match
        pieces.push(
            view! {cx,
            {target
                .chars()
                .skip(last_end)
                .take(c.start() - last_end)
                .collect::<String>()}}
            .into_view(cx),
        );

        // This match
        pieces.push(
            view! {cx, <b>{target.chars().skip(c.start()).take(c.len()).collect::<String>()}</b>}
                .into_view(cx),
        );

        last_end = c.start() + c.len();
    }

    // Leftover chars
    if last_end != target.len() {
        pieces.push(view! {cx, {target.chars().skip(last_end).collect::<String>()}}.into_view(cx));
    }

    view! {cx, {pieces}}
}

#[component]
pub fn ItemSearchResult(
    cx: Scope,
    item_id: i32,
    set_search: WriteSignal<String>,
    matches: Match,
) -> impl IntoView {
    let data = xiv_gen_db::decompress_data();
    let categories = &data.item_ui_categorys;
    let items = &data.items;
    let item = items.get(&ItemId(item_id));
    view! {
        cx,
        {if let Some(item) = item {
            view!{cx,
            <html::a on:click=move |_| set_search("".to_string()) href=format!("/listings/North-America/{item_id}")> // this needs to be updated to be able to point to any region
                <div class="search-result">
                    <ItemIcon item_id icon_size=IconSize::Small />
                    <div class="search-result-details">
                        <span class="item-name"><MatchFormatter m=matches target=item.name.to_string() /></span>
                        <div class="flex-row flex-space">
                            <span class="item-type">{categories.get(&item.item_ui_category).map(|i| i.name.as_str()).unwrap_or_default()}</span>
                            <span class="item-ilvl">"ITEM LEVEL " {item.level_item.0}</span>
                        </div>
                    </div>
                </div>
            </html::a>
    }
        } else {
            view!{cx, <html::a class="search-result">"Invalid result"</html::a>}
        }}
    }
}
