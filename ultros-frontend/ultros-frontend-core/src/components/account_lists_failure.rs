//! The account-list section's failure line in the add-to-list modals.
//!
//! Inside the `lists-sync` lab a signed-out visitor still has the device
//! lists that [`LocalListTargets`](super::local_list_targets::LocalListTargets)
//! renders right below, so their `NotAuthenticated` is nothing to warn about
//! — the modal simply offers what they can add to. Every other failure, and a
//! signed-out visitor outside the lab, still gets the message.
use leptos::prelude::*;
use ultros_api_types::result::ApiError;

use crate::error::AppError;
use crate::global_state::local_lists::use_local_lists;
use crate::i18n::{t, use_i18n};

/// Whether `error` needs explaining to the player. `local_lists` is whether
/// the device-list section is on offer in the same modal.
pub fn needs_explaining(error: &AppError, local_lists: bool) -> bool {
    !(local_lists && matches!(error, AppError::ApiError(ApiError::NotAuthenticated)))
}

#[component]
pub fn AccountListsFailure(error: AppError) -> impl IntoView {
    let i18n = use_i18n();
    let local_lists = use_local_lists();
    view! {
        <Show when=move || needs_explaining(&error, local_lists.get().is_some())>
            <div class="text-red-400 text-sm">{t!(i18n, add_to_list_unable_to_load)}</div>
        </Show>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signed_out_is_silent_only_with_device_lists_on_offer() {
        let signed_out = AppError::ApiError(ApiError::NotAuthenticated);
        assert!(!needs_explaining(&signed_out, true));
        assert!(needs_explaining(&signed_out, false));
    }

    #[test]
    fn every_other_failure_is_explained() {
        for error in [
            AppError::Json("unexpected end of input".into()),
            AppError::ApiError(ApiError::Forbidden),
            AppError::InternalApiTimeout,
        ] {
            assert!(needs_explaining(&error, true), "{error:?}");
            assert!(needs_explaining(&error, false), "{error:?}");
        }
    }
}
