use crate::global_state::cookies::Cookies;
use crate::global_state::crafter_levels::CrafterLevels;
use crate::i18n::*;
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
    let jobs: [(&str, CrafterCallback); 8] = [
        ("CRP", |l: &CrafterLevels| l.carpenter),
        ("BSM", |l: &CrafterLevels| l.blacksmith),
        ("ARM", |l: &CrafterLevels| l.armorer),
        ("GSM", |l: &CrafterLevels| l.goldsmith),
        ("LTW", |l: &CrafterLevels| l.leatherworker),
        ("WVR", |l: &CrafterLevels| l.weaver),
        ("ALC", |l: &CrafterLevels| l.alchemist),
        ("CUL", |l: &CrafterLevels| l.culinarian),
    ];

    view! {
        <div class="panel p-6 rounded-xl">
            <h3 class="text-2xl font-bold text-[color:var(--brand-fg)] mb-4">"Crafter Levels"</h3>
            <div class="grid grid-cols-2 md:grid-cols-4 gap-4">
                {jobs.into_iter()
                    .map(|(code, getter)| {
                        let id = format!("crafter-level-{}", code);
                        let label = match code {
                            "CRP" => view! { {t!(i18n, carpenter)} }.into_any(),
                            "BSM" => view! { {t!(i18n, blacksmith)} }.into_any(),
                            "ARM" => view! { {t!(i18n, armorer)} }.into_any(),
                            "GSM" => view! { {t!(i18n, goldsmith)} }.into_any(),
                            "LTW" => view! { {t!(i18n, leatherworker)} }.into_any(),
                            "WVR" => view! { {t!(i18n, weaver)} }.into_any(),
                            "ALC" => view! { {t!(i18n, alchemist)} }.into_any(),
                            "CUL" => view! { {t!(i18n, culinarian)} }.into_any(),
                            _ => view! { {t!(i18n, unknown)} }.into_any(),
                        };
                        view! {
                            <div class="space-y-1">
                                <label
                                    class="text-sm font-medium text-[color:var(--color-text-muted)]"
                                    for=id.clone()
                                >
                                    {label}
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
                                        "Lv"
                                    </span>
                                </div>
                            </div>
                        }
                    })
                    .collect::<Vec<_>>()}
            </div>
            <p class="mt-4 text-sm text-gray-400">
                "Set your crafter levels to filter recipes in the Recipe Analyzer."
            </p>
        </div>
    }
    .into_any()
}
