use super::{datacenter_name::*, gil::*, relative_time::*, world_name::*};
use crate::components::icon::Icon;
use icondata as i;
use leptos::prelude::*;
use ultros_api_types::{SaleHistory, world_helper::AnySelector};

use crate::i18n::*;
use crate::i18n_fallback::use_i18n_or_default;

#[component]
pub fn SaleHistoryTable(sales: Signal<Vec<SaleHistory>>) -> impl IntoView {
    // Not `use_i18n()`: the item page builds this table inside a
    // `<Transition>`, so on the server it can be constructed under the fresh,
    // empty owner `ScopedFuture` substitutes when the request's owner was
    // already disposed. The panicking accessor aborts the SSR response there
    // (GlitchTip #7288); the default locale does not.
    let i18n = use_i18n_or_default();
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
        <div class="max-h-[26rem] overflow-y-auto overflow-x-auto rounded-lg border border-[color:var(--color-outline)]">
            <table class="w-full min-w-[720px]">
            <thead class="sticky top-0 z-10 bg-[color:var(--color-background)]">
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Reproduces GlitchTip #7288 — the sibling of #7289 in
    /// `listings_table.rs`. The item page builds this table inside a
    /// `<Transition>`, so on the server it is constructed when the resource
    /// resolves. If that request's owner was already disposed, `ScopedFuture`
    /// hands the fragment a fresh, empty owner that never saw
    /// `<I18nContextProvider>`, and the panicking `use_i18n()` aborts the
    /// whole SSR response. Rendering under a bare owner is that situation.
    #[test]
    fn renders_without_an_i18n_context() {
        let _ = any_spawner::Executor::init_futures_executor();
        let owner = Owner::new();
        owner.with(|| {
            let sales = vec![SaleHistory {
                id: 1,
                quantity: 2,
                price_per_item: 250,
                buying_character_id: 1,
                hq: false,
                sold_item_id: 1,
                sold_date: chrono::Utc::now().naive_utc(),
                world_id: 100,
                buyer_name: None,
            }];
            let html = view! { <SaleHistoryTable sales=Signal::stored(sales) /> }.to_html();
            assert!(html.contains("250"), "{html}");
        });
    }
}
