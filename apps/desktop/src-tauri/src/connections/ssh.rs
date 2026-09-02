//! SSH tunnel fields, both directions (ADR-0069).
//!
//! The tunnel fronts a URL-bearing connection; the forward target (the DB
//! `host:port`) is parsed from the connection URL, never stored here, so these
//! DTOs only carry the bastion coordinates, auth, and host-key policy. Auth and
//! host-key are tagged unions so exactly one variant's fields ever arrive —
//! mirroring the config layer's "exactly one auth / exactly one host-key policy"
//! invariant. Host-key verification is mandatory: there is no "accept any".
//!
//! The prefill projection lives here too, so both halves of the same contract
//! stay in one file.

use dbboard_config::{
    SshAuthDraft, SshAuthEditDraft, SshEditField, SshHostKeyDraft, SshPassphraseField,
    SshTunnelDraft, SshTunnelEditDraft, SshTunnelToml,
};

use crate::none_if_blank;

use super::input::secret_field;

pub(crate) fn default_ssh_port() -> u16 {
    22
}

/// Add-time SSH auth. Secrets arrive inline (seeded into the keyring by the
/// admin layer); an absent/blank `passphrase` means the key is unencrypted.
#[derive(serde::Deserialize)]
#[serde(tag = "method", rename_all = "snake_case")]
pub(crate) enum SshAuthInput {
    Key {
        key_path: String,
        passphrase: Option<String>,
    },
    Password {
        password: String,
    },
}

/// Host-key verification policy. Exactly one variant; both the add and edit
/// paths reuse it because a fingerprint / known_hosts path is not a secret.
#[derive(serde::Deserialize)]
#[serde(tag = "policy", rename_all = "snake_case")]
pub(crate) enum SshHostKeyInput {
    Fingerprint { fingerprint: String },
    KnownHosts { known_hosts: String },
}

/// Add-time SSH tunnel, as the connection form submits it.
#[derive(serde::Deserialize)]
pub(crate) struct SshInput {
    host: String,
    #[serde(default = "default_ssh_port")]
    port: u16,
    user: String,
    auth: SshAuthInput,
    host_key: SshHostKeyInput,
}

/// Edit-time SSH auth. Secrets are keep-or-overwrite (a blank keeps the stored
/// one); `encrypted: false` on a key means "unencrypted", distinct from "keep".
#[derive(serde::Deserialize)]
#[serde(tag = "method", rename_all = "snake_case")]
pub(crate) enum SshAuthEditInput {
    Key {
        key_path: String,
        encrypted: bool,
        passphrase: Option<String>,
    },
    Password {
        password: Option<String>,
    },
}

/// Edit-time SSH intent. `keep` leaves the stored tunnel untouched, `disable`
/// removes it (secrets purged), `set` replaces it. The desktop form always
/// knows the toggle state, so it sends `disable`/`set` explicitly; `keep`
/// exists for callers with no tunnel UI.
#[derive(serde::Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub(crate) enum SshEditInput {
    Keep,
    Disable,
    Set {
        host: String,
        #[serde(default = "default_ssh_port")]
        port: u16,
        user: String,
        auth: SshAuthEditInput,
        host_key: SshHostKeyInput,
    },
}

pub(crate) fn to_host_key(host_key: SshHostKeyInput) -> SshHostKeyDraft {
    match host_key {
        SshHostKeyInput::Fingerprint { fingerprint } => SshHostKeyDraft::Fingerprint(fingerprint),
        SshHostKeyInput::KnownHosts { known_hosts } => SshHostKeyDraft::KnownHosts(known_hosts),
    }
}

pub(crate) fn to_ssh_draft(ssh: SshInput) -> SshTunnelDraft {
    let auth = match ssh.auth {
        SshAuthInput::Key {
            key_path,
            passphrase,
        } => SshAuthDraft::Key {
            key_path,
            // An unencrypted key seeds no passphrase secret.
            passphrase: none_if_blank(passphrase),
        },
        SshAuthInput::Password { password } => SshAuthDraft::Password(password),
    };
    SshTunnelDraft {
        host: ssh.host,
        port: ssh.port,
        user: ssh.user,
        auth,
        host_key: to_host_key(ssh.host_key),
    }
}

pub(crate) fn to_ssh_edit_field(ssh: SshEditInput) -> SshEditField {
    let (host, port, user, auth, host_key) = match ssh {
        SshEditInput::Keep => return SshEditField::Keep,
        SshEditInput::Disable => return SshEditField::Disable,
        SshEditInput::Set {
            host,
            port,
            user,
            auth,
            host_key,
        } => (host, port, user, auth, host_key),
    };
    let auth = match auth {
        SshAuthEditInput::Key {
            key_path,
            encrypted,
            passphrase,
        } => SshAuthEditDraft::Key {
            key_path,
            passphrase: if encrypted {
                // Encrypted key: blank input keeps the stored passphrase.
                match passphrase {
                    Some(s) if !s.trim().is_empty() => SshPassphraseField::Set(s),
                    _ => SshPassphraseField::Keep,
                }
            } else {
                SshPassphraseField::Unencrypted
            },
        },
        SshAuthEditInput::Password { password } => {
            SshAuthEditDraft::Password(secret_field(password))
        }
    };
    SshEditField::Set(SshTunnelEditDraft {
        host,
        port,
        user,
        auth,
        host_key: to_host_key(host_key),
    })
}

