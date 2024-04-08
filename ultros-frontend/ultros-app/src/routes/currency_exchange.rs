use std::cmp::Ordering;
use std::collections::HashMap;

use crate::api::get_cheapest_listings;
use crate::api::get_recent_sales_for_world;
use crate::components::add_to_list::AddToList;
use crate::components::item_icon::ItemIcon;
use crate::components::loading::Loading;
use crate::components::meta::MetaDescription;
use crate::components::meta::MetaTitle;
use crate::components::modal::Modal;
use crate::components::number_input::ParseableInputBox;
use crate::components::query_button::QueryButton;
use crate::error::AppError;
use crate::global_state::home_world::use_home_world;
use crate::Ad;
use crate::A;
use chrono::NaiveDateTime;
use chrono::Utc;
use field_iterator::field_iter;
use field_iterator::FieldLabels;
use field_iterator::SortableVec;
use itertools::Itertools;
use leptos::*;
use leptos_icons::Icon;
use leptos_router::create_query_signal;
use leptos_router::use_params_map;
use leptos_router::use_query_map;
use leptos_router::Outlet;

use leptos_router::ParamsMap;
use ultros_api_types::cheapest_listings::CheapestListingItem;
use ultros_api_types::icon_size::IconSize;
use ultros_api_types::recent_sales::SaleData;
use xiv_gen::Item;
use xiv_gen::{ItemId, SpecialShop};

#[derive(Copy, Clone, PartialEq, Debug)]
struct ItemAmount {
    item: &'static Item,
    amount: u32,
}

impl PartialOrd for ItemAmount {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        match self.item.name.partial_cmp(&other.item.name) {
            Some(core::cmp::Ordering::Equal) => {}
            ord => return ord,
        }
        self.amount.partial_cmp(&other.amount)
    }
}

impl Eq for ItemAmount {}

impl Ord for ItemAmount {
    fn cmp(&self, other: &Self) -> Ordering {
        self.item
            .name
            .cmp(&other.item.name)
            .then_with(|| self.amount.cmp(&other.amount))
    }
}

impl IntoView for ItemAmount {
    fn into_view(self) -> View {
        view !{
            <A class="flex flex-row" href=format!("/item/{}", self.item.key_id.0)>
                <ItemIcon item_id=self.item.key_id.0 icon_size=IconSize::Small /><span>{self.item.name.as_str()}</span>"x"{self.amount}
            </A>
        }.into_view()
    }
}

struct ShopItems {
    recv: Vec<ItemAmount>,
    cost: Vec<ItemAmount>,
}

fn from_lists(
    item: impl Iterator<Item = ItemId>,
    amount: impl Iterator<Item = u32>,
) -> impl Iterator<Item = Option<ItemAmount>> {
    let items = &xiv_gen_db::data().items;
    item.zip(amount).map(|(item_id, amount)| {
        let item = items.get(&item_id)?;
        Some(ItemAmount { item, amount })
    })
}

fn shop_items(special_shop: &SpecialShop) -> impl Iterator<Item = ShopItems> + '_ {
    let SpecialShop {
        item_receive_0,
        count_receive_0,
        item_receive_1,
        count_receive_1,
        item_cost_0,
        count_cost_0,
        item_cost_1,
        count_cost_1,
        item_cost_2,
        count_cost_2,
        ..
    } = special_shop;

    let recv_0 = from_lists(
        item_receive_0.iter().copied(),
        count_receive_0.iter().copied(),
    );
    let recv_1 = from_lists(
        item_receive_1.iter().copied(),
        count_receive_1.iter().copied(),
    );
    let cost_0 = from_lists(item_cost_0.iter().copied(), count_cost_0.iter().copied());
    let cost_1 = from_lists(item_cost_1.iter().copied(), count_cost_1.iter().copied());
    let cost_2 = from_lists(item_cost_2.iter().copied(), count_cost_2.iter().copied());
    recv_0
        .zip(recv_1)
        .zip(
            cost_0
                .zip(cost_1.zip(cost_2))
                .map(|(cost_0, (cost_1, cost_2))| (cost_0, cost_1, cost_2)),
        )
        .map(|((recv_0, recv_1), (cost_0, cost_1, cost_2))| ShopItems {
            recv: [recv_0, recv_1].into_iter().flatten().collect(),
            cost: [cost_0, cost_1, cost_2].into_iter().flatten().collect(),
        })
}

