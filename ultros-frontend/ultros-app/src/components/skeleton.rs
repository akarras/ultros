use leptos::prelude::*;

#[component]
pub fn SingleLineSkeleton() -> impl IntoView {
    view! {
        <div class="animate-pulse">
            <div class="w-full h-3 bg-gradient-to-r from-[color:color-mix(in_srgb,_var(--brand-ring)_20%,_transparent)] via-[color:color-mix(in_srgb,_var(--brand-ring)_10%,_transparent)] to-transparent rounded-md background-animate"></div>
            <div class="sr-only">"Loading"</div>
        </div>
    }.into_any()
}

#[component]
pub fn BoxSkeleton() -> impl IntoView {
    view! {
        <div class="w-full h-full animate-pulse">
            <div class="space-y-2">
                <div class="flex items-center gap-4 p-3 rounded-lg panel">
                    <div class="w-10 h-10 rounded-md bg-[color:color-mix(in_srgb,_var(--brand-ring)_16%,_transparent)]"></div>
                    <div class="flex-1 space-y-2">
                        <div class="h-3 w-3/5 bg-gradient-to-r from-[color:color-mix(in_srgb,_var(--brand-ring)_20%,_transparent)] via-[color:color-mix(in_srgb,_var(--brand-ring)_10%,_transparent)] to-transparent rounded-md background-animate"></div>
                        <div class="h-3 w-2/5 bg-gradient-to-r from-[color:color-mix(in_srgb,_var(--brand-ring)_20%,_transparent)] via-[color:color-mix(in_srgb,_var(--brand-ring)_10%,_transparent)] to-transparent rounded-md background-animate"></div>
                    </div>
                </div>
                <div class="flex items-center gap-4 p-3 rounded-lg panel">
                    <div class="w-10 h-10 rounded-md bg-[color:color-mix(in_srgb,_var(--brand-ring)_16%,_transparent)]"></div>
                    <div class="flex-1 space-y-2">
                        <div class="h-3 w-3/5 bg-gradient-to-r from-[color:color-mix(in_srgb,_var(--brand-ring)_20%,_transparent)] via-[color:color-mix(in_srgb,_var(--brand-ring)_10%,_transparent)] to-transparent rounded-md background-animate"></div>
                        <div class="h-3 w-2/5 bg-gradient-to-r from-[color:color-mix(in_srgb,_var(--brand-ring)_20%,_transparent)] via-[color:color-mix(in_srgb,_var(--brand-ring)_10%,_transparent)] to-transparent rounded-md background-animate"></div>
                    </div>
                </div>
                <div class="flex items-center gap-4 p-3 rounded-lg panel">
                    <div class="w-10 h-10 rounded-md bg-[color:color-mix(in_srgb,_var(--brand-ring)_16%,_transparent)]"></div>
                    <div class="flex-1 space-y-2">
                        <div class="h-3 w-3/5 bg-gradient-to-r from-[color:color-mix(in_srgb,_var(--brand-ring)_20%,_transparent)] via-[color:color-mix(in_srgb,_var(--brand-ring)_10%,_transparent)] to-transparent rounded-md background-animate"></div>
                        <div class="h-3 w-2/5 bg-gradient-to-r from-[color:color-mix(in_srgb,_var(--brand-ring)_20%,_transparent)] via-[color:color-mix(in_srgb,_var(--brand-ring)_10%,_transparent)] to-transparent rounded-md background-animate"></div>
                    </div>
                </div>
                <div class="flex items-center gap-4 p-3 rounded-lg panel">
                    <div class="w-10 h-10 rounded-md bg-[color:color-mix(in_srgb,_var(--brand-ring)_16%,_transparent)]"></div>
                    <div class="flex-1 space-y-2">
                        <div class="h-3 w-3/5 bg-gradient-to-r from-[color:color-mix(in_srgb,_var(--brand-ring)_20%,_transparent)] via-[color:color-mix(in_srgb,_var(--brand-ring)_10%,_transparent)] to-transparent rounded-md background-animate"></div>
                        <div class="h-3 w-2/5 bg-gradient-to-r from-[color:color-mix(in_srgb,_var(--brand-ring)_20%,_transparent)] via-[color:color-mix(in_srgb,_var(--brand-ring)_10%,_transparent)] to-transparent rounded-md background-animate"></div>
                    </div>
                </div>
                <div class="flex items-center gap-4 p-3 rounded-lg panel">
                    <div class="w-10 h-10 rounded-md bg-[color:color-mix(in_srgb,_var(--brand-ring)_16%,_transparent)]"></div>
                    <div class="flex-1 space-y-2">
                        <div class="h-3 w-3/5 bg-gradient-to-r from-[color:color-mix(in_srgb,_var(--brand-ring)_20%,_transparent)] via-[color:color-mix(in_srgb,_var(--brand-ring)_10%,_transparent)] to-transparent rounded-md background-animate"></div>
                        <div class="h-3 w-2/5 bg-gradient-to-r from-[color:color-mix(in_srgb,_var(--brand-ring)_20%,_transparent)] via-[color:color-mix(in_srgb,_var(--brand-ring)_10%,_transparent)] to-transparent rounded-md background-animate"></div>
                    </div>
                </div>
                <div class="flex items-center gap-4 p-3 rounded-lg panel">
                    <div class="w-10 h-10 rounded-md bg-[color:color-mix(in_srgb,_var(--brand-ring)_16%,_transparent)]"></div>
                    <div class="flex-1 space-y-2">
                        <div class="h-3 w-3/5 bg-gradient-to-r from-[color:color-mix(in_srgb,_var(--brand-ring)_20%,_transparent)] via-[color:color-mix(in_srgb,_var(--brand-ring)_10%,_transparent)] to-transparent rounded-md background-animate"></div>
                        <div class="h-3 w-2/5 bg-gradient-to-r from-[color:color-mix(in_srgb,_var(--brand-ring)_20%,_transparent)] via-[color:color-mix(in_srgb,_var(--brand-ring)_10%,_transparent)] to-transparent rounded-md background-animate"></div>
                    </div>
                </div>
            </div>
            <div class="sr-only">"loading"</div>
        </div>
    }.into_any()
}
