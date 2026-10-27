use super::{PluginContext, PluginLabsNotice, setup_available};
use crate::global_state::labs::use_dalamud_labs;
use crate::{components::clipboard::Clipboard, i18n::*};
use leptos::prelude::*;
use leptos_meta::Title;

#[component]
pub fn PluginSetup() -> impl IntoView {
    let i18n = use_i18n();
    let (dalamud_labs, _) = use_dalamud_labs();
    let PluginContext { info, .. } = expect_context::<PluginContext>();
    view! {
        <Title text=move || t_string!(i18n, plugin_guide_title) />
        <Show when=move || dalamud_labs.get() fallback=|| view! { <PluginLabsNotice /> }>
        <Suspense fallback=move || view! { <p role="status">{t!(i18n, plugin_loading)}</p> }>
            {move || match info.get().flatten() {
                Some(config) if setup_available(&config) => view! { <PluginSetupGuide /> }.into_any(),
                Some(_) => view! {
                    <p class="plugin-notice" role="status" data-testid="plugin-unavailable">{t!(i18n, plugin_guide_unavailable)}</p>
                    <a class="underline inline-block" href="/">{t!(i18n, home)}</a>
                }.into_any(),
                None => view! { <p role="status">{t!(i18n, plugin_guide_retry)}</p> }.into_any(),
            }}
        </Suspense>
        </Show>
    }
}

