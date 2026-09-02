//! Registering, editing and reordering connections (ADR-0062).
//!
//! This is the write side of `connections.toml`. Every command here takes the
//! `ConnectionAdmin` lock; the commit discipline behind it (keyring and TOML
//! rolled back together) belongs to `dbboard-config`. What lives here is the
//! wiring, plus the two things the frontend cannot get anywhere else: the
//! identity marks, and a host-key probe that never authenticates.

pub(crate) mod fields;
pub(crate) mod graft;
pub(crate) mod input;
pub(crate) mod ssh;
pub(crate) mod transfer;

use std::collections::BTreeMap;

use crate::{lock_poisoned, AppState};

use graft::graft_url;
use input::{to_add_draft, to_edit_draft, KindEditInput, KindInput, MarkInput};
use ssh::{SshEditInput, SshInput};
use transfer::ForeignRefDto;

/// The identity mark of every marked connection, keyed by id (ADR-0126).
///
/// Both halves in one call, because they are rendered together: a swatch with
/// no tag beside it is the failure mode the tag exists to prevent, and two
/// commands would let a caller fetch one without noticing the other.
///
/// Separate from [`list_connections`] rather than a field on it, because that
/// projection is the *agent's* view of a connection and is deliberately narrow:
/// a mark is for a human eye, so it never needed to cross into `dbboard-mcp`
/// (issue #0026, "Not in scope here"). Reading it straight off the admin also
/// means the list and the marks cannot disagree after an edit.
///
/// Unmarked connections are absent rather than present-with-nulls: "no key" and
/// "no mark" are the same statement, and the caller has to handle a missing id
/// anyway. A connection with only one half present *is* marked, and appears.
#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct ConnectionMark {
    color: Option<String>,
    tag: Option<String>,
}

#[tauri::command]
pub(crate) fn connection_marks(
    state: tauri::State<'_, AppState>,
) -> Result<BTreeMap<String, ConnectionMark>, String> {
    let admin = state.admin.lock().map_err(|_| lock_poisoned())?;
    Ok(admin
        .entries()
        .iter()
        .filter(|e| e.color.is_some() || e.tag.is_some())
        .map(|e| {
            (
                e.id.clone(),
                ConnectionMark {
                    color: e.color.clone(),
                    tag: e.tag.clone(),
                },
            )
        })
        .collect())
}

/// Read the SSH server's host-key fingerprint so the connection form can offer
/// it for pinning. This is the SSH client's first-connection prompt, moved into
/// the form: without it the fingerprint field is a required box with no way to
/// discover its value short of running `ssh-keyscan` by hand.
///
/// Two properties make this safe to expose. It never authenticates — the probe
/// handler captures the key and then rejects it, so no credential is sent to a
/// server whose identity is still unverified. And it never writes: the returned
/// string is filled into the form for the user to confirm and save, so pinning
/// stays a deliberate act rather than trust-on-first-use behind their back.
#[tauri::command]
pub(crate) async fn probe_ssh_host_key(host: String, port: u16) -> Result<String, String> {
    dbboard_tunnel::probe_host_key(&host, port)
        .await
        .map_err(|e| e.to_string())
}

