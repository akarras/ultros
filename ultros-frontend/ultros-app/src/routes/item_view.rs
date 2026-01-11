use crate::components::add_to_list::AddToList;
use crate::components::clipboard::*;
use crate::components::item_icon::*;
use crate::components::item_view::listings::*;
use crate::components::item_view::world_navigation::*;
use crate::components::meta::*;
use crate::components::recently_viewed::RecentItems;
use crate::components::related_items::*;
use crate::components::stats_display::*;
use crate::components::ui_text::*;
use crate::global_state::home_world::get_price_zone;
use leptos::prelude::*;
use leptos_meta::{Link, Meta};
use leptos_router::hooks::use_params_map;
use xiv_gen::ItemId;

#[component]
pub fn ItemView() -> impl IntoView {
    let params = use_params_map();
    let item_id = Memo::new(move |_| {
        params()
            .get("id")
            .and_then(|id| id.parse::<i32>().ok())
            .unwrap_or_default()
    });

    let recently_viewed = use_context::<RecentItems>().unwrap();
    Effect::new(move |_| {
        recently_viewed.add_item(item_id());
    });

    let data = &xiv_gen_db::data();
    let items = &data.items;
    let categories = &data.item_ui_categorys;
    let search_categories = &data.item_search_categorys;
    let (price_zone, _) = get_price_zone();

    let world = Memo::new(move |_| {
        params.with(|p| {
            p.get("world").clone().unwrap_or_else(move || {
                price_zone
                    .get()
                    .map(|zone| zone.get_name().to_string())
                    .unwrap_or_else(|| "North-America".to_string())
            })
        })
    });

    let item_name = move || {
        items
            .get(&ItemId(item_id()))
            .map(|item| item.name.as_str())
            .unwrap_or_default()
    };

    let item = move || items.get(&ItemId(item_id()));

    let item_description = move || {
        items
            .get(&ItemId(item_id()))
            .map(|item| item.description.as_str())
            .unwrap_or_default()
    };

    let item_category = move || {
        items
            .get(&ItemId(item_id()))
            .and_then(|item| categories.get(&item.item_ui_category))
    };

    let item_search_category = move || {
        items
            .get(&ItemId(item_id()))
            .and_then(|item| search_categories.get(&item.item_search_category))
    };

    let description = Memo::new(move |_| {
        format!(
            "Current market board listings for {} within {}. Find the lowest prices in your region.",
            item_name(),
            world(),
        )
    });

    view! {
        <MetaTitle title=move || {
            format!("{} - 🌍{} - Market board - Ultros", item_name(), world())
        } />
        <MetaDescription text=description />
        <MetaImage url=move || format!("https://ultros.app/itemcard/{}/{}", world(), item_id()) />
        <Meta
            property="thumbnail"
            content=move || format!("https://ultros.app/static/itemicon/{}?size=Large", item_id())
        />
        <Link rel="canonical" prop:href=move || format!("https://ultros.app/item/{}", item_id()) />
        <div class="min-h-screen">
            <div class="w-full px-0 sm:px-4 py-4 sm:py-6">
                <div class="flex flex-col gap-6 p-4 sm:p-6 panel">
                    <div class="flex flex-col md:flex-row items-start gap-4">
                        <div class="flex items-center gap-4 flex-1">
                            <ItemIcon item_id icon_size=IconSize::Large />
                            <div class="flex flex-col">
                                <h1 class="text-3xl font-bold text-[color:var(--color-text)] flex items-center gap-2">
                                    {item_name}
                                    <Clipboard clipboard_text=Signal::derive(move || {
                                        item_name().to_string()
                                    }) />
                                </h1>
                                <div class="text-brand-300 text-lg">
                                    {move || {
                                        item_category()
                                            .and_then(|c| item_search_category().map(|s| (c, s)))
                                            .map(|(c, s)| {
                                                view! {
                                                    <a
                                                        class="text-brand-300 hover:text-brand-200 transition-colors"
                                                        href=["/items/category/", &s.name.replace("/", "%2F")]
                                                            .concat()
                                                    >
                                                        {c.name.as_str()}
                                                    </a>
                                                }
                                            })
                                    }}
                                </div>
                            </div>
                        </div>

                        <div class="flex flex-wrap gap-2 items-center">
                            <div class="cursor-pointer"><AddToList item_id /></div>
                            <a
                                class="btn-primary"
                                target="_blank"
                                rel="noopener noreferrer"
                                aria-label="Open Universalis market page in a new tab"
                                href=move || format!("https://universalis.app/market/{}", item_id())
                            >
                                "Universalis"
                            </a>
                            <a
                                class="btn-primary"
                                target="_blank"
                                rel="noopener noreferrer"
                                aria-label="Open Garlandtools item page in a new tab"
                                href=move || format!("https://garlandtools.org/db/#item/{}", item_id())
                            >
                                "Garlandtools"
                            </a>
                        </div>
                    </div>

                    // Moved Description and Item Level here
                    <div class="space-y-3 pt-4 border-t border-[color:var(--color-outline)] text-[color:var(--color-text)]/90">
                        <div class="flex items-center gap-2">
                            <span class="text-brand-300 font-medium tracking-wide text-sm uppercase">Item Level</span>
                            <span class="bg-brand-900/40 text-brand-100 px-2 py-0.5 rounded text-sm font-bold border border-brand-700/50">
                                {move || item().map(|item| item.level_item.0).unwrap_or_default()}
                            </span>
                            <div class="flex-grow"></div>
                             <div>{move || view! { <ItemStats item_id=ItemId(item_id()) /> }}</div>
                        </div>
                        <div
                            class=""
                            class:hidden=move || { item_description().is_empty() }
                        >
                            {move || view! { <UIText text=item_description().to_string() /> }}
                        </div>
                    </div>
                </div>
            </div>

            <WorldMenu world_name=world item_id />

            <div class="main-content px-0 sm:px-4">
                <ListingsContent item_id world />
                <div class="mt-6">
                    <RelatedItems item_id=Signal::from(item_id) />
                </div>
            </div>
        </div>
    }.into_any()
}