/// Non-secret SSH auth prefill. The passphrase/password secrets are never sent
/// back (ADR-0016); `encrypted` tells the form whether a stored passphrase
/// exists so it can render "encrypted key, leave blank to keep" vs "unencrypted".
#[derive(serde::Serialize)]
#[serde(tag = "method", rename_all = "snake_case")]
pub(crate) enum SshAuthFieldsDto {
    Key { key_path: String, encrypted: bool },
    Password {},
}

/// Non-secret host-key policy prefill.
#[derive(serde::Serialize)]
#[serde(tag = "policy", rename_all = "snake_case")]
pub(crate) enum SshHostKeyFieldsDto {
    Fingerprint { fingerprint: String },
    KnownHosts { known_hosts: String },
}

/// Non-secret SSH tunnel prefill for the edit form (ADR-0069).
#[derive(serde::Serialize)]
pub(crate) struct SshEditFieldsDto {
    host: String,
    port: u16,
    user: String,
    auth: SshAuthFieldsDto,
    host_key: SshHostKeyFieldsDto,
}

/// Project a stored [`dbboard_config::SshTunnelToml`] into its non-secret
/// prefill DTO. Auth method is inferred from which slot is populated (the
/// config layer guarantees exactly one), matching its own `validate()`.
pub(crate) fn ssh_edit_fields(ssh: &SshTunnelToml) -> SshEditFieldsDto {
    let auth = if let Some(key_path) = &ssh.key_path {
        SshAuthFieldsDto::Key {
            key_path: key_path.clone(),
            encrypted: ssh.keyring_passphrase_ref.is_some(),
        }
    } else {
        SshAuthFieldsDto::Password {}
    };
    let host_key = if let Some(fingerprint) = &ssh.fingerprint {
        SshHostKeyFieldsDto::Fingerprint {
            fingerprint: fingerprint.clone(),
        }
    } else {
        SshHostKeyFieldsDto::KnownHosts {
            // `validate()` guarantees a policy is set, so the else-arm implies
            // known_hosts; default to empty only to avoid an unwrap.
            known_hosts: ssh.known_hosts.clone().unwrap_or_default(),
        }
    };
    SshEditFieldsDto {
        host: ssh.host.clone(),
        port: ssh.port,
        user: ssh.user.clone(),
        auth,
        host_key,
    }
}

#[cfg(test)]
mod tests {
    //! Both directions of the tunnel contract: what the form sends, and what
    //! it is prefilled with.
    use super::*;
    #[test]
    fn ssh_input_deserializes_the_frontend_key_auth_contract() {
        // Locks the JSON the Svelte form sends: tagged `auth.method` and
        // `host_key.policy`, with `port` optional (defaults to 22).
        let json = serde_json::json!({
            "host": "bastion.example",
            "user": "deploy",
            "auth": { "method": "key", "key_path": "/home/deploy/.ssh/id_ed25519", "passphrase": "unlock" },
            "host_key": { "policy": "fingerprint", "fingerprint": "SHA256:abc" }
        });
        let input: SshInput = serde_json::from_value(json).expect("deserialize ssh input");
        assert_eq!(input.port, 22, "omitted port defaults to 22");
        let draft = to_ssh_draft(input);
        assert_eq!(draft.host, "bastion.example");
        match draft.auth {
            SshAuthDraft::Key {
                key_path,
                passphrase,
            } => {
                assert_eq!(key_path, "/home/deploy/.ssh/id_ed25519");
                assert_eq!(passphrase.as_deref(), Some("unlock"));
            }
            SshAuthDraft::Password(_) => panic!("expected key auth"),
        }
        assert!(matches!(draft.host_key, SshHostKeyDraft::Fingerprint(f) if f == "SHA256:abc"));
    }

    #[test]
    fn to_ssh_draft_treats_a_blank_passphrase_as_an_unencrypted_key() {
        let input = SshInput {
            host: "b".to_string(),
            port: 2222,
            user: "u".to_string(),
            auth: SshAuthInput::Key {
                key_path: "/k".to_string(),
                passphrase: Some("   ".to_string()),
            },
            host_key: SshHostKeyInput::KnownHosts {
                known_hosts: "/kh".to_string(),
            },
        };
        match to_ssh_draft(input).auth {
            SshAuthDraft::Key { passphrase, .. } => {
                assert!(passphrase.is_none(), "blank passphrase → unencrypted key");
            }
            SshAuthDraft::Password(_) => panic!("expected key auth"),
        }
    }

