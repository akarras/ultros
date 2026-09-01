use super::{datacenter_name::*, gil::*, relative_time::*, world_name::*};
use crate::components::icon::Icon;
use icondata as i;
use leptos::prelude::*;
use ultros_api_types::{SaleHistory, world_helper::AnySelector};

use crate::i18n::*;

#[component]
pub fn SaleHistoryTable(sales: Signal<Vec<SaleHistory>>) -> impl IntoView {
    let i18n = use_i18n();
    let (show_more, set_show_more) = signal(false);
    // Optimization: Avoid cloning the entire sales vector when we only need a slice.
    // Using `sales.with` allows us to inspect the vector without cloning it.
    // If show_more is false, we only clone the first 10 items.
    let sale_history = Memo::new(move |_| {
        sales.with(|sales| {
            if show_more() {
                sales.clone()
            } else {
                sales.iter().take(10).cloned().collect()
            }
        })
    });
    view! {
        <div class="overflow-x-auto max-h-[60vh] overflow-y-auto rounded-lg">
            <table class="w-full text-sm min-w-[720px]">
            <thead class="text-xs uppercase">
                <tr>
                    <th scope="col">{t!(i18n, sale_history_col_hq)}</th>
                    <th scope="col">{t!(i18n, sale_history_col_price)}</th>
                    <th scope="col">{t!(i18n, sale_history_col_qty)}</th>
                    <th scope="col">{t!(i18n, sale_history_col_total)}</th>
                    <th scope="col">{t!(i18n, sale_history_col_purchaser)}</th>
                    <th scope="col">{t!(i18n, sale_history_col_world)}</th>
                    <th scope="col">{t!(i18n, sale_history_col_datacenter)}</th>
                    <th scope="col">{t!(i18n, sale_history_col_time_sold)}</th>
                </tr>
            </thead>
            <tbody class="divide-y divide-[color:var(--color-outline)]">
                <For
                    each=sale_history
                    key=move |sale| sale.sold_date.and_utc().timestamp()
                    children=move |sale| {
                        let total = sale.price_per_item * sale.quantity;
                        view! {
                            <tr>
                                <td>
                                    {sale
                                        .hq
                                        .then(|| {
                                            view! {
                                                <span class="sr-only">{t!(i18n, sale_history_high_quality_sr)}</span>
                                                <Icon icon=i::BsCheck aria_hidden=true />
                                            }
                                            .into_view()
                                        })}
                                </td>
                                <td>
                                    <Gil amount=sale.price_per_item />
                                </td>
                                <td>{sale.quantity}</td>
                                <td>
                                    <Gil amount=total />
                                </td>
                                <td>{sale.buyer_name}</td>
                                <td>
                                    <WorldName id=AnySelector::World(sale.world_id) />
                                </td>
                                <td>
                                    <DatacenterName world_id=sale.world_id />
                                </td>
                                <td>
                                    <RelativeToNow timestamp=sale.sold_date />
                                </td>
                            </tr>
                        }
                    }
                />

            </tbody>
        </table>
        </div>
        // Outside the overflow-x-auto container on purpose: as a table row the
        // button was as wide as the 720px-min table and scrolled (and clipped)
        // with it whenever the container was narrower than the table.
        {move || {
            (!show_more() && sales.with(|sales| sales.len() > 10))
                .then(|| {
                    view! {
                        <button
                            class="btn btn-primary w-full mt-2"
                            on:click=move |_| set_show_more(true)
                        >
                            {t!(i18n, sale_history_show_more)}
                        </button>
                    }
                })
        }}
    }
}
