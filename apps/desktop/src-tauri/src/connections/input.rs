//! What the connection form submits (write path, ADR-0062).
//!
//! The frontend speaks these Deserialize DTOs; we map them to the
//! `dbboard-config` draft types so the Svelte contract stays decoupled from
//! the crate's internal enums. `#[serde(tag = "kind")]` matches the same
//! `kind` discriminator the read-side `ConnectionView` already carries.

use dbboard_config::{
    ConnectionDraft, ConnectionEditDraft, ConnectionKindDraft, ConnectionKindEditDraft,
    FirestoreCredentialField, SecretField,
};

use crate::none_if_blank;

use super::ssh::{to_ssh_draft, to_ssh_edit_field, SshEditInput, SshInput};

/// Add-time kind + inline secret, as the connection form submits it.
#[derive(serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum KindInput {
    Turso {
        path: String,
    },
    /// Turso Cloud / any networked libSQL endpoint (ADR-0111). The tag is
    /// `turso_remote`, not the TOML's `turso-remote`: this DTO's contract is
    /// with the frontend, which is snake_case throughout (`aurora_dsql_iam`
    /// here vs `aurora-dsql-iam` on disk).
    TursoRemote {
        url: String,
        token: String,
    },
    D1 {
        account_id: String,
        database_id: String,
        base_url: Option<String>,
        token: String,
    },
    Postgres {
        url: String,
    },
    // snake_case would emit `my_sql`; pin the tag to `mysql` to match the
    // frontend draft and `ConnectionKind::MySql`'s discriminator (ADR-0068).
    #[serde(rename = "mysql")]
    MySql {
        url: String,
    },
    Neon {
        url: String,
    },
    Supabase {
        url: String,
    },
    AuroraDsql {
        url: String,
    },
    /// Aurora DSQL with IAM auth (ADR-0036, ADR-0103). No URL: the five plain
    /// fields are what a SigV4 token is minted from at connect time, and only
    /// the AWS secret access key is a secret.
    AuroraDsqlIam {
        endpoint: String,
        region: String,
        database: String,
        username: String,
        access_key_id: String,
        secret_access_key: String,
    },
    /// Firestore (ADR-0093). A blank `service_account` means the local
    /// emulator, which has no credential — not an empty secret.
    Firestore {
        project_id: String,
        database_id: Option<String>,
        base_url: Option<String>,
        service_account: Option<String>,
    },
    // snake_case would emit `mongo_db`; pin the tag to `mongodb` to match
    // `ConnectionKind::MongoDb`'s discriminator (ADR-0096).
    #[serde(rename = "mongodb")]
    MongoDb {
        /// The whole URI is the secret — the password rides in its authority —
        /// so it is submitted as one field rather than host/user/password parts.
        uri: String,
        database: Option<String>,
    },
}

/// Edit-time kind. Secret fields are `Option`: absent or blank means
/// "keep the stored secret" (the existing value is never sent back to the
/// UI, ADR-0016); a non-blank value replaces it.
#[derive(serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum KindEditInput {
    Turso {
        path: String,
    },
    TursoRemote {
        url: String,
        /// Blank keeps the stored token, so the endpoint can be changed
        /// without retyping the credential.
        token: Option<String>,
    },
    D1 {
        account_id: String,
        database_id: String,
        base_url: Option<String>,
        token: Option<String>,
    },
    Postgres {
        url: Option<String>,
    },
    #[serde(rename = "mysql")]
    MySql {
        url: Option<String>,
    },
    Neon {
        url: Option<String>,
    },
    Supabase {
        url: Option<String>,
    },
    AuroraDsql {
        url: Option<String>,
    },
    /// Aurora DSQL with IAM auth (ADR-0103). Two states for the secret access
    /// key, as for D1's token: a blank box keeps the stored one, which is what
    /// makes rotating the *access key id* alone possible.
    AuroraDsqlIam {
        endpoint: String,
        region: String,
        database: String,
        username: String,
        access_key_id: String,
        secret_access_key: Option<String>,
    },
    /// Firestore (ADR-0093). Three states, like the SSH passphrase:
    /// `use_emulator` drops the credential outright, otherwise a blank
    /// `service_account` keeps the stored one and a non-blank one replaces it.
    Firestore {
        project_id: String,
        database_id: Option<String>,
        base_url: Option<String>,
        use_emulator: bool,
        service_account: Option<String>,
    },
    /// `MongoDB` (ADR-0096). Two states, not Firestore's three: a MongoDB
    /// connection always has a URI, so there is no "drop the credential" mode.
    #[serde(rename = "mongodb")]
    MongoDb {
        uri: Option<String>,
        database: Option<String>,
    },
}