#[component]
fn PluginSetupGuide() -> impl IntoView {
    let i18n = use_i18n();
    let PluginContext { info, account } = expect_context::<PluginContext>();
    view! {
        <ul class="grid gap-5 sm:grid-cols-2" aria-label=move || t_string!(i18n, plugin_features) data-testid="plugin-features">
            <li class="space-y-1">
                <h2 class="font-semibold">{t!(i18n, plugin_guide_prices)}</h2>
                <p class="text-sm text-[color:var(--color-text-muted)]">{t!(i18n, plugin_guide_prices_body)}</p>
            </li>
            <li class="space-y-1">
                <h2 class="font-semibold">{t!(i18n, plugin_guide_lists)}</h2>
                <p class="text-sm text-[color:var(--color-text-muted)]">{t!(i18n, plugin_guide_lists_body)}</p>
            </li>
            <li class="space-y-1">
                <h2 class="font-semibold">{t!(i18n, plugin_guide_retainers)}</h2>
                <p class="text-sm text-[color:var(--color-text-muted)]">{t!(i18n, plugin_guide_retainers_body)}</p>
            </li>
            <li class="space-y-1">
                <h2 class="font-semibold">{t!(i18n, plugin_guide_alerts)}</h2>
                <p class="text-sm text-[color:var(--color-text-muted)]">{t!(i18n, plugin_guide_alerts_body)}</p>
            </li>
        </ul>
        <ol class="space-y-6 list-none" data-testid="plugin-install-guide">
            <li class="panel p-5 sm:p-7 space-y-4">
                <h2 class="text-xl font-bold"><span class="text-[color:var(--brand-fg)] mr-3">"1"</span>{t!(i18n, plugin_guide_install)}</h2>
                <p>{t!(i18n, plugin_guide_prerequisite)}" "<a class="underline" href="https://goatcorp.github.io/faq/" rel="noreferrer">{t!(i18n, plugin_guide_launcher)}</a></p>
                <Suspense fallback=|| ()>
                    {move || match info.get().flatten() {
                        Some(config) if config.enabled => {
                            let repository=config.repository_url.unwrap_or_default();
                            let copy=repository.clone();
                            view! {
                                <p>{t!(i18n, plugin_guide_repository_steps)}</p>
                                <div class="rounded-lg border border-[color:var(--color-outline)] p-3 space-y-2">
                                    <p class="text-sm font-semibold">{t!(i18n, plugin_guide_repository)}</p>
                                    <div class="flex items-center gap-3 min-w-0">
                                        <code class="break-all text-sm flex-1" data-testid="plugin-repository-url">{repository}</code>
                                        <Clipboard clipboard_text=copy />
                                    </div>
                                </div>
                                <p>{t!(i18n, plugin_guide_installer_steps)}</p>
                                {match config.latest_version {
                                    Some(version) => view! { <p class="text-sm font-semibold">{t!(i18n, plugin_guide_available)}": "<code>{version}</code></p> }.into_any(),
                                    None => view! { <p class="plugin-notice" role="status" data-testid="plugin-release-pending">{t!(i18n, plugin_guide_pending)}</p> }.into_any(),
                                }}
                            }.into_any()
                        },
                        Some(_) => view! { <p class="plugin-notice" role="status">{t!(i18n, plugin_guide_unavailable)}</p> }.into_any(),
                        None => view! { <p role="status">{t!(i18n, plugin_guide_retry)}</p> }.into_any(),
                    }}
                </Suspense>
            </li>
            <li class="panel p-5 sm:p-7 space-y-4">
                <h2 class="text-xl font-bold"><span class="text-[color:var(--brand-fg)] mr-3">"2"</span>{t!(i18n, plugin_guide_connect)}</h2>
                <Suspense fallback=|| ()>
                    {move || match account.get().flatten() {
                        Some(name) => view! { <p class="text-sm">{t!(i18n, plugin_guide_signed_in, name=name.clone())}</p> }.into_any(),
                        None => view! { <a class="btn-secondary inline-flex" href="/login?next=/plugin" rel="external">{t!(i18n, plugin_signin_button)}</a> }.into_any(),
                    }}
                </Suspense>
                <p>{t!(i18n, plugin_guide_connect_steps)}</p>
                <p class="text-sm text-[color:var(--color-text-muted)]">{t!(i18n, plugin_guide_approval)}</p>
                <a class="underline inline-block" href="/plugin/verify">{t!(i18n, plugin_guide_connect_link)}</a>
            </li>
        </ol>
        <details class="panel p-4" data-testid="plugin-setup-help">
            <summary>{t!(i18n, plugin_guide_troubleshooting)}</summary>
            <div class="space-y-3 mt-3">
            <details class="panel p-4"><summary>{t!(i18n, plugin_guide_code)}</summary><p class="mt-3">{t!(i18n, plugin_guide_phone)}</p><a class="underline" href="/plugin/verify">{t!(i18n, plugin_guide_connect_link)}</a></details>
            <details class="panel p-4"><summary>{t!(i18n, plugin_guide_retainers)}</summary><p class="mt-3">{t!(i18n, plugin_guide_uploads)}</p></details>
            <details class="panel p-4"><summary class="font-semibold cursor-pointer">{t!(i18n, plugin_guide_old_install)}</summary><p class="mt-3">{t!(i18n, plugin_guide_old_install_body)}</p></details>
            <details class="panel p-4"><summary class="font-semibold cursor-pointer">{t!(i18n, plugin_guide_update)}</summary><p class="mt-3">{t!(i18n, plugin_guide_update_body)}</p></details>
            <details class="panel p-4"><summary class="font-semibold cursor-pointer">{t!(i18n, plugin_guide_capture)}</summary><p class="mt-3">{t!(i18n, plugin_guide_capture_body)}</p></details>
            <Suspense fallback=|| ()>
                {move || info.get().flatten().and_then(|config|config.download_url).map(|download|view! {
                    <details class="panel p-4" data-testid="plugin-manual-install">
                        <summary class="font-semibold cursor-pointer">{t!(i18n, plugin_guide_manual)}</summary>
                        <p class="mt-3">{t!(i18n, plugin_guide_manual_body)}</p>
                        <a class="btn-secondary inline-flex mt-3" href=download download="Ultros.zip">{t!(i18n, plugin_guide_download)}</a>
                    </details>
                })}
            </Suspense>
            <Suspense fallback=|| ()>
                {move || info.get().flatten().filter(|config|config.localhost).map(|_|view! {
                    <details class="panel p-4">
                        <summary class="font-semibold cursor-pointer">{t!(i18n, plugin_guide_development)}</summary>
                        <p class="mt-3">{t!(i18n, plugin_guide_development_body)}</p>
                    </details>
                })}
            </Suspense>
            <a class="underline inline-block text-sm" href="https://github.com/akarras/ultros/tree/main/plugins/dalamud" rel="noreferrer">{t!(i18n, plugin_guide_developer_docs)}</a>
            </div>
        </details>
    }
}

