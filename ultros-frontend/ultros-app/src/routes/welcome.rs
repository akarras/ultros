use leptos::either::Either;
use leptos::prelude::*;
use leptos_router::components::A;

use crate::components::icon::Icon;
use crate::components::language_picker::LanguagePicker;
use crate::components::meta::{MetaDescription, MetaTitle};
use crate::components::world_picker::{WorldOnlyPicker, WorldPicker};
use crate::global_state::home_world::{
    get_price_zone, result_to_selector_read, selector_to_setter_signal, use_home_world,
};
use crate::i18n::*;

use icondata as i;

#[component]
pub fn Welcome() -> impl IntoView {
    let i18n = use_i18n();
    let (homeworld, set_homeworld) = use_home_world();
    let (price_region, set_price_region) = get_price_zone();
    let price_region = result_to_selector_read(price_region);
    let set_price_region = selector_to_setter_signal(set_price_region);

    // ⚡ Bolt Optimization: Replace Memo::new with Signal::derive to remove reactive allocation overhead
    let has_homeworld = Signal::derive(move || homeworld.with(|w| w.is_some()));

    view! {
        <div class="main-content p-6">
            <MetaTitle title=move || t_string!(i18n, welcome_page_title).to_string() />
            <MetaDescription text=move || t_string!(i18n, welcome_page_desc).to_string() />

            <div class="container mx-auto max-w-4xl space-y-8">
                // Hero
                <div class="panel p-6 sm:p-10 rounded-2xl relative overflow-hidden">
                    <div class="flex flex-col md:flex-row items-center gap-6 md:gap-10">
                        <div class="flex-1 space-y-3 z-10">
                            <h1 class="text-5xl sm:text-6xl font-extrabold leading-none tracking-tighter">
                                <span class="bg-clip-text text-transparent bg-gradient-to-br from-brand-300 via-purple-400 to-pink-500">
                                    {t!(i18n, welcome_heading)}
                                </span>
                            </h1>
                            <p class="text-lg text-[color:var(--color-text-muted)] max-w-prose leading-relaxed">
                                {t!(i18n, welcome_subtitle)}
                            </p>
                        </div>
                        <div class="w-32 md:w-40 aspect-square rounded-2xl elevated surface-blur flex items-center justify-center animate-float">
                            <Icon
                                icon=i::FaMapLocationDotSolid
                                width="4em"
                                height="4em"
                                attr:class="text-brand-300"
                            />
                        </div>
                    </div>
                </div>

                // Why
                <div class="panel p-6 rounded-xl">
                    <h2 class="text-xl font-bold text-[color:var(--brand-fg)] mb-2 flex items-center gap-2">
                        <Icon icon=i::BsInfoCircle />
                        {t!(i18n, welcome_why_home_world_title)}
                    </h2>
                    <p class="text-[color:var(--color-text)] leading-relaxed">
                        {t!(i18n, welcome_why_home_world_body)}
                    </p>
                </div>

                // Step 1: home world
                <div class="panel p-6 rounded-xl space-y-4 border-l-4 border-brand-300/60">
                    <div>
                        <h2 class="text-2xl font-bold text-[color:var(--brand-fg)]">
                            {t!(i18n, welcome_step_home_world_label)}
                        </h2>
                        <p class="text-sm text-[color:var(--color-text-muted)] mt-1">
                            {t!(i18n, welcome_step_home_world_help)}
                        </p>
                    </div>
                    <div class="max-w-md">
                        <WorldOnlyPicker
                            current_world=homeworld
                            set_current_world=set_homeworld
                        />
                    </div>
                    {move || {
                        if let Some(w) = homeworld.get() {
                            Either::Left(view! {
                                <div class="mt-2 p-3 rounded-lg bg-green-900/20 border border-green-700/30 text-green-300 flex items-center gap-2">
                                    <Icon icon=i::BsCheckCircleFill />
                                    <span>
                                        {t!(i18n, welcome_home_world_set_with_name, world = w.name.clone())}
                                    </span>
                                </div>
                            })
                        } else {
                            Either::Right(view! {
                                <div class="mt-2 text-sm italic text-[color:var(--color-text-muted)]">
                                    {t!(i18n, welcome_pick_world_to_continue)}
                                </div>
                            })
                        }
                    }}
                </div>

                // Step 2: price zone
                <div class="panel p-6 rounded-xl space-y-4">
                    <div>
                        <h2 class="text-2xl font-bold text-[color:var(--brand-fg)]">
                            {t!(i18n, welcome_step_price_zone_label)}
                        </h2>
                        <p class="text-sm text-[color:var(--color-text-muted)] mt-1">
                            {t!(i18n, welcome_step_price_zone_help)}
                        </p>
                    </div>
                    <div class="max-w-md">
                        <WorldPicker
                            current_world=price_region
                            set_current_world=set_price_region
                        />
                    </div>
                </div>

                // Step 3: language
                <div class="panel p-6 rounded-xl space-y-4">
                    <div>
                        <h2 class="text-2xl font-bold text-[color:var(--brand-fg)]">
                            {t!(i18n, welcome_step_language_label)}
                        </h2>
                        <p class="text-sm text-[color:var(--color-text-muted)] mt-1">
                            {t!(i18n, welcome_step_language_help)}
                        </p>
                    </div>
                    <div class="max-w-md">
                        <LanguagePicker />
                    </div>
                </div>

                // CTA
                <div class="flex flex-wrap items-center justify-between gap-4 pt-2">
                    <A
                        href="/"
                        attr:class="btn-ghost py-3 px-6"
                    >
                        {t!(i18n, welcome_skip_for_now)}
                    </A>
                    {move || {
                        let enabled = has_homeworld.get();
                        let class = if enabled {
                            "btn-primary py-3 px-6 text-lg"
                        } else {
                            "btn-primary py-3 px-6 text-lg opacity-50 pointer-events-none"
                        };
                        view! {
                            <A href="/" attr:class=class attr:aria-disabled=move || (!enabled).then_some("true")>
                                <span>{t!(i18n, welcome_continue_cta)}</span>
                                <Icon icon=i::FaArrowRightSolid width="1em" height="1em" />
                            </A>
                        }
                    }}
                </div>
            </div>
        </div>
    }
    .into_any()
}