/// A blank secret input means "leave the keyring entry alone"; anything
/// else overwrites it. The value is stored verbatim (never trimmed) —
/// only the blank check trims.
pub(crate) fn secret_field(v: Option<String>) -> SecretField {
    match v {
        Some(s) if !s.trim().is_empty() => SecretField::Set(s),
        _ => SecretField::Keep,
    }
}

/// The identity mark as it arrives from a caller (ADR-0126).
///
/// One argument rather than two because the halves are never meaningfully
/// separate: a colour with no tag is refused by the form and rendered as the
/// colour's own name everywhere else, so nothing downstream wants one without
/// having looked at the other.
#[derive(Debug, Default)]
pub(crate) struct MarkInput {
    pub(crate) color: Option<String>,
    pub(crate) tag: Option<String>,
}

pub(crate) fn to_add_draft(
    id: String,
    name: String,
    kind: KindInput,
    ssh: Option<SshInput>,
    mcp_write: bool,
    mcp_alias: Option<String>,
    mark: MarkInput,
) -> ConnectionDraft {
    let kind = match kind {
        KindInput::Turso { path } => ConnectionKindDraft::Turso { path },
        KindInput::TursoRemote { url, token } => ConnectionKindDraft::TursoRemote { url, token },
        KindInput::D1 {
            account_id,
            database_id,
            base_url,
            token,
        } => ConnectionKindDraft::D1 {
            account_id,
            database_id,
            base_url: none_if_blank(base_url),
            token,
        },
        KindInput::Postgres { url } => ConnectionKindDraft::Postgres { url },
        KindInput::MySql { url } => ConnectionKindDraft::MySql { url },
        KindInput::Neon { url } => ConnectionKindDraft::Neon { url },
        KindInput::Supabase { url } => ConnectionKindDraft::Supabase { url },
        KindInput::AuroraDsql { url } => ConnectionKindDraft::AuroraDsql { url },
        KindInput::AuroraDsqlIam {
            endpoint,
            region,
            database,
            username,
            access_key_id,
            secret_access_key,
        } => ConnectionKindDraft::AuroraDsqlIam {
            endpoint,
            region,
            database,
            username,
            access_key_id,
            secret_access_key,
        },
        KindInput::Firestore {
            project_id,
            database_id,
            base_url,
            service_account,
        } => ConnectionKindDraft::Firestore {
            project_id,
            database_id: none_if_blank(database_id),
            base_url: none_if_blank(base_url),
            // Blank is the emulator, so it must collapse to None rather than
            // seeding an empty-string secret that later reads as a real one.
            service_account: none_if_blank(service_account),
        },
        KindInput::MongoDb { uri, database } => ConnectionKindDraft::MongoDb {
            uri,
            // Blank means "the URI's path names it"; an empty-string database
            // would instead be written to the TOML as a real, unusable name.
            database: none_if_blank(database),
        },
    };
    ConnectionDraft {
        mcp_write,
        mcp_alias,
        color: mark.color,
        tag: mark.tag,
        id,
        name,
        kind,
        ssh: ssh.map(to_ssh_draft),
    }
}