#[component]
fn FilterModal(filter_name: &'static str) -> impl IntoView {
    let (is_open, set_open) = create_signal(false);
    view! {
        <div on:click=move |_| set_open(true)>
            <div><Icon icon=icondata::AiFilterFilled /></div>
            {move || is_open().then(|| {
                let (min, set_min) = create_query_signal::<i32>(format!("{filter_name}_min"));
                let (max, set_max) = create_query_signal::<i32>(format!("{filter_name}_max"));
                view ! {
                    <Modal set_visible=set_open>
                        <h3 class="text-2xl font-bold">"Edit filter"</h3>
                        {filter_name.replace("_", " ")}
                        <div class="flex flex-row justify-between">
                            <span>"Max"</span>
                            <ParseableInputBox input=Signal::derive(move || { max().unwrap_or(i32::MAX) }) set_value=SignalSetter::map(move |value| set_max(Some(value))) />
                        </div>
                        <div class="flex flex-row justify-between">
                            <span>"Min"</span>
                            <ParseableInputBox input=Signal::derive(move || { min().unwrap_or(i32::MIN) }) set_value=SignalSetter::map(move |value| set_min(Some(value))) />
                        </div>
                    </Modal>
                }
            })}
        </div>
    }
}

fn is_in_range(value: i32, field_label: &str, query_map: &ParamsMap) -> bool {
    let max = query_map
        .get(&format!("{field_label}_max"))
        .and_then(|p| p.parse::<i32>().ok());
    let min = query_map
        .get(&format!("{field_label}_min"))
        .and_then(|p| p.parse::<i32>().ok());

    match (min, max) {
        (None, None) => true,
        (None, Some(max)) => value < max,
        (Some(min), None) => value > min,
        (Some(min), Some(max)) => (min..max).contains(&value),
    }
}