/// Add a connection: writes the non-secret entry to `connections.toml`
/// and the secret to the OS keyring atomically (rolled back together on
/// failure). Fails with `DuplicateId` if the id is taken.
///
/// `mcp_write` defaults to closed when the caller omits it, so a form that
/// never rendered the toggle cannot grant the MCP write permission by
/// accident (ADR-0087).
///
/// `mcp_alias` is optional for the mirror-image reason (ADR-0088): omitting it
/// leaves the connection's real id and name visible to agents, which is what a
/// caller with no alias input meant.
///
/// `color` and `tag` are optional the same way (ADR-0126): a caller with no
/// picker and no tag input leaves the connection unmarked rather than guessing
/// a mark for it.
// The parameter list *is* the wire contract; see `update_connection` below for
// why it is allowed to grow rather than being folded into a payload struct.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub(crate) fn add_connection(
    state: tauri::State<'_, AppState>,
    id: String,
    name: String,
    kind: KindInput,
    ssh: Option<SshInput>,
    mcp_write: Option<bool>,
    mcp_alias: Option<String>,
    color: Option<String>,
    tag: Option<String>,
) -> Result<(), String> {
    let mut admin = state.admin.lock().map_err(|_| lock_poisoned())?;
    admin
        .add(to_add_draft(
            id,
            name,
            kind,
            ssh,
            mcp_write.unwrap_or(false),
            mcp_alias,
            MarkInput { color, tag },
        ))
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Edit an existing connection. The id and kind are immutable here (a
/// kind change is a delete + re-add); a blank secret keeps the stored
/// one. Evicts the read path's cached adapter so the next query rebuilds
/// with the new credentials.
///
/// `keep_password` is the structured-input counterpart of that blank-secret
/// rule (ADR-0080): the form rebuilt the DSN from host/port/user/database but
/// the user did not retype the password, so the stored one is grafted back on
/// here rather than being sent to the webview and back.
///
/// `mcp_write` is `Option` for the same reason (ADR-0087): omitting it keeps
/// whatever is stored, so a caller with no toggle cannot revoke a permission
/// it never showed. `mcp_alias` follows the same rule with one extra state
/// (ADR-0088): omitted keeps, a filled string sets, and an empty string — what
/// an emptied text input sends — clears the alias. `color` has the same three
/// states (issue #192), with the empty string sent by the picker's "no colour"
/// option, and `tag` the same again with the empty string sent by an emptied
/// tag input (ADR-0126).
// The parameter list *is* the wire contract: each name is a key the webview
// sends. Folding them into one payload struct would rename every key for a
// lint, so the arity is allowed to grow with the form instead.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub(crate) async fn update_connection(
    state: tauri::State<'_, AppState>,
    id: String,
    name: String,
    kind: KindEditInput,
    ssh: SshEditInput,
    keep_password: Option<bool>,
    mcp_write: Option<bool>,
    mcp_alias: Option<String>,
    color: Option<String>,
    tag: Option<String>,
) -> Result<(), String> {
    {
        let mut admin = state.admin.lock().map_err(|_| lock_poisoned())?;
        let kind = if keep_password.unwrap_or(false) {
            graft_url(kind, |url| {
                admin
                    .dsn_with_stored_password(&id, url)
                    .map_err(|e| e.to_string())
            })?
        } else {
            kind
        };
        admin
            .update(
                &id,
                to_edit_draft(
                    name,
                    kind,
                    ssh,
                    mcp_write,
                    mcp_alias,
                    MarkInput { color, tag },
                ),
            )
            .map_err(|e| e.to_string())?;
    } // drop the guard before awaiting — keeps the command future Send.
    state.service.invalidate(&id).await;
    Ok(())
}

/// Copy an existing connection into a new one that owns its own keychain
/// slots, seeded with the source's secret values (issue #213).
///
/// This exists because a ref is minted only on add: before this command the
/// only way to register a second connection sharing one credential — two D1
/// databases behind one API token, two schemas behind one Postgres URL — was
/// to hand-edit `connections.toml`, which produces exactly the state
/// `foreign_connection_refs` reports and the import guard refuses (ADR-0038).
///
/// The copy drops the MCP alias and leaves MCP writes off; see
/// [`dbboard_config::ConnectionAdmin::duplicate`] for why.
#[tauri::command]
pub(crate) fn duplicate_connection(
    state: tauri::State<'_, AppState>,
    id: String,
    new_id: String,
    new_name: String,
) -> Result<(), String> {
    let mut admin = state.admin.lock().map_err(|_| lock_poisoned())?;
    admin
        .duplicate(&id, new_id, new_name)
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Re-point one of `id`'s keyring slots — currently another connection's —
/// at a slot of its own, and store `secret` there (issue #213).
///
/// `secret` comes from the user rather than being copied out of the slot
/// being abandoned, because that value belongs to the other connection. The
/// abandoned slot is left in place for the same reason.
///
/// Evicts the cached adapter: the entry now reads its credential from a
/// different place, so a pooled adapter built from the old one is stale.
#[tauri::command]
pub(crate) async fn repair_connection_ref(
    state: tauri::State<'_, AppState>,
    id: String,
    key_ref: String,
    secret: String,
) -> Result<(), String> {
    {
        let mut admin = state.admin.lock().map_err(|_| lock_poisoned())?;
        admin
            .repair_foreign_ref(&id, &key_ref, secret)
            .map_err(|e| e.to_string())?;
    } // drop the guard before awaiting — keeps the command future Send.
    state.service.invalidate(&id).await;
    Ok(())
}

/// Every connection that points at a keychain slot minted for a different
/// connection (issue #194).
///
/// Reported to the connection list rather than only at export time (issue
/// #213): the state is worth seeing before a bundle is built, because it is
/// also why such a connection cannot be duplicated.
#[tauri::command]
pub(crate) fn foreign_connection_refs(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<ForeignRefDto>, String> {
    let admin = state.admin.lock().map_err(|_| lock_poisoned())?;
    Ok(admin
        .foreign_refs()
        .into_iter()
        .map(ForeignRefDto::from)
        .collect())
}

/// Delete a connection and purge its keyring secrets, then evict any
/// cached adapter for it.
#[tauri::command]
pub(crate) async fn delete_connection(
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<(), String> {
    {
        let mut admin = state.admin.lock().map_err(|_| lock_poisoned())?;
        admin.delete(&id).map_err(|e| e.to_string())?;
    }
    state.service.invalidate(&id).await;
    Ok(())
}

/// Move a connection to position `index` in the stored order (issue #192).
///
/// The order of `[[connections]]` is the order the sidebar and the manager
/// list render, so this is the whole of "put my connections in the order I
/// work in". No adapter is evicted: nothing about how a connection dials
/// changed, only where it sits in the list.
#[tauri::command]
pub(crate) fn move_connection(
    state: tauri::State<'_, AppState>,
    id: String,
    index: usize,
) -> Result<(), String> {
    let mut admin = state.admin.lock().map_err(|_| lock_poisoned())?;
    admin.move_to(&id, index).map_err(|e| e.to_string())
}

/// Set one connection's identity mark from the list, with no edit form
/// (ADR-0130).
///
/// [`update_connection`] can write a mark too, but only alongside the whole
/// connection — the kind, the tunnel, the MCP permissions — which means the
/// form has to be open and every secret decision re-made. Marking is the one
/// thing an operator does while looking at the sidebar, so it gets a command
/// that carries only what it changes.
///
/// Blank strings clear: `color: ""` is "no colour", `tag: ""` is "no tag", and
/// both blank leaves the connection unmarked. No adapter is evicted — nothing
/// about how the connection dials changed, only how it looks.
#[tauri::command]
pub(crate) fn set_connection_mark(
    state: tauri::State<'_, AppState>,
    id: String,
    color: String,
    tag: String,
) -> Result<(), String> {
    let mut admin = state.admin.lock().map_err(|_| lock_poisoned())?;
    admin
        .set_mark(&id, Some(color), Some(tag))
        .map_err(|e| e.to_string())
}

/// Drop the cached adapter for `id` and open a fresh connection.
///
/// The read path already re-checks an idle adapter before handing it out, so
/// this is not needed to *recover* — it is needed to recover *now*. A user
/// looking at a pane that just failed should not have to guess whether
/// clicking again will work; through an SSH bastion the dead thing is the
/// tunnel, and only dropping the adapter rebuilds the forward.
///
/// Connecting pings, so an `Ok` here means the database answered.
#[tauri::command]
pub(crate) async fn reconnect_connection(
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<(), String> {
    state
        .service
        .reconnect(&id)
        .await
        .map_err(|e| e.to_string())
}

/// Shared test fixtures. Both this module's tests and `transfer`'s drive a
/// real `ConnectionAdmin`, and they must agree on what "a throwaway store"
/// means — an in-memory keyring, so no test ever reaches the OS credential
/// store.
#[cfg(test)]
pub(crate) mod testing {
    use std::sync::Arc;

    use dbboard_config::{ConnectionAdmin, InMemorySecretStore};

    /// A `ConnectionAdmin` over a throwaway `connections.toml` paired with an
    /// in-memory keyring, so add/update/delete never touch the real OS store.
    pub(crate) fn admin_over_temp() -> (tempfile::TempDir, ConnectionAdmin) {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("connections.toml");
        let secrets = Arc::new(InMemorySecretStore::default());
        let admin = ConnectionAdmin::open(path, secrets).expect("open admin");
        (dir, admin)
    }
}

#[cfg(test)]
mod tests {
    //! The add/update/delete flow, driving a real `ConnectionAdmin` over a
    //! temp store. The commit discipline (keyring/TOML rollback) is covered by
    //! `dbboard-config`'s own suite; here we prove our wiring reaches it.
    use super::testing::admin_over_temp;
    use super::*;
    #[test]
    fn add_update_delete_flow_over_a_temp_store() {
        let (_dir, mut admin) = admin_over_temp();

        admin
            .add(to_add_draft(
                "t".to_string(),
                "Turso".to_string(),
                KindInput::Turso {
                    path: ":memory:".to_string(),
                },
                None,
                false,
                None,
                MarkInput::default(),
            ))
            .expect("add turso");
        admin
            .add(to_add_draft(
                "p".to_string(),
                "PG".to_string(),
                KindInput::Postgres {
                    url: "postgres://u:pw@h/db".to_string(),
                },
                None,
                false,
                None,
                MarkInput::default(),
            ))
            .expect("add postgres");
        assert_eq!(admin.entries().len(), 2);

        // Rename only (blank secret → Keep the stored URL). Must not error
        // for lack of a resupplied secret.
        admin
            .update(
                "p",
                to_edit_draft(
                    "PG-renamed".to_string(),
                    KindEditInput::Postgres { url: None },
                    SshEditInput::Keep,
                    None,
                    None,
                    MarkInput::default(),
                ),
            )
            .expect("rename postgres, keep secret");
        let pg = admin
            .entries()
            .iter()
            .find(|e| e.id == "p")
            .expect("postgres still present");
        assert_eq!(pg.name, "PG-renamed");

        admin.delete("t").expect("delete turso");
        let ids: Vec<&str> = admin.entries().iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, vec!["p"], "only the postgres entry survives");
    }

    #[test]
    fn add_rejects_a_duplicate_id() {
        let (_dir, mut admin) = admin_over_temp();
        let mk = || {
            to_add_draft(
                "dup".to_string(),
                "One".to_string(),
                KindInput::Turso {
                    path: ":memory:".to_string(),
                },
                None,
                false,
                None,
                MarkInput::default(),
            )
        };
        admin.add(mk()).expect("first add");
        assert!(admin.add(mk()).is_err(), "a taken id must be rejected");
    }
}