pub(crate) fn to_edit_draft(
    name: String,
    kind: KindEditInput,
    ssh: SshEditInput,
    mcp_write: Option<bool>,
    mcp_alias: Option<String>,
    mark: MarkInput,
) -> ConnectionEditDraft {
    let kind = match kind {
        KindEditInput::Turso { path } => ConnectionKindEditDraft::Turso { path },
        KindEditInput::TursoRemote { url, token } => ConnectionKindEditDraft::TursoRemote {
            url,
            token: secret_field(token),
        },
        KindEditInput::D1 {
            account_id,
            database_id,
            base_url,
            token,
        } => ConnectionKindEditDraft::D1 {
            account_id,
            database_id,
            base_url: none_if_blank(base_url),
            token: secret_field(token),
        },
        KindEditInput::Postgres { url } => ConnectionKindEditDraft::Postgres {
            url: secret_field(url),
        },
        KindEditInput::MySql { url } => ConnectionKindEditDraft::MySql {
            url: secret_field(url),
        },
        KindEditInput::Neon { url } => ConnectionKindEditDraft::Neon {
            url: secret_field(url),
        },
        KindEditInput::Supabase { url } => ConnectionKindEditDraft::Supabase {
            url: secret_field(url),
        },
        KindEditInput::AuroraDsql { url } => ConnectionKindEditDraft::AuroraDsql {
            url: secret_field(url),
        },
        KindEditInput::AuroraDsqlIam {
            endpoint,
            region,
            database,
            username,
            access_key_id,
            secret_access_key,
        } => ConnectionKindEditDraft::AuroraDsqlIam {
            endpoint,
            region,
            database,
            username,
            access_key_id,
            secret_access_key: secret_field(secret_access_key),
        },
        KindEditInput::Firestore {
            project_id,
            database_id,
            base_url,
            use_emulator,
            service_account,
        } => ConnectionKindEditDraft::Firestore {
            project_id,
            database_id: none_if_blank(database_id),
            base_url: none_if_blank(base_url),
            service_account: if use_emulator {
                // The emulator wins over anything left in the credential box,
                // matching how an unencrypted SSH key discards a typed
                // passphrase rather than half-applying the user's choice.
                FirestoreCredentialField::Emulator
            } else {
                match service_account {
                    Some(s) if !s.trim().is_empty() => FirestoreCredentialField::Set(s),
                    _ => FirestoreCredentialField::Keep,
                }
            },
        },
        KindEditInput::MongoDb { uri, database } => ConnectionKindEditDraft::MongoDb {
            uri: secret_field(uri),
            database: none_if_blank(database),
        },
    };
    ConnectionEditDraft {
        mcp_write,
        mcp_alias,
        color: mark.color,
        tag: mark.tag,
        name,
        kind,
        ssh: to_ssh_edit_field(ssh),
    }
}

#[cfg(test)]
mod tests {
    //! The DTO-to-draft mapping is the part this crate owns: blank-handling
    //! for optional and secret fields, and the graft that keeps a stored
    //! password when the form sends a URL back with the password blanked out.
    use super::*;
    #[test]
    fn secret_field_keeps_on_blank_and_sets_verbatim_otherwise() {
        assert!(matches!(secret_field(None), SecretField::Keep));
        assert!(matches!(
            secret_field(Some("  ".to_string())),
            SecretField::Keep
        ));
        // A real secret is stored exactly as typed — surrounding spaces can
        // be significant in a URL/token, so only the blank check trims.
        match secret_field(Some(" tok ".to_string())) {
            SecretField::Set(v) => assert_eq!(v, " tok "),
            SecretField::Keep => panic!("a non-blank secret must Set, not Keep"),
        }
    }

    #[test]
    fn to_add_draft_maps_d1_and_drops_a_blank_base_url() {
        let draft = to_add_draft(
            "d".to_string(),
            "D".to_string(),
            KindInput::D1 {
                account_id: "acct".to_string(),
                database_id: "db".to_string(),
                base_url: Some("   ".to_string()),
                token: "t".to_string(),
            },
            None,
            false,
            None,
            MarkInput::default(),
        );
        assert_eq!(draft.id, "d");
        assert_eq!(draft.name, "D");
        match draft.kind {
            ConnectionKindDraft::D1 {
                account_id,
                database_id,
                base_url,
                token,
            } => {
                assert_eq!(account_id, "acct");
                assert_eq!(database_id, "db");
                assert!(base_url.is_none(), "blank base_url collapses to None");
                assert_eq!(token, "t");
            }
            _ => panic!("expected a D1 draft"),
        }
    }

