use crate::global_state::cookies::Cookies;
use crate::global_state::crafter_levels::CrafterLevels;
use crate::i18n::{t, t_string, use_i18n};
use leptos::prelude::*;

#[component]
pub fn CrafterSettings() -> impl IntoView {
    let i18n = use_i18n();
    let cookies = use_context::<Cookies>().unwrap();
    let (levels, set_levels) = cookies.use_cookie_typed::<_, CrafterLevels>("CRAFTER_LEVELS");

    let update_level = move |job: &str, level: i32| {
        let mut current = levels.get_untracked().unwrap_or_default();
        match job {
            "CRP" => current.carpenter = level,
            "BSM" => current.blacksmith = level,
            "ARM" => current.armorer = level,
            "GSM" => current.goldsmith = level,
            "LTW" => current.leatherworker = level,
            "WVR" => current.weaver = level,
            "ALC" => current.alchemist = level,
            "CUL" => current.culinarian = level,
            _ => {}
        }
        set_levels(Some(current));
    };
    type CrafterCallback = fn(&CrafterLevels) -> i32;
    let jobs: [(&str, String, CrafterCallback); 8] = [
        (
            "CRP",
            t_string!(i18n, carpenter).to_string(),
            (|l: &CrafterLevels| l.carpenter) as CrafterCallback,
        ),
        (
            "BSM",
            t_string!(i18n, blacksmith).to_string(),
            (|l: &CrafterLevels| l.blacksmith) as CrafterCallback,
        ),
        (
            "ARM",
            t_string!(i18n, armorer).to_string(),
            (|l: &CrafterLevels| l.armorer) as CrafterCallback,
        ),
        (
            "GSM",
            t_string!(i18n, goldsmith).to_string(),
            (|l: &CrafterLevels| l.goldsmith) as CrafterCallback,
        ),
        (
            "LTW",
            t_string!(i18n, leatherworker).to_string(),
            (|l: &CrafterLevels| l.leatherworker) as CrafterCallback,
        ),
        (
            "WVR",
            t_string!(i18n, weaver).to_string(),
            (|l: &CrafterLevels| l.weaver) as CrafterCallback,
        ),
        (
            "ALC",
            t_string!(i18n, alchemist).to_string(),
            (|l: &CrafterLevels| l.alchemist) as CrafterCallback,
        ),
        (
            "CUL",
            t_string!(i18n, culinarian).to_string(),
            (|l: &CrafterLevels| l.culinarian) as CrafterCallback,
        ),
    ];

    view! {
        <div class="panel p-6 rounded-xl">
            <h3 class="text-2xl font-bold text-[color:var(--brand-fg)] mb-4">{t!(i18n, crafter_levels_title)}</h3>
            <div class="grid grid-cols-2 md:grid-cols-4 gap-4">
                {jobs.into_iter()
                    .map(|(code, name, getter)| {
                        let id = format!("crafter-level-{}", code);
                        view! {
                            <div class="space-y-1">
                                <label
                                    class="text-sm font-medium text-[color:var(--color-text-muted)]"
                                    for=id.clone()
                                >
                                    {name}
                                </label>
                                <div class="relative">
                                    <input
                                        id=id
                                        type="number"
                                        min="0"
                                        max="100"
                                        class="input w-full"
                                        prop:value=move || {
                                            levels.get().map(|l| getter(&l)).unwrap_or(0)
                                        }
                                        on:change=move |ev| {
                                            let val = event_target_value(&ev).parse::<i32>().unwrap_or(0);
                                            log::info!("Updating level for {}: {}", code, val);
                                            update_level(code, val);
                                        }
                                    />
                                    <span class="absolute right-3 top-1/2 -translate-y-1/2 text-xs text-[color:var(--color-text-muted)] pointer-events-none">
                                        {t!(i18n, item_explorer_lv_prefix)}
                                    </span>
                                </div>
                            </div>
                        }
                    })
                    .collect::<Vec<_>>()}
            </div>
            <p class="mt-4 text-sm text-gray-400">
                {t!(i18n, crafter_levels_help)}
            </p>
        </div>
    }
    .into_any()
}
