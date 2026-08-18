//! Every per-user file dbboard owns resolves through one directory.
//!
//! Why this matters: `DBBOARD_CONFIG_DIR` exists so that dbboard can be
//! started against a config directory other than the per-user one — for a
//! screenshot, a walkthrough, or a throwaway profile that holds no real
//! credentials. Before it, there was no supported way to do that. On Windows
//! the `directories` lookup reads the known-folder API rather than `%APPDATA%`,
//! so redirecting the environment variable does nothing and the app opens the
//! real file anyway, with real host names in the connection list.
//!
//! An override that only some of the files honour would be worse than none: it
//! would put a demo profile's connections next to the real profile's query
//! history. So the property under test is *togetherness*, not the variable —
//! the semantics of the variable itself are unit-tested next to the code in
//! `store.rs`, where they can be exercised without mutating the environment
//! (`unsafe_code` is forbidden workspace-wide, and `set_var` is unsafe).

use dbboard_config::ai_store::default_ai_providers_path;
use dbboard_config::annotations::default_annotations_path;
use dbboard_config::store::{config_dir, default_history_path, default_path};
use dbboard_config::ui_command::{default_ui_command_path, default_ui_result_path};
use dbboard_config::ui_settings::default_ui_settings_path;

#[test]
fn every_config_file_resolves_inside_the_one_config_dir() {
    let dir = config_dir().expect("the config dir should resolve");

    for (path, name) in [
        (default_path().expect("connections"), "connections.toml"),
        (default_history_path().expect("history"), "history.jsonl"),
        (
            default_ai_providers_path().expect("ai providers"),
            "ai-providers.toml",
        ),
        (
            default_annotations_path().expect("annotations"),
            "annotations.toml",
        ),
        (
            default_ui_settings_path().expect("ui settings"),
            "ui-settings.toml",
        ),
        // The command channel (ADR-0109) is two processes agreeing on a
        // path. Split them across profiles and neither errors: the MCP side
        // writes into one directory, the window watches another, and every
        // command times out reporting that dbboard is not running.
        (
            default_ui_command_path().expect("ui command"),
            "ui-command.toml",
        ),
        (
            default_ui_result_path().expect("ui command result"),
            "ui-command-result.toml",
        ),
    ] {
        assert_eq!(
            path,
            dir.join(name),
            "{name} does not resolve through config_dir(), so an override would leave it behind"
        );
    }
}
