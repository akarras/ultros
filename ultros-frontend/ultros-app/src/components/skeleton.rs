use leptos::*;

#[component]
pub fn SingleLineSkeleton() -> impl IntoView {
    view! {
        <div class="animate-pulse">
            <div class="w-full h-3 bg-gradient-to-r from-purple-950 to-transparent rounded-md background-animate"></div>
            <div class="sr-only">"Loading"</div>
        </div>
    }
}

#[component]
pub fn BoxSkeleton() -> impl IntoView {
    view! {
        <div class="w-full h-full bg-gradient-to-r from-purple-950 to-transparent rounded-md background-animate m-2">
            <div class="sr-only">"loading"</div>
        </div>
    }
}
