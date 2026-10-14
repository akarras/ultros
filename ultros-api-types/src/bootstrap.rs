use serde::{Deserialize, Serialize};

use crate::user::UserData;
use crate::world::WorldData;

/// Server-rendered payload embedded in the initial HTML so the client can
/// hydrate without making `/world_data`, `/detectregion`, and `/current_user`
/// requests on every cold load.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Bootstrap {
    pub world_data: WorldData,
    pub region: String,
    pub current_user: Option<UserData>,
    /// Operator kill-switch (`ULTROS_LISTS_READONLY` on the server): when
    /// set, list routes render a read-only view and never open a list
    /// document. Defaults to `false` so payloads written before the field
    /// existed — stale cached HTML and the service worker's generated
    /// offline-guest shell, which builds this object by hand — still decode.
    #[serde(default)]
    pub lists_readonly: bool,
}

#[cfg(test)]
mod tests {
    use super::Bootstrap;

    #[test]
    fn payloads_without_the_readonly_flag_decode_as_editable() {
        // The exact shape the server emitted before the field existed, and
        // the shape the service-worker shell still builds by hand.
        let old = r#"{"world_data":{"regions":[]},"region":"","current_user":null}"#;
        let decoded: Bootstrap = serde_json::from_str(old).expect("old bootstrap decodes");
        assert!(!decoded.lists_readonly);
    }
}
