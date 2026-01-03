use crate::components::ad::Ad;
use crate::components::live_sale_ticker::LiveSaleTicker;
use crate::components::meta::{MetaDescription, MetaTitle};
use leptos::prelude::*;

#[component]
pub fn LiveSales() -> impl IntoView {
    view! {
        <MetaTitle title="Live Sales Ticker - Ultros" />
        <MetaDescription text="Watch real-time sales happening across your world. Spot deals and track market movement live." />

        <div class="main-content p-6">
            <div class="container mx-auto max-w-7xl">
                <div class="flex flex-col gap-8">
                    // Header Section
                    <div class="panel p-8 rounded-2xl bg-gradient-to-r from-brand-900 to-transparent">
                        <h1 class="text-3xl font-bold text-[color:var(--brand-fg)] mb-2">
                            "Live Sales Ticker"
                        </h1>
                        <p class="text-lg text-[color:var(--color-text)]/90">
                            "Monitor the market beat in real-time."
                        </p>
                    </div>

                    <div class="grid grid-cols-1 lg:grid-cols-3 gap-6">
                        // Main Ticker Area
                        <div class="lg:col-span-2">
                            <LiveSaleTicker />
                        </div>

                        // Sidebar / Ad Space
                        <div class="flex flex-col gap-4">
                            <div class="panel p-4 rounded-xl">
                                <h3 class="font-bold text-lg mb-2 text-[color:var(--color-text)]">"Sponsorship"</h3>
                                <Ad class="h-[250px] w-[300px]" />
                            </div>

                            <div class="panel p-6 rounded-xl bg-brand-900/10 border-brand-500/20">
                                <h3 class="font-bold text-lg mb-2 text-[color:var(--brand-fg)]">"Pro Tip"</h3>
                                <p class="text-[color:var(--color-text-muted)] text-sm">
                                    "Keep this page open on a second monitor to spot underpriced items as soon as they sell, giving you insight into active markets."
                                </p>
                            </div>
                        </div>
                    </div>
                </div>
            </div>
        </div>
    }
}
