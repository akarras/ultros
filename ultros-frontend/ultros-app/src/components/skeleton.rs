use leptos::prelude::*;

#[component]
pub fn SingleLineSkeleton() -> impl IntoView {
    view! {
        <div class="animate-pulse">
            <div class="w-full h-3 bg-gradient-to-r from-white/10 via-white/5 to-transparent rounded-md background-animate"></div>
            <div class="sr-only">"Loading"</div>
        </div>
    }.into_any()
}

#[component]
pub fn BoxSkeleton() -> impl IntoView {
    view! {
        <div class="w-full h-full animate-pulse">
            <div class="space-y-2">
                <div class="flex items-center gap-4 p-3 rounded-lg bg-black/30 border border-white/5">
                    <div class="w-10 h-10 rounded-md bg-white/10"></div>
                    <div class="flex-1 space-y-2">
                        <div class="h-3 w-3/5 bg-gradient-to-r from-white/10 via-white/5 to-transparent rounded-md background-animate"></div>
                        <div class="h-3 w-2/5 bg-gradient-to-r from-white/10 via-white/5 to-transparent rounded-md background-animate"></div>
                    </div>
                </div>
                <div class="flex items-center gap-4 p-3 rounded-lg bg-black/30 border border-white/5">
                    <div class="w-10 h-10 rounded-md bg-white/10"></div>
                    <div class="flex-1 space-y-2">
                        <div class="h-3 w-3/5 bg-gradient-to-r from-white/10 via-white/5 to-transparent rounded-md background-animate"></div>
                        <div class="h-3 w-2/5 bg-gradient-to-r from-white/10 via-white/5 to-transparent rounded-md background-animate"></div>
                    </div>
                </div>
                <div class="flex items-center gap-4 p-3 rounded-lg bg-black/30 border border-white/5">
                    <div class="w-10 h-10 rounded-md bg-white/10"></div>
                    <div class="flex-1 space-y-2">
                        <div class="h-3 w-3/5 bg-gradient-to-r from-white/10 via-white/5 to-transparent rounded-md background-animate"></div>
                        <div class="h-3 w-2/5 bg-gradient-to-r from-white/10 via-white/5 to-transparent rounded-md background-animate"></div>
                    </div>
                </div>
                <div class="flex items-center gap-4 p-3 rounded-lg bg-black/30 border border-white/5">
                    <div class="w-10 h-10 rounded-md bg-white/10"></div>
                    <div class="flex-1 space-y-2">
                        <div class="h-3 w-3/5 bg-gradient-to-r from-white/10 via-white/5 to-transparent rounded-md background-animate"></div>
                        <div class="h-3 w-2/5 bg-gradient-to-r from-white/10 via-white/5 to-transparent rounded-md background-animate"></div>
                    </div>
                </div>
                <div class="flex items-center gap-4 p-3 rounded-lg bg-black/30 border border-white/5">
                    <div class="w-10 h-10 rounded-md bg-white/10"></div>
                    <div class="flex-1 space-y-2">
                        <div class="h-3 w-3/5 bg-gradient-to-r from-white/10 via-white/5 to-transparent rounded-md background-animate"></div>
                        <div class="h-3 w-2/5 bg-gradient-to-r from-white/10 via-white/5 to-transparent rounded-md background-animate"></div>
                    </div>
                </div>
                <div class="flex items-center gap-4 p-3 rounded-lg bg-black/30 border border-white/5">
                    <div class="w-10 h-10 rounded-md bg-white/10"></div>
                    <div class="flex-1 space-y-2">
                        <div class="h-3 w-3/5 bg-gradient-to-r from-white/10 via-white/5 to-transparent rounded-md background-animate"></div>
                        <div class="h-3 w-2/5 bg-gradient-to-r from-white/10 via-white/5 to-transparent rounded-md background-animate"></div>
                    </div>
                </div>
            </div>
            <div class="sr-only">"loading"</div>
        </div>
    }.into_any()
}