    #[test]
    fn to_ssh_edit_field_maps_the_three_intents() {
        assert!(matches!(
            to_ssh_edit_field(SshEditInput::Keep),
            SshEditField::Keep
        ));
        assert!(matches!(
            to_ssh_edit_field(SshEditInput::Disable),
            SshEditField::Disable
        ));
    }

    #[test]
    fn to_ssh_edit_field_encrypted_key_keeps_on_blank_and_sets_otherwise() {
        let mk = |passphrase: Option<&str>| SshEditInput::Set {
            host: "b".to_string(),
            port: 22,
            user: "u".to_string(),
            auth: SshAuthEditInput::Key {
                key_path: "/k".to_string(),
                encrypted: true,
                passphrase: passphrase.map(str::to_string),
            },
            host_key: SshHostKeyInput::Fingerprint {
                fingerprint: "SHA256:x".to_string(),
            },
        };
        let keep = to_ssh_edit_field(mk(None));
        match keep {
            SshEditField::Set(d) => match d.auth {
                SshAuthEditDraft::Key { passphrase, .. } => {
                    assert!(matches!(passphrase, SshPassphraseField::Keep));
                }
                SshAuthEditDraft::Password(_) => panic!("expected key auth"),
            },
            _ => panic!("expected Set"),
        }
        let set = to_ssh_edit_field(mk(Some("new-pass")));
        match set {
            SshEditField::Set(d) => match d.auth {
                SshAuthEditDraft::Key { passphrase, .. } => {
                    assert!(matches!(passphrase, SshPassphraseField::Set(v) if v == "new-pass"));
                }
                SshAuthEditDraft::Password(_) => panic!("expected key auth"),
            },
            _ => panic!("expected Set"),
        }
    }

    #[test]
    fn to_ssh_edit_field_unencrypted_key_drops_the_passphrase() {
        let input = SshEditInput::Set {
            host: "b".to_string(),
            port: 22,
            user: "u".to_string(),
            auth: SshAuthEditInput::Key {
                key_path: "/k".to_string(),
                encrypted: false,
                // Even a supplied value is ignored when the key is unencrypted.
                passphrase: Some("ignored".to_string()),
            },
            host_key: SshHostKeyInput::Fingerprint {
                fingerprint: "SHA256:x".to_string(),
            },
        };
        match to_ssh_edit_field(input) {
            SshEditField::Set(d) => match d.auth {
                SshAuthEditDraft::Key { passphrase, .. } => {
                    assert!(matches!(passphrase, SshPassphraseField::Unencrypted));
                }
                SshAuthEditDraft::Password(_) => panic!("expected key auth"),
            },
            _ => panic!("expected Set"),
        }
    }

    #[test]
    fn ssh_edit_fields_projects_a_stored_block_without_secrets() {
        // A password-auth tunnel with a known_hosts policy. The prefill DTO
        // must carry the coordinates but never a secret value.
        let toml = SshTunnelToml {
            host: "bastion.example".to_string(),
            port: 2222,
            user: "deploy".to_string(),
            key_path: None,
            keyring_passphrase_ref: None,
            keyring_password_ref: Some("dbboard.x.ssh_password".to_string()),
            fingerprint: None,
            known_hosts: Some("/home/deploy/.ssh/known_hosts".to_string()),
        };
        let dto = ssh_edit_fields(&toml);
        assert_eq!(dto.host, "bastion.example");
        assert_eq!(dto.port, 2222);
        assert!(matches!(dto.auth, SshAuthFieldsDto::Password {}));
        let json = serde_json::to_value(&dto).expect("serialize prefill");
        let s = json.to_string();
        assert!(
            !s.contains("ssh_password") && !s.contains("keyring"),
            "prefill must not leak secret refs: {s}"
        );
        assert_eq!(json["host_key"]["policy"], "known_hosts");
    }

    #[test]
    fn ssh_edit_fields_reports_an_encrypted_key() {
        let toml = SshTunnelToml {
            host: "b".to_string(),
            port: 22,
            user: "u".to_string(),
            key_path: Some("/home/u/.ssh/id".to_string()),
            keyring_passphrase_ref: Some("dbboard.x.ssh_passphrase".to_string()),
            keyring_password_ref: None,
            fingerprint: Some("SHA256:abc".to_string()),
            known_hosts: None,
        };
        match ssh_edit_fields(&toml).auth {
            SshAuthFieldsDto::Key {
                key_path,
                encrypted,
            } => {
                assert_eq!(key_path, "/home/u/.ssh/id");
                assert!(encrypted, "a passphrase ref means the key is encrypted");
            }
            SshAuthFieldsDto::Password {} => panic!("expected key auth"),
        }
    }
}
