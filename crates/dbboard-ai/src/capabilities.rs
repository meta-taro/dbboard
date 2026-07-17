//! Per-provider capability flags (ADR-0023 Decision 2).
//!
//! Mirrors the shape of `dbboard_core::Capabilities`: a flat,
//! `Copy`-able struct of independent booleans, defaulting to all-false.
//! Stage 1 ships with two flags reserved for the most obvious Stage 2
//! capabilities; new flags are added one-per-capability as additive
//! changes when those capabilities are introduced.

use serde::{Deserialize, Serialize};

// Same rationale as `dbboard_core::Capabilities`: these flags are
// independent, the shape is fixed by ADR-0023's "additive recipe",
// and collapsing them into an enum would break the extension model.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiCapabilities {
    pub has_streaming: bool,
    pub has_function_calling: bool,
}

#[cfg(test)]
mod tests {
    use super::AiCapabilities;

    #[test]
    fn default_capabilities_are_all_false() {
        let caps = AiCapabilities::default();
        assert!(!caps.has_streaming);
        assert!(!caps.has_function_calling);
    }

    #[test]
    fn each_flag_can_be_set_independently() {
        let caps = AiCapabilities {
            has_streaming: true,
            ..AiCapabilities::default()
        };
        assert!(caps.has_streaming);
        assert!(!caps.has_function_calling);
    }

    #[test]
    fn capabilities_are_copy() {
        let caps = AiCapabilities {
            has_streaming: true,
            ..AiCapabilities::default()
        };
        let copy = caps;
        assert!(caps.has_streaming);
        assert!(copy.has_streaming);
    }

    #[test]
    fn serializes_as_a_flat_snake_case_object() {
        let caps = AiCapabilities {
            has_streaming: true,
            has_function_calling: false,
        };
        let json = serde_json::to_string(&caps).unwrap();
        assert_eq!(
            json,
            r#"{"has_streaming":true,"has_function_calling":false}"#
        );
    }

    #[test]
    fn capabilities_round_trip_through_json() {
        let caps = AiCapabilities {
            has_streaming: true,
            has_function_calling: true,
        };
        let json = serde_json::to_string(&caps).unwrap();
        let back: AiCapabilities = serde_json::from_str(&json).unwrap();
        assert_eq!(back, caps);
    }
}
