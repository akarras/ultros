use std::ops::Range;

use super::{datacenter_name::*, gil::*, relative_time::*, world_name::*};
use chrono::Utc;
use icondata as i;
use leptos::*;
use leptos_icons::*;
use ultros_api_types::{world_helper::AnySelector, SaleHistory};

#[component]
pub fn SaleHistoryTable(sales: Signal<Vec<SaleHistory>>) -> impl IntoView {
    let (show_more, set_show_more) = create_signal(false);
    let sale_history = create_memo(move |_| {
        let mut sales = sales();
        if !show_more() {
            sales.truncate(10);
        }
        sales
    });
    view! {  <table class="w-full">
        <thead>
            <tr>
                <th>"hq"</th>
                <th>"price"</th>
                <th>"qty."</th>
                <th>"total"</th>
                <th>"purchaser"</th>
                <th>"world"</th>
                <th>"datacenter"</th>
                <th>"time sold"</th>
            </tr>
        </thead>
        <tbody>
            <For each=sale_history
                key=move |sale| sale.sold_date.timestamp()
                children=move |sale| {
                    let total = sale.price_per_item * sale.quantity;
                    view! {
                        <tr>
                            <td>{sale.hq.then(||{view!{<Icon icon=i::BsCheck />}.into_view()})}</td>
                            <td><Gil amount=sale.price_per_item/></td>
                            <td>{sale.quantity}</td>
                            <td><Gil amount=total /></td>
                            <td>{sale.buyer_name}</td>
                            <td><WorldName id=AnySelector::World(sale.world_id)/></td>
                            <td><DatacenterName world_id=sale.world_id/></td>
                            <td><RelativeToNow timestamp=sale.sold_date/></td>
                        </tr>
                    }
                }
            />
            {move || (!show_more() && sales.with(|sales| sales.len() > 10)).then(|| {
                view!{<tr><td colspan="8"><button class="btn" style="width: 100%;" on:click=move |_| set_show_more(true)>"Show more"</button></td></tr>}
            })}
        </tbody>
    </table>
    }
}

struct SalesWindow {
    /// Total amount of gil sold in this window
    total_gil: f64,
    average_gil: f64,
    minimum_price_sold: f64,
    maximum_price_sold: f64,
    average_stack_size: f64,
    projected_sale_price: f64,
}

impl SalesWindow {
    fn new(date_range: Range<Utc>, sales: &[SaleHistory]) -> Self {
        
        todo!()
    }
}


/// The SalesSummaryData should provide generic market analytics
struct SalesSummaryData {
    
}

impl SalesSummaryData {
    fn new(sale_history: &[SaleHistory]) -> Self {
        sale_history.iter().map(|sale| sale.price_per_item);
        let sold_per_day = sale_history.iter().map(|s| s.quantity);
        todo!("New sales summary data");
    }
}

#[component]
pub fn SalesInsights(sales: Signal<Vec<SaleHistory>>) -> impl IntoView {
    let sales = sales.with(|sales| {
        SalesSummaryData::new(&sales)
    });

    view ! {

    }
}