#[component]
pub fn ExchangeItem() -> impl IntoView {
    let params = use_params_map();
    let query = use_query_map();
    let (home_world, _) = use_home_world();
    let (currency_quantity, set_currency_quantity) = create_query_signal::<i32>("currency_amount");
    let sales = create_resource(home_world, move |world| async move {
        let world = world.ok_or(AppError::NoHomeWorld)?;
        get_recent_sales_for_world(&world.name).await
    });

    let world_cheapest_listings = create_resource(home_world, move |world| async move {
        let world = world.ok_or(AppError::NoHomeWorld)?;
        get_cheapest_listings(&world.name).await
    });
    let data = xiv_gen_db::data();
    let item_id = move || {
        ItemId(
            params
                .get()
                .get("id")
                .and_then(|p| p.parse::<i32>().ok())
                .unwrap_or_default(),
        )
    };
    let item = move || data.items.get(&item_id());
    let currency_quantity = create_memo(move |_| {
        if let Some(quantity) = currency_quantity() {
            return quantity;
        }
        let Some(item) = item() else {
            return 0;
        };
        item.stack_size as i32
    });
    let shop_data = move || {
        let item = item_id();
        data.special_shops
            .values()
            .flat_map(move |shop| {
                shop_items(shop)
                    .filter(move |items| {
                        // make sure the item is valid on the marketboard before we lookup prices for it
                        items.cost.iter().any(|i| i.item.key_id.0 == item.0)
                            && items
                                .recv
                                .iter()
                                .any(|i| i.item.item_search_category.0 != 0)
                    })
                    .map(move |items| (items, shop))
            })
            .collect::<Vec<_>>()
    };
    let with_prices = move || {
        let current_quantity = currency_quantity.get();
        let sales: HashMap<(bool, i32), SaleData> = sales()?
            .ok()?
            .sales
            .into_iter()
            .map(|sale| ((sale.hq, sale.item_id), sale))
            .collect();
        let world_listings: HashMap<(bool, i32), CheapestListingItem> = world_cheapest_listings()?
            .ok()?
            .cheapest_listings
            .into_iter()
            .map(|cheapest| ((cheapest.hq, cheapest.item_id), cheapest))
            .collect();
        let shops_with_item = shop_data();
        let now = Utc::now().naive_utc();
        let rows = shops_with_item
            .iter()
            .filter_map(|(item, shop)| {
                // going to just assume first item matters?
                let recv = item.recv[0];
                let cost = item.cost[0];
                let item_key = (false, recv.item.key_id.0);
                let sales = &sales.get(&item_key)?.sales;
                let sale = sales.first()?.price_per_unit;
                let current_listing_price = world_listings
                    .get(&item_key)
                    .map(|listing| listing.cheapest_price - 1);
                let guessed_price_per_item = current_listing_price.unwrap_or(sale).min(sale);
                let input_amount = current_quantity;
                let number_received = recv.amount as i32 * (input_amount / cost.amount as i32);
                let sales_len = sales.len();
                let hours_between_sales = sales
                    .last()
                    .map(|last| {
                        let time_between = (now - last.sale_date) / sales_len as i32;
                        time_between.num_hours() as i16
                    })
                    .unwrap_or(i16::MAX);
                Some(CurrencyTrade {
                    shop_name: shop.name.to_string(),
                    cost_item: Some(cost),
                    receive_item: Some(recv),
                    price_per_item: guessed_price_per_item,
                    number_received,
                    total_profit: guessed_price_per_item as i64 * number_received as i64,
                    hours_between_sales,
                })
            })
            .collect::<Vec<_>>();
        Some(rows)
    };

    let (sorted_by, _set_sorted_by) = create_query_signal::<String>("sorted-by");
    let item_name = move || item().map(|i| i.name.as_str()).unwrap_or_default();
    view! {
        <div>
            <MetaTitle title={move || format!("Currency Exchange - {}", item_name())} />
            <MetaDescription text=move || format!("All possible items that can be exchanged for {} with how much you can potentially earn", item_name()) />
            <div class="flex flex-col">
                <div>{move || item().map(|i| &i.name)}" - Currency Exchange"</div>
                <div class="flex flex-row gap-1">
                    "Amount in:"
                    <input prop:value=currency_quantity on:input=move |e| {
                        let event = event_target_value(&e);
                        if let Ok(p) = event.parse() {
                            set_currency_quantity.set(Some(p));
                        }
                    } />
                </div>
            </div>
            <div class="flex flex-col">
                {move || {
                    if home_world().is_none() {
                        view! {
                            <div>
                                "Home world is not set, go to the "<A href="/settings">"settings"</A>" page and set your home world to see prices on this page"
                            </div>
                        }.into_view()
                    } else {
                        view! {
                            <Suspense fallback=Loading>
                    {move || {
                        let sort_label = sorted_by();
                        with_prices().map(|p: Vec<CurrencyTrade>| {
                            let sorted_and_filtered_rows = move || {
                                let query = query();
                                let mut p = p.clone().into_iter().filter(|currency| {
                                    let query = &query;
                                    is_in_range(currency.price_per_item as i32, "price_per_item", query)
                                    && is_in_range(currency.number_received as i32, "number_received", query)
                                    && is_in_range(currency.total_profit as i32, "total_profit", query)
                                    && is_in_range(currency.hours_between_sales as i32, "hours_between_sales", query)

                                }).collect::<Vec<_>>();
                                CurrencyTrade::sort_vec_by_label(&mut p, sort_label.as_deref().unwrap_or("total_profit"), None);
                                p.into_iter().map(|p| view!{
                                    <tr>
                                        <td>{p.shop_name}</td>
                                        <td>{p.cost_item}</td>
                                        <td>{p.receive_item}</td>
                                        <td>{p.price_per_item}</td>
                                        <td>{p.number_received}</td>
                                        <td>{p.total_profit}</td>
                                        <td>{p.hours_between_sales}</td>
                                        <td><AddToList item_id=p.receive_item.map(|i| i.item.key_id.0).unwrap_or_default() /></td>
                                    </tr>
                                }).collect::<Vec<_>>()

                             };
                            let labels = CurrencyTrade::field_labels();


                            view! {
                                <table>
                                    <thead>
                                    {labels
                                        .into_iter()
                                        .enumerate()
                                        .map(|(i, l)| view!{
                                            <th class="uppercase">
                                                <div class="flex flex-row gap-1">
                                                    <QueryButton query_name="sorted-by" value={l.to_string()} class="font-bold"
                                                    active_classes="font-bold underline" default={"total_profit" == *l}>
                                                        {l.replace("_", " ")}
                                                    </QueryButton>
                                                    {(i > 2).then(|| view!{ <FilterModal filter_name=l /> })}
                                                </div>
                                            </th>}).collect::<Vec<_>>()}
                                            <th>"Add to list"</th>
                                    </thead>
                                    <tbody>
                                        {sorted_and_filtered_rows}
                                    </tbody>
                                </table>
                            }
                        })
                    }}
                    {move || {
                        sales.with(|sales| if let Some(Err(_e)) = sales {
                            view!{ "Error loading, try again in 30 seconds!"}.into_view()
                        } else {
                            ().into_view()
                        })
                    }}
                </Suspense>
                        }
                    }
                }}

            </div>
        </div>
    }
}

