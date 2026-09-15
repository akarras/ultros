use leptos::prelude::*;
use ultros_list_doc::{Quality, RowChange, RowSnapshot, diff_rows};
use xiv_gen::ItemId;

use super::handle::{ListDocHandle, RecoveryState};
use crate::global_state::{LocalWorldData, xiv_data::tracked_data};
use crate::i18n::*;

#[component]
pub fn ListRecovery(doc: ListDocHandle) -> impl IntoView {
    let i18n = use_i18n();
    let worlds = StoredValue::new(use_context::<LocalWorldData>().and_then(|data| data.0.ok()));
    view! {
        <Show when=move || doc.recovery_state.try_get().is_some_and(|state| state != RecoveryState::None)>
            <section data-testid="list-recovery" class="mt-3 rounded-lg border border-amber-500/50 bg-amber-500/10 p-3 text-sm">
                {move || if doc.recovery_state.try_get() == Some(RecoveryState::Review) {
                    view! {
                        <h3 role="alert" class="font-semibold">{t!(i18n, list_recovery_title)}</h3>
                        <p class="mt-2">{t!(i18n, list_recovery_help)}</p>
                        <div class="mt-3 overflow-x-auto">
                            <table class="w-full text-left">
                                <thead><tr>
                                    <th class="p-2">{t!(i18n, list_recovery_field)}</th>
                                    <th class="p-2">{t!(i18n, list_recovery_local)}</th>
                                    <th class="p-2">{t!(i18n, list_recovery_server)}</th>
                                </tr></thead>
                                <tbody>{
                                    move || {
                                        let _ = doc.revision.try_get();
                                        let Some(server) = doc.recovery_server() else { return Vec::new(); };
                                        let local_meta = doc.meta();
                                        let server_meta = server.meta();
                                        let mut comparisons = Vec::new();
                                        if local_meta.name != server_meta.name {
                                            comparisons.push((t_string!(i18n, list_view_settings_rename_label).to_string(), local_meta.name, server_meta.name));
                                        }
                                        if local_meta.scope != server_meta.scope {
                                            let name = |scope: Option<ultros_api_types::world_helper::AnySelector>| scope.and_then(|scope| worlds.get_value().as_ref()?.lookup_selector(scope).map(|world| world.get_name().to_string())).unwrap_or_else(|| "—".into());
                                            comparisons.push((t_string!(i18n, list_recovery_scope).to_string(), name(local_meta.scope), name(server_meta.scope)));
                                        }
                                        let describe = |row: Option<RowSnapshot>| match row {
                                            Some(row) => t_string!(i18n, list_recovery_values, needed = row.need, owned = row.acquired, target = row.target.map(|n| n.to_string()).unwrap_or_else(|| "—".into())).to_string(),
                                            None => t_string!(i18n, list_recovery_absent).to_string(),
                                        };
                                        for change in diff_rows(&server.rows(), &doc.rows()) {
                                            let key = change.key();
                                            let name = tracked_data().items.get(&ItemId(key.item_id)).map(|item| item.name.to_string()).unwrap_or_else(|| key.item_id.to_string());
                                            let quality = match key.quality {
                                                Quality::Any => t_string!(i18n, lists_workspace_any_quality).to_string(),
                                                Quality::Hq => "HQ".into(),
                                                Quality::Nq => "NQ".into(),
                                            };
                                            let (local, remote) = match change {
                                                RowChange::Added(row) => (Some(row), None),
                                                RowChange::Removed(row) => (None, Some(row)),
                                                RowChange::Updated { before, after } => (Some(after), Some(before)),
                                            };
                                            comparisons.push((format!("{name} · {quality}"), describe(local), describe(remote)));
                                        }
                                        comparisons.into_iter().map(|(field, local, server)| view! {
                                            <tr><th scope="row" class="p-2 font-normal">{field}</th><td class="p-2">{local}</td><td class="p-2">{server}</td></tr>
                                        }).collect::<Vec<_>>()
                                    }
                                }</tbody>
                            </table>
                        </div>
                        <div class="mt-3 flex flex-wrap gap-2">
                            <button type="button" class="btn-secondary" on:click=move |_| doc.download_recovery()>{t!(i18n, account_list_save_export)}</button>
                            <button type="button" class="btn-secondary" data-testid="list-recovery-retry" on:click=move |_| doc.retry_recovery()>{t!(i18n, list_recovery_retry)}</button>
                            <button type="button" class="btn-secondary max-w-full whitespace-normal" data-testid="list-recovery-use-server" on:click=move |_| doc.use_server_version()>{t!(i18n, list_recovery_use_server)}</button>
                        </div>
                    }.into_any()
                } else {
                    view! {
                        <p role="status">{t!(i18n, list_recovery_reset)}</p>
                        <Show when=move || doc.recovery_state.try_get() == Some(RecoveryState::Recovered { dropped_meta: true })>
                            <p class="mt-2">{t!(i18n, list_recovery_meta_dropped)}</p>
                        </Show>
                    }.into_any()
                }}
            </section>
        </Show>
    }
}