    #[test]
    fn to_add_draft_maps_a_firestore_service_account() {
        let draft = to_add_draft(
            "fs".to_string(),
            "FS".to_string(),
            KindInput::Firestore {
                project_id: "demo-project".to_string(),
                database_id: Some("  ".to_string()),
                base_url: None,
                service_account: Some(r#"{"type":"service_account"}"#.to_string()),
            },
            None,
            false,
            None,
            MarkInput::default(),
        );
        match draft.kind {
            ConnectionKindDraft::Firestore {
                project_id,
                database_id,
                base_url,
                service_account,
            } => {
                assert_eq!(project_id, "demo-project");
                assert!(database_id.is_none(), "blank database_id collapses to None");
                assert!(base_url.is_none());
                assert_eq!(
                    service_account.as_deref(),
                    Some(r#"{"type":"service_account"}"#)
                );
            }
            _ => panic!("expected a Firestore draft"),
        }
    }

    #[test]
    fn to_add_draft_treats_a_blank_firestore_service_account_as_the_emulator() {
        // The form hides the credential box when "use the emulator" is on, so
        // what arrives is blank — and blank must mean "no credential", not an
        // empty-string secret written into the keychain.
        let draft = to_add_draft(
            "fs".to_string(),
            "FS".to_string(),
            KindInput::Firestore {
                project_id: "demo-project".to_string(),
                database_id: None,
                base_url: Some("http://127.0.0.1:8080/v1".to_string()),
                service_account: Some("   ".to_string()),
            },
            None,
            false,
            None,
            MarkInput::default(),
        );
        match draft.kind {
            ConnectionKindDraft::Firestore {
                base_url,
                service_account,
                ..
            } => {
                assert_eq!(base_url.as_deref(), Some("http://127.0.0.1:8080/v1"));
                assert!(service_account.is_none(), "blank → emulator");
            }
            _ => panic!("expected a Firestore draft"),
        }
    }

    #[test]
    fn to_edit_draft_firestore_maps_all_three_credential_states() {
        let with = |use_emulator: bool, service_account: Option<&str>| {
            to_edit_draft(
                "FS".to_string(),
                KindEditInput::Firestore {
                    project_id: "demo-project".to_string(),
                    database_id: None,
                    base_url: None,
                    use_emulator,
                    service_account: service_account.map(str::to_string),
                },
                SshEditInput::Keep,
                None,
                None,
                MarkInput::default(),
            )
            .kind
        };
        let keep = with(false, Some("  "));
        assert!(
            matches!(
                keep,
                ConnectionKindEditDraft::Firestore {
                    service_account: FirestoreCredentialField::Keep,
                    ..
                }
            ),
            "blank with the emulator off → keep the stored credential"
        );
        let set = with(false, Some("{\"type\":\"service_account\"}"));
        assert!(
            matches!(
                set,
                ConnectionKindEditDraft::Firestore {
                    service_account: FirestoreCredentialField::Set(v),
                    ..
                } if v == "{\"type\":\"service_account\"}"
            ),
            "a supplied value overwrites"
        );
        // Even a supplied value is ignored once the emulator is chosen —
        // the same precedence `to_ssh_edit_field` gives an unencrypted key.
        let emulator = with(true, Some("{\"type\":\"service_account\"}"));
        assert!(
            matches!(
                emulator,
                ConnectionKindEditDraft::Firestore {
                    service_account: FirestoreCredentialField::Emulator,
                    ..
                }
            ),
            "the emulator toggle wins over a typed credential"
        );
    }

    #[test]
    fn to_add_draft_maps_a_mongodb_uri() {
        let draft = to_add_draft(
            "mg".to_string(),
            "MG".to_string(),
            KindInput::MongoDb {
                uri: "mongodb://app:hunter2@127.0.0.1:27117".to_string(),
                database: Some("  ".to_string()),
            },
            None,
            false,
            None,
            MarkInput::default(),
        );
        match draft.kind {
            ConnectionKindDraft::MongoDb { uri, database } => {
                assert_eq!(uri, "mongodb://app:hunter2@127.0.0.1:27117");
                // The URI may name the database in its path, so blank means
                // "let the URI decide" — not an empty database name.
                assert!(database.is_none(), "blank database collapses to None");
            }
            _ => panic!("expected a MongoDB draft"),
        }
    }

    #[test]
    fn to_edit_draft_mongodb_maps_both_uri_states() {
        let with = |uri: Option<&str>| {
            to_edit_draft(
                "MG".to_string(),
                KindEditInput::MongoDb {
                    uri: uri.map(str::to_string),
                    database: Some("shop".to_string()),
                },
                SshEditInput::Keep,
                None,
                None,
                MarkInput::default(),
            )
            .kind
        };
        assert!(
            matches!(
                with(Some("   ")),
                ConnectionKindEditDraft::MongoDb {
                    uri: SecretField::Keep,
                    ..
                }
            ),
            "blank → keep the stored URI"
        );
        assert!(
            matches!(
                with(Some("mongodb://other:27017")),
                ConnectionKindEditDraft::MongoDb {
                    uri: SecretField::Set(v),
                    database,
                } if v == "mongodb://other:27017" && database.as_deref() == Some("shop")
            ),
            "a supplied URI overwrites"
        );
    }

    #[test]
    fn to_add_draft_carries_the_mcp_alias() {
        let with = |alias: Option<&str>| {
            to_add_draft(
                "d".to_string(),
                "D".to_string(),
                KindInput::Turso {
                    path: ":memory:".to_string(),
                },
                None,
                false,
                alias.map(str::to_string),
                MarkInput::default(),
            )
            .mcp_alias
        };
        assert_eq!(with(None), None, "no alias by default (ADR-0088)");
        assert_eq!(with(Some("store-a")), Some("store-a".to_string()));
    }

    #[test]
    fn to_edit_draft_passes_the_alias_through_unchanged() {
        // The three states of ADR-0088's edit semantics have to survive the
        // trip verbatim: the config layer, not this mapper, decides that a
        // blank string clears. Flattening `Some("")` to `None` here would turn
        // "clear the alias" into "keep it" and the alias could never be removed.
        let with = |alias: Option<&str>| {
            to_edit_draft(
                "D".to_string(),
                KindEditInput::Turso {
                    path: ":memory:".to_string(),
                },
                SshEditInput::Keep,
                None,
                alias.map(str::to_string),
                MarkInput::default(),
            )
            .mcp_alias
        };
        assert_eq!(with(None), None, "omitted → keep the stored alias");
        assert_eq!(with(Some("store-a")), Some("store-a".to_string()));
        assert_eq!(with(Some("")), Some(String::new()), "emptied → clear");
    }

    #[test]
    fn to_add_draft_carries_the_colour() {
        let with = |color: Option<&str>| {
            to_add_draft(
                "d".to_string(),
                "D".to_string(),
                KindInput::Turso {
                    path: ":memory:".to_string(),
                },
                None,
                false,
                None,
                MarkInput {
                    color: color.map(str::to_string),
                    tag: None,
                },
            )
            .color
        };
        assert_eq!(with(None), None, "unmarked by default (issue #192)");
        assert_eq!(with(Some("red")), Some("red".to_string()));
    }

    #[test]
    fn to_edit_draft_passes_the_colour_through_unchanged() {
        // Same three states as the alias, and the same reason for not
        // collapsing them here: `Some("")` is how the picker says "no colour",
        // and flattening it to `None` would mean a mark could never be removed.
        let with = |color: Option<&str>| {
            to_edit_draft(
                "D".to_string(),
                KindEditInput::Turso {
                    path: ":memory:".to_string(),
                },
                SshEditInput::Keep,
                None,
                None,
                MarkInput {
                    color: color.map(str::to_string),
                    tag: None,
                },
            )
            .color
        };
        assert_eq!(with(None), None, "omitted → keep the stored colour");
        assert_eq!(with(Some("teal")), Some("teal".to_string()));
        assert_eq!(with(Some("")), Some(String::new()), "emptied → clear");
    }
}