#[field_iter(field_prefix = "item_cost_", count = 3)]
fn item_cost_iter(shop: &SpecialShop) -> impl Iterator<Item = ItemId> + '_ {}

// #[derive(TableRow, Clone, Default, Debug)]
// #[table(
//     impl_vec_data_provider,
//     sortable,
//     classes_provider = "TailwindClassesPreset"
// )]
#[derive(SortableVec, FieldLabels, Clone)]
pub struct CurrencyTrade {
    shop_name: String,
    cost_item: Option<ItemAmount>,
    receive_item: Option<ItemAmount>,
    price_per_item: i32,
    number_received: i32,
    total_profit: i64,
    hours_between_sales: i16,
}

#[component]
pub fn CurrencySelection() -> impl IntoView {
    let data = xiv_gen_db::data();
    let ui_categories = &data.item_ui_categorys;
    let allowed_item_ui_categories = ["Currency", "Miscellany", "Other"]
        .into_iter()
        .map(|category| {
            ui_categories
                .iter()
                .find(|f| f.1.name == category)
                .map(|(id, _)| *id)
                .unwrap()
        })
        .collect::<Vec<_>>();
    let currencies = data
        .special_shops
        .iter()
        .flat_map(|(_shops, special_shop)| {
            shop_items(special_shop)
                .filter(|items| {
                    items
                        .recv
                        .iter()
                        .any(|i| i.item.item_search_category.0 != 0)
                })
                .flat_map(|f| f.cost.into_iter().map(|i| i.item.key_id))
        })
        .filter(|f| {
            let Some(item) = data.items.get(f) else {
                return false;
            };
            allowed_item_ui_categories.contains(&item.item_ui_category)
        })
        .unique_by(|i| i.0)
        .collect::<Vec<_>>();
    let items = &data.items;
    view! {
        <div class="container mx-auto gap-1 flex flex-col">
        {
            currencies
                .into_iter()
                .sorted_by_key(|item| item.0)
                .filter_map(|c| {
                    let item = items.get(&c)?;
                    let ui_category = item.item_ui_category;
                    let category = ui_categories.get(&ui_category);
                    let category_name = category
                        .as_ref()
                        .map(|ui| ui.name.as_str())
                        .unwrap_or_default();
                    Some(view! {
                    <A href={item.key_id.0.to_string()} class="flex flex-col p-1 border border-fuchsia-950 rounded-xl">
                        <div class="text-xl font-bold text-white hover:text-gray-50">{item.name.to_string()}</div>
                        <div class="italic text-white hover:text-gray-50">{category_name}</div>
                    </A>})
                })
                .collect::<Vec<_>>()
        }
        </div>
    }
}

#[component]
pub fn CurrencyExchange() -> impl IntoView {
    view! {
        <A href="/currency-exchange"><h3 class="text-2xl font-bold text-white">"Currency Exchange"</h3></A>
        <Ad class="w-full h-[100px]"/>
        <div class="main-content">
            <Outlet />

        </div>
    }
}
