use leptos::prelude::*;

#[component]
pub fn ExchangeItem() -> impl IntoView {
    view! {
        <div class="container mx-auto p-4">
            <div class="panel p-6 rounded-xl mb-6">
                <h2 class="text-2xl font-bold mb-4 text-[color:var(--brand-fg)]">
                    "Currency Exchange"
                </h2>
                <p>"This feature is temporarily disabled due to data format changes."</p>
            </div>
        </div>
    }
}

#[component]
pub fn CurrencySelection() -> impl IntoView {
    view! {
        <div class="container mx-auto space-y-6">
            <div class="panel p-6 rounded-xl">
                <p class="text-[color:var(--color-text)] leading-relaxed">
                    "Currency Exchange is temporarily unavailable."
                </p>
            </div>
        </div>
    }
}

#[component]
pub fn CurrencyExchange() -> impl IntoView {
    view! {
        <div class="main-content">
            <h3 class="text-2xl font-bold text-[color:var(--brand-fg)]">
                "Currency Exchange"
            </h3>
            <ExchangeItem />
        </div>
    }
}
