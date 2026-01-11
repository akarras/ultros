use crate::components::gil::Gil;
use crate::components::icon::Icon;
use crate::components::related_items::{
    calculate_crafting_cost, is_vendor_item, leve_rewards_item, recipe_tree_iter,
    special_shop_has_item,
};
use crate::components::skeleton::BoxSkeleton;
use crate::components::world_name::WorldName;
use crate::error::AppError;
use crate::global_state::cheapest_prices::CheapestPrices;
use icondata;
use leptos::prelude::*;
use ultros_api_types::CurrentlyShownItem;
use ultros_api_types::world_helper::AnySelector;
use xiv_gen::ItemId;

#[component]
pub fn SummaryCards(
    listing_resource: Resource<Result<CurrentlyShownItem, AppError>>,
    item_id: i32,
) -> impl IntoView {
    view! {
        <Transition fallback=move || view! { <BoxSkeleton /> }>
            {move || {
                let data_ref = listing_resource.get();
                if let Some(Ok(data)) = data_ref.as_ref() {
                    let cheapest_nq = data.listings
                        .iter()
                        .filter(|(l, _)| !l.hq)
                        .min_by_key(|(l, _)| l.price_per_unit)
                        .cloned();

                    let cheapest_hq = data.listings
                        .iter()
                        .filter(|(l, _)| l.hq)
                        .min_by_key(|(l, _)| l.price_per_unit)
                        .cloned();

                    let recent_sales = &data.sales;
                    let avg_price = if !recent_sales.is_empty() {
                        recent_sales.iter().map(|s| s.price_per_item as i64).sum::<i64>() / recent_sales.len() as i64
                    } else {
                        0
                    };
                    let listings_count = data.listings.len();
                    let has_nq = cheapest_nq.is_some();

                    // Re-evaluate logic inside the closure to avoid cloning AnyView
                    let non_market_card = {
                         let data = xiv_gen_db::data();
                         let cheapest_prices = use_context::<CheapestPrices>();

                         let vendor_exists = is_vendor_item(item_id);
                         let exchange_exists = data
                             .special_shops
                             .values()
                             .any(|s| special_shop_has_item(s, item_id));
                         let leve_exists = data.leves.values().any(|l| {
                             leve_rewards_item(
                                 l,
                                 item_id,
                                 &data.leve_reward_items,
                                 &data.leve_reward_item_groups,
                             )
                         });
                         let recipe_exists = recipe_tree_iter(ItemId(item_id)).next().is_some();

                         if vendor_exists || exchange_exists || leve_exists || recipe_exists {
                             let (title, summary, icon, href, color_class, border_color) = if vendor_exists {
                                 let price = data
                                     .items
                                     .get(&ItemId(item_id))
                                         .map(|i| if i.price_mid > 0 { i.price_mid } else { i.price_low })
                                     .unwrap_or(0);
                                 (
                                     "Vendor Available",
                                     view! { <span>"Sold for " <Gil amount=price as i32 /></span> }.into_any(),
                                     icondata::FaShopSolid,
                                     "#vendor-sources",
                                     "from-amber-900/20",
                                     "border-l-amber-500",
                                 )
                             } else if exchange_exists {
                                 (
                                     "Exchange Available",
                                     view! { <span>"Exchange for items/currency"</span> }.into_any(),
                                     icondata::BsArrowLeftRight,
                                     "#exchange-sources",
                                     "from-purple-900/20",
                                     "border-l-purple-500",
                                 )
                             } else if recipe_exists {
                                 let summary_view = view! {
                                     <Suspense fallback=move || "Craftable">
                                         {move || {
                                             if let Some(recipe) = recipe_tree_iter(ItemId(item_id)).next() {
                                                 if let Some(prices) = cheapest_prices.as_ref() {
                                                     prices.read_listings.with(|prices| {
                                                         let prices = prices.as_ref().and_then(|p| p.as_ref().ok());
                                                         if let Some(prices) = prices {
                                            let prices = prices.clone();
                                                             let (hq, lq) = calculate_crafting_cost(recipe, &prices);
                                                             let min_cost = if lq > 0 { lq } else { hq };
                                                             if min_cost > 0 {
                                                 // Determine phrasing based on if this item is a recipe result
                                                 // or just an ingredient.
                                                 // Actually, recipe_tree_iter returns recipes *related* to the item.
                                                 // It could be the item ITSELF (craftable), or it could be an ingredient.
                                                 // We only want to show "Craft for ~" if the item itself is the result.
                                                 if recipe.item_result.0 == item_id {
                                                     view! { <span>"Craft for ~" <Gil amount=min_cost /></span> }
                                                         .into_any()
                                                 } else {
                                                     "Used in Crafting".into_any()
                                                 }
                                             } else if recipe.item_result.0 == item_id {
                                                                 "Craftable".into_any()
                                             } else {
                                                 "Used in Crafting".into_any()
                                                             }
                                         } else if recipe.item_result.0 == item_id {
                                                             "Craftable".into_any()
                                         } else {
                                             "Used in Crafting".into_any()
                                                         }
                                                     })
                                                 } else {
                                                     "Craftable".into_any()
                                                 }
                                             } else {
                                                 "Craftable".into_any()
                                             }
                                         }}
                                     </Suspense>
                                 }
                                 .into_any();

                                 (
                                     "Crafting Recipe",
                                     summary_view,
                                     icondata::FaHammerSolid,
                                     "#crafting-recipes",
                                     "from-orange-900/20",
                                     "border-l-orange-500",
                                 )
                             } else {
                                 (
                                     "Levequest Reward",
                                     view! { "Obtainable via Levequest" }.into_any(),
                                     icondata::FaScrollSolid,
                                     "#leve-sources",
                                     "from-pink-900/20",
                                     "border-l-pink-500",
                                 )
                             };

                             Some(
                                 view! {
                                     <a
                                         href=href
                                         class=format!(
                                             "panel p-4 border-l-4 hover:scale-[1.02] transition-all cursor-pointer group bg-gradient-to-br to-transparent {} {}",
                                             border_color,
                                             color_class,
                                         )
                                     >
                                         <div class="flex justify-between items-start">
                                             <div>
                                                 <div class=format!(
                                                     "text-xs font-bold uppercase tracking-wider mb-2 {}",
                                                     border_color.replace("border-l-", "text-"),
                                                 )>
                                                     {title}
                                                 </div>
                                                 <div class="text-xl font-bold text-[color:var(--color-text)]">
                                                     {summary}
                                                 </div>
                                                 <div class="text-xs opacity-80 mt-1 text-[color:var(--color-text-muted)]">
                                                     "Click to view details"
                                                 </div>
                                             </div>
                                             <Icon
                                                 icon=icon
                                                 attr:class=format!(
                                                     "text-3xl opacity-20 group-hover:opacity-40 transition-opacity {}",
                                                     border_color.replace("border-l-", "text-"),
                                                 )
                                             />
                                         </div>
                                     </a>
                                 }
                                 .into_any(),
                             )
                         } else {
                             None
                         }
                    };

                    let grid_class = if non_market_card.is_some() {
                        "grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-4 mb-6"
                    } else {
                        "grid grid-cols-1 md:grid-cols-3 gap-4 mb-6"
                    };

                    view! {
                         <div class=grid_class>
                            // Card 1: Cheapest Found
                             <a href="#listings" class="panel p-4 border-l-4 border-l-brand-500 hover:scale-[1.02] transition-all cursor-pointer group bg-gradient-to-br from-brand-900/50 to-transparent">
                                 <div class="flex justify-between items-start">
                                     <div>
                                         <div class="text-xs font-bold text-brand-300 uppercase tracking-wider mb-2">"Cheapest Found"</div>
                                         <div class="flex flex-col gap-3">
                                             // NQ Display
                                             {if let Some((listing, _retainer)) = cheapest_nq {
                                                     view! {
                                                         <div>
                                                             <div class="flex items-center gap-2">
                                                                 <span class="text-xs font-bold text-brand-400 bg-brand-900/50 px-1.5 py-0.5 rounded border border-brand-700/50">"NQ"</span>
                                        <div class="text-xl font-bold text-[color:var(--color-text)]">
                                                                     <Gil amount=listing.price_per_unit />
                                                                 </div>
                                                             </div>
                                                         <div class="text-xs text-brand-200 mt-0.5 flex items-center gap-1 opacity-80">
                                                             <Icon icon=icondata::FaGlobeSolid attr:class="text-[10px]" />
                                                             <WorldName id=AnySelector::World(listing.world_id) />
                                                         </div>
                                                     </div>
                                                 }.into_any()
                                             } else {
                                                 // Don't show "No NQ" if HQ exists to avoid clutter, or maybe small text?
                                                 // If ONLY HQ exists, it will pop.
                                                 match cheapest_hq {
                                                    None => view! { <div class="text-lg text-gray-400 italic">"No listings"</div> }.into_any(),
                                                    _ => ().into_any()
                                                 }
                                             }}

                                             // HQ Display
                                             {if let Some((listing, _retainer)) = cheapest_hq {
                                                 view! {
                                                     <div class="relative">
                                                         // Add a separator if NQ also exists
                                                         <Show when=move || has_nq>
                                                             <div class="absolute -top-1.5 left-0 w-8 border-t border-brand-700/30"></div>
                                                         </Show>
                                                         <div class="flex items-center gap-2">
                                                             <span class="text-xs font-bold text-[#95c521] bg-[#95c521]/10 px-1.5 py-0.5 rounded border border-[#95c521]/20 flex items-center gap-1">
                                                                 <Icon icon=icondata::FaStarSolid attr:class="text-[9px]" />
                                                                 "HQ"
                                                             </span>
                                        <div class="text-xl font-bold text-[color:var(--color-text)]">
                                                                 <Gil amount=listing.price_per_unit />
                                                             </div>
                                                         </div>
                                                         <div class="text-xs text-brand-200 mt-0.5 flex items-center gap-1 opacity-80">
                                                             <Icon icon=icondata::FaGlobeSolid attr:class="text-[10px]" />
                                                             <WorldName id=AnySelector::World(listing.world_id) />
                                                         </div>
                                                     </div>
                                                 }.into_any()
                                             } else {
                                                 ().into_any()
                                             }}
                                         </div>
                                     </div>
                                     <Icon icon=icondata::FaCoinsSolid attr:class="text-3xl text-brand-500/20 group-hover:text-brand-500/40 transition-colors" />
                                 </div>
                             </a>

                            // Card 2: Recent History
                            <a href="#history" class="panel p-4 border-l-4 border-l-blue-500 hover:scale-[1.02] transition-all cursor-pointer group bg-gradient-to-br from-blue-900/20 to-transparent">
                                 <div class="flex justify-between items-start">
                                     <div>
                                    <div class="text-xs font-bold text-blue-700 dark:text-blue-300 uppercase tracking-wider mb-1">"Recent Average"</div>
                                    <div class="text-2xl font-bold text-[color:var(--color-text)]">
                                            {if avg_price > 0 {
                                                view! { <Gil amount=avg_price as i32 /> }.into_any()
                                            } else {
                                                view! { <span class="text-gray-400">"No Data"</span> }.into_any()
                                            }}
                                         </div>
                                         <div class="text-sm text-blue-700 dark:text-blue-200 mt-1">
                                             {format!("Based on {} sales", recent_sales.len())}
                                         </div>
                                         <div class="text-sm text-blue-700 dark:text-blue-200 mt-1">
                                             {
                                                 if recent_sales.len() > 1 {
                                                     let newest = recent_sales.first().unwrap().sold_date;
                                                     let oldest = recent_sales.last().unwrap().sold_date;
                                                     let seconds = (newest - oldest).num_seconds().abs();
                                                     let count = recent_sales.len() - 1;

                                                     if seconds > 0 {
                                                         let seconds_per_sale = seconds as f64 / count as f64;
                                                         if seconds_per_sale < 60.0 {
                                                             format!("Sells ~{:.1} times per minute", 60.0 / seconds_per_sale)
                                                         } else if seconds_per_sale < 3600.0 {
                                                             format!("Sells ~{:.1} times per hour", 3600.0 / seconds_per_sale)
                                                         } else if seconds_per_sale < 86400.0 {
                                                             format!("Sells ~{:.1} times per day", 86400.0 / seconds_per_sale)
                                                         } else {
                                                             format!("Sells ~1 every {:.1} days", seconds_per_sale / 86400.0)
                                                         }
                                                     } else {
                                                         "Very high frequency".to_string()
                                                     }
                                                 } else {
                                                     "Not enough data".to_string()
                                                 }
                                             }
                                         </div>
                                     </div>
                                     <Icon icon=icondata::FaChartLineSolid attr:class="text-3xl text-blue-500/20 group-hover:text-blue-500/40 transition-colors" />
                                 </div>
                            </a>

                            // Card 3: Active Listings
                            <a href="#listings" class="panel p-4 border-l-4 border-l-emerald-500 hover:scale-[1.02] transition-all cursor-pointer group bg-gradient-to-br from-emerald-900/20 to-transparent">
                                 <div class="flex justify-between items-start">
                                     <div>
                                    <div class="text-xs font-bold text-emerald-700 dark:text-emerald-300 uppercase tracking-wider mb-1">"Active Listings"</div>
                                    <div class="text-2xl font-bold text-[color:var(--color-text)]">
                                             {listings_count}
                                         </div>
                                    <div class="text-sm text-emerald-700 dark:text-emerald-200 mt-1">
                                             "Available now"
                                         </div>
                                     </div>
                                     <Icon icon=icondata::FaListSolid attr:class="text-3xl text-emerald-500/20 group-hover:text-emerald-500/40 transition-colors" />
                                 </div>
                            </a>
                            {non_market_card}
                         </div>
                    }.into_any()
                } else {
                    ().into_any()
                }
            }}
        </Transition>
    }.into_any()
}
