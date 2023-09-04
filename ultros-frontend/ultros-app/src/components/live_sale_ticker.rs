use super::gil::*;
use super::item_icon::*;
use super::loading::*;
use super::relative_time::RelativeToNow;
use std::collections::VecDeque;

use leptos::*;
use ultros_api_types::SaleHistory;
use ultros_api_types::UnknownCharacter;
use xiv_gen::ItemId;

use crate::global_state::home_world::get_homeworld;
#[cfg(not(feature = "ssr"))]
use crate::ws::live_data::live_sales;

#[component]
pub fn LiveSaleTicker() -> impl IntoView {
    let sales = create_rw_signal::<VecDeque<(SaleHistory, UnknownCharacter)>>(VecDeque::new());
    let (homeworld, _) = get_homeworld();
    create_effect(move |_| {
        #[cfg(not(feature = "ssr"))]
        let hw = homeworld();
        spawn_local(async move {
            #[cfg(not(feature = "ssr"))]
            if let Some(sale) = hw.map(|h| ultros_api_types::world_helper::AnySelector::World(h.id))
            {
                log::info!("live sale");
                live_sales(sales, sale).await.unwrap();
            }
        });
    });

    let items = &xiv_gen_db::data().items;
    view! {
        <Suspense fallback=move || view!{<Loading />}>
            {move ||{
                view!{
                    <div class="content-well">
                        <div class="text-xl">{move || format!("Sales on {}", homeworld().map(|world| world.name).unwrap_or_default())}</div>
                        <div class="flex flex-row-reverse flex-nowrap h-28 gap-3 overflow-x-auto">
                            <For each=sales
                                // the sale ID is just zero because I haven't figured out how to insert and fetch in an effiecient way...
                                // use the timestamp instead!
                                key=|(sale, _character)| sale.sold_date
                                view=|(sale, character)| items.get(&ItemId(sale.sold_item_id)).map(|item| (item, sale, character))
                                    .map(|(item, sale, character)| { view!{
                                        <div class="flex flex-col gap-1 whitespace-nowrap">
                                            <div class="flex flex-row flex-nowrap">
                                                <ItemIcon item_id=item.key_id.0 icon_size=IconSize::Small />
                                                <span>{&item.name}</span>
                                            </div>
                                            <Gil amount=sale.price_per_item />
                                            <span>{character.name}</span>
                                            <RelativeToNow timestamp=sale.sold_date />
                                        </div>} })

                            />
                    </div>
                </div>}
            }}

    </Suspense>
    }
}
