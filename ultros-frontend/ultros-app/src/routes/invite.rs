use crate::components::loading::Loading;
use leptos::prelude::*;
use leptos_router::hooks::use_params_map;

#[component]
pub fn InviteAccept() -> impl IntoView {
    let params = use_params_map();
    let invite_id = Memo::new(move |_| params.with(|p| p.get("id").clone()).unwrap_or_default());
    let (message, set_message) = signal("Accepting shared list invite...".to_string());

    #[cfg(feature = "hydrate")]
    {
        use crate::{api::use_list_invite, global_state::toasts::use_toast};
        use leptos_router::{NavigateOptions, hooks::use_navigate};

        let nav = use_navigate();
        let toasts = use_toast();
        Effect::new(move |_| {
            let invite_id = invite_id();
            let nav = nav.clone();
            if invite_id.is_empty() {
                set_message("Invite link is missing an invite id.".to_string());
                return;
            }

            leptos::task::spawn_local(async move {
                match use_list_invite(invite_id.clone()).await {
                    Ok(list_id) => {
                        if let Some(toasts) = toasts {
                            toasts.success("Shared list added.");
                        }
                        nav(
                            &format!("/list/{list_id}"),
                            NavigateOptions {
                                replace: true,
                                ..Default::default()
                            },
                        );
                    }
                    Err(error) => {
                        let text = format!("Unable to accept invite: {error}");
                        if let Some(toasts) = toasts {
                            toasts.error(text.clone());
                        }
                        set_message(text);
                    }
                }
            });
        });
    }

    #[cfg(not(feature = "hydrate"))]
    {
        let _ = invite_id;
        let _ = set_message;
    }

    view! {
        <div class="main-content">
            <div class="container mx-auto max-w-xl">
                <div class="panel p-6 rounded-xl flex flex-col gap-4 items-center text-center">
                    <Loading />
                    <h1 class="text-2xl font-bold text-[color:var(--brand-fg)]">"Shared List Invite"</h1>
                    <p class="text-[color:var(--color-text-muted)]">{message}</p>
                </div>
            </div>
        </div>
    }
    .into_any()
}