#[cfg(all(test, feature = "ssr"))]
mod tests {
    use super::*;
    use futures::StreamExt;
    use ultros_api_types::plugin::PluginWebsiteInfo;

    fn released() -> PluginWebsiteInfo {
        PluginWebsiteInfo {
            enabled: true,
            localhost: false,
            origin: "https://ultros.app".into(),
            download_url: Some("/api/v1/plugin/releases/1.3.1.0/download".into()),
            repository_url: Some("https://ultros.app/api/v1/plugin/repository.json".into()),
            latest_version: Some("1.3.1.0".into()),
            minimum_version: Some("1.0.0.0".into()),
        }
    }

    async fn render(config: Option<PluginWebsiteInfo>) -> String {
        render_with_labs(config, true, false).await
    }

    async fn render_with_labs(
        config: Option<PluginWebsiteInfo>,
        labs: bool,
        verify: bool,
    ) -> String {
        let _ = any_spawner::Executor::init_futures_executor();
        let owner = Owner::new();
        let stream = owner.with(|| {
            provide_context(leptos_i18n::context::init_i18n_context::<Locale>());
            leptos_meta::provide_meta_context();
            crate::global_state::toasts::provide_toast_context();
            provide_context(axum::http::Request::new(()).into_parts().0);
            provide_context(crate::global_state::cookies::Cookies::new());
            if labs {
                let (_, enable_labs) = use_dalamud_labs();
                enable_labs(true);
            }
            let info = Resource::new_blocking(
                || (),
                move |_| {
                    let config = config.clone();
                    async move { config }
                },
            );
            let account = Resource::new_blocking(|| (), |_| async { None });
            provide_context(PluginContext { info, account });
            if verify {
                view! { <super::super::PluginVerify /> }.to_html_stream_in_order()
            } else {
                view! { <PluginSetup /> }.to_html_stream_in_order()
            }
        });
        stream.collect::<String>().await
    }

    #[tokio::test]
    async fn setup_and_approval_require_the_default_off_labs_opt_in() {
        for verify in [false, true] {
            let html = render_with_labs(Some(released()), false, verify).await;
            assert!(html.contains("plugin-labs-required"), "{html}");
            assert!(html.contains("/settings#labs"), "{html}");
            assert!(!html.contains("plugin-install-guide"), "{html}");
            assert!(!html.contains("/login?next=/plugin"), "{html}");
            assert!(!html.contains("plugin-select"), "{html}");
        }
    }

    fn assert_unavailable(html: &str) {
        assert!(html.contains("plugin-unavailable"), "{html}");
        assert!(!html.contains("plugin-install-guide"), "{html}");
        assert!(!html.contains("/login?next=/plugin"), "{html}");
        assert!(!html.contains("href=\"/plugin/verify\""), "{html}");
        assert!(!html.contains("plugin-repository-url"), "{html}");
        assert!(!html.contains("plugin-manual-install"), "{html}");
    }

    #[tokio::test]
    async fn disabled_connections_hide_installation_and_sign_in() {
        let mut config = released();
        config.enabled = false;
        assert_unavailable(&render(Some(config)).await);
    }

    #[tokio::test]
    async fn public_setup_waits_for_a_published_package() {
        for missing in 0..3 {
            let mut config = released();
            match missing {
                0 => config.latest_version = None,
                1 => config.repository_url = None,
                _ => config.download_url = None,
            }
            assert_unavailable(&render(Some(config)).await);
        }
    }

    #[tokio::test]
    async fn release_lookup_failure_does_not_offer_setup() {
        let html = render(None).await;
        assert!(html.contains("Release information could not be loaded"));
        assert!(!html.contains("plugin-install-guide"));
        assert!(!html.contains("/login?next=/plugin"));
    }

    #[tokio::test]
    async fn published_release_and_local_development_keep_the_guide() {
        for localhost in [false, true] {
            let mut config = released();
            if localhost {
                config.localhost = true;
                config.latest_version = None;
                config.download_url = None;
            }
            let html = render(Some(config)).await;
            assert!(html.contains("plugin-install-guide"), "{html}");
            assert!(html.contains("href=\"/plugin/verify\""), "{html}");
            assert!(!html.contains("plugin-unavailable"), "{html}");
            assert_eq!(html.contains("plugin-manual-install"), !localhost);
        }
    }
}
