//! Per-adapter capability flags advertised over the HTTP contract.
//!
//! [`Capabilities`] is the cheap, `Copy`-able discovery struct returned
//! by `DatabaseAdapter::capabilities` (added in the same Phase 2 work).
//! One flag per optional capability trait; adding a new capability is one
//! new flag here plus one new marker trait under this module, per
//! ADR-0012.
//!
//! The wire shape is a flat JSON object with the same `snake_case`
//! field names. The `Default` instance is "all false" — used by
//! adapters that ship no capabilities.

mod auth;
mod functions;
mod realtime;
mod storage;
mod views;

pub use auth::AuthAdmin;
pub use functions::FunctionIntrospection;
pub use realtime::RealtimeChannels;
pub use storage::StorageAdmin;
pub use views::ViewIntrospection;

use serde::{Deserialize, Serialize};

// The struct_excessive_bools lint suggests an enum or state machine, but
// these flags are independent and the flat JSON shape is fixed by
// ADR-0012 for HTTP discovery — collapsing them would break the wire
// contract and the "add one flag per capability" extension model.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capabilities {
    pub has_views: bool,
    pub has_functions: bool,
    pub has_auth: bool,
    pub has_storage: bool,
    pub has_realtime: bool,
    /// The adapter implements `DatabaseAdapter::describe_table`
    /// (ADR-0028). `#[serde(default)]` keeps pre-ADR-0028
    /// `/capabilities` payloads parseable — the flag reads as `false`.
    #[serde(default)]
    pub has_describe_table: bool,
}

#[cfg(test)]
mod tests {
    use super::Capabilities;

    #[test]
    fn default_capabilities_are_all_false() {
        let caps = Capabilities::default();
        assert!(!caps.has_views);
        assert!(!caps.has_functions);
        assert!(!caps.has_auth);
        assert!(!caps.has_storage);
        assert!(!caps.has_realtime);
        assert!(!caps.has_describe_table);
    }

    #[test]
    fn each_flag_can_be_set_independently() {
        let caps = Capabilities {
            has_views: true,
            ..Capabilities::default()
        };
        assert!(caps.has_views);
        assert!(!caps.has_functions);
        assert!(!caps.has_auth);
        assert!(!caps.has_storage);
        assert!(!caps.has_realtime);
        assert!(!caps.has_describe_table);
    }

    #[test]
    fn capabilities_are_copy() {
        let caps = Capabilities {
            has_auth: true,
            ..Capabilities::default()
        };
        let copy = caps;
        // Both still usable because Capabilities is Copy.
        assert!(caps.has_auth);
        assert!(copy.has_auth);
    }

    #[test]
    fn serializes_as_a_flat_snake_case_object() {
        let caps = Capabilities {
            has_views: true,
            has_realtime: true,
            ..Capabilities::default()
        };
        let json = serde_json::to_string(&caps).unwrap();
        assert_eq!(
            json,
            r#"{"has_views":true,"has_functions":false,"has_auth":false,"has_storage":false,"has_realtime":true,"has_describe_table":false}"#
        );
    }

    #[test]
    fn capabilities_round_trip_through_json() {
        let caps = Capabilities {
            has_views: true,
            has_functions: false,
            has_auth: true,
            has_storage: false,
            has_realtime: true,
            has_describe_table: true,
        };
        let json = serde_json::to_string(&caps).unwrap();
        let back: Capabilities = serde_json::from_str(&json).unwrap();
        assert_eq!(back, caps);
    }

    #[test]
    fn legacy_json_without_describe_table_flag_deserializes_as_false() {
        // Pre-ADR-0028 `/capabilities` payloads do not carry the flag;
        // they must keep parsing (additive wire contract, ADR-0012).
        let json = r#"{"has_views":false,"has_functions":false,"has_auth":false,"has_storage":false,"has_realtime":false}"#;
        let caps: Capabilities = serde_json::from_str(json).unwrap();
        assert!(!caps.has_describe_table);
    }
}
