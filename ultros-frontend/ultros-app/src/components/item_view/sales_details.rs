use crate::components::sale_history_table::{SaleHistoryTable, SalesInsights};
use crate::components::skeleton::BoxSkeleton;
use crate::error::AppError;
use leptos::prelude::*;
use ultros_api_types::CurrentlyShownItem;

#[component]
pub fn SalesDetails(
    listing_resource: Resource<Result<CurrentlyShownItem, AppError>>,
) -> impl IntoView {
    view! {
        // Removed mt-8 and space-y-6 wrapper to let grid control layout
        <Transition fallback=move || {
            view! { <BoxSkeleton /> }
        }>
            {move || {
                let sales = Memo::new(move |_| {
                    listing_resource
                        .with(|l| {
                            l.as_ref().and_then(|l| l.as_ref().map(|l| l.sales.clone()).ok())
                        })
                        .unwrap_or_default()
                });

                view! {
                    <div class="flex flex-col gap-6 h-full"> // Use flex col to stack table and insights
                        <div class="panel p-4 sm:p-6 flex-1">
                            <h2 class="text-xl font-bold text-center mb-4 text-brand-200">
                                "Sale History"
                            </h2>
                            <SaleHistoryTable sales=sales.into() />
                        </div>

                        <div class="panel p-4 sm:p-6">
                            <SalesInsights sales=sales.into() />
                        </div>
                    </div>
                }
                    .into_any()
            }}
        </Transition>
    }
    .into_any()
}
