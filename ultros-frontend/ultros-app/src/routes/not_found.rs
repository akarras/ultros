use leptos::prelude::*;
use leptos_meta::Title;
use leptos_router::components::A;

#[component]
pub fn NotFound() -> impl IntoView {
    view! {
        <Title text="Page Not Found - Ultros" />
        <div class="flex flex-col items-center justify-center min-h-[80vh] text-center space-y-12 p-4 overflow-hidden relative select-none">

            // Background effect
            <div class="fixed inset-0 pointer-events-none opacity-30 bg-[radial-gradient(circle_at_center,var(--brand-ring),transparent_70%)]"></div>

            <div class="relative w-64 h-64 sm:w-80 sm:h-80 flex items-center justify-center">
                 // Void Rings
                 // Outer Ring
                 <div class="absolute inset-0 border-4 border-dashed border-[color:var(--brand-text)]/30 rounded-full"
                      style="animation: spin 20s linear infinite;"></div>

                 // Middle Ring (Reverse)
                 <div class="absolute inset-8 border-2 border-dotted border-[color:var(--decor-spot)]/40 rounded-full"
                      style="animation: spin 15s linear infinite reverse;"></div>

                 // Inner Ring (Slow)
                 <div class="absolute inset-16 border border-[color:var(--color-text)]/20 rounded-full"
                      style="animation: spin 10s linear infinite;"></div>

                 // Core "Void"
                 <div class="relative w-32 h-32 bg-black/80 backdrop-blur-sm rounded-full flex items-center justify-center shadow-[0_0_50px_var(--brand-ring)] animate-pulse z-10 border border-[color:var(--brand-text)]/50">
                    <div class="absolute inset-0 bg-gradient-to-br from-purple-900/50 to-black rounded-full"></div>
                    <div class="z-20 text-5xl font-bold tracking-widest bg-clip-text text-transparent bg-gradient-to-br from-white to-gray-400">
                        "404"
                    </div>
                 </div>

                 // Orbiting Particles
                 <div class="absolute inset-0 animate-spin" style="animation-duration: 8s;">
                    <div class="absolute top-0 left-1/2 -translate-x-1/2 w-3 h-3 bg-[color:var(--brand-text)] rounded-full shadow-[0_0_10px_var(--brand-text)]"></div>
                 </div>
                 <div class="absolute inset-4 animate-spin" style="animation-duration: 12s; animation-direction: reverse;">
                    <div class="absolute bottom-0 left-1/2 -translate-x-1/2 w-2 h-2 bg-[color:var(--decor-spot)] rounded-full shadow-[0_0_10px_var(--decor-spot)]"></div>
                 </div>
            </div>

            <div class="space-y-6 max-w-lg z-10 relative">
                <h1 class="text-4xl sm:text-5xl font-extrabold tracking-tight drop-shadow-lg">
                    <span class="text-[color:var(--color-text)]">
                        "Lost in the Void"
                    </span>
                </h1>
                <p class="text-lg sm:text-xl text-[color:var(--color-text)] leading-relaxed font-medium drop-shadow-md">
                    "The page you are looking for has been cast into the void, or perhaps never existed at all."
                </p>

                <div class="pt-6 flex flex-wrap justify-center gap-4">
                    <A href="/" attr:class="btn btn-primary px-8 py-3 text-lg shadow-lg shadow-brand-900/20 hover:shadow-brand-900/40 transition-all duration-300">
                        "Return to Source"
                    </A>
                    <a href="javascript:history.back()" class="btn btn-neutral px-8 py-3 text-lg">
                        "Go Back"
                    </a>
                </div>
            </div>
        </div>
    }
}
