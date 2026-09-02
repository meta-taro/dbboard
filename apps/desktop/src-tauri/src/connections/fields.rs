//! What the edit form prefills with, and deliberately what it does not.
//!
//! Secrets are never read back out of the keyring (ADR-0016): the form leaves
//! those inputs blank, and blank means "keep the stored secret". Everything
//! here is therefore a projection of the non-secret half of a stored entry.

use dbboard_config::ConnectionKind;

use crate::{lock_poisoned, AppState};

use super::ssh::{ssh_edit_fields, SshEditFieldsDto};

/// The non-secret editable fields of one connection, so the edit form can
/// prefill without ever reading a secret back out of the keyring (ADR-0016).
/// Secret fields (D1 token, the Postgres-family URL) are intentionally
/// absent — the form leaves them blank, meaning "keep the stored secret".
/// The `kind` discriminator is snake_case to match the frontend's draft
/// model (and `AuroraDsql` → `aurora_dsql`).
#[derive(serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum EditFieldsDto {
    Turso {
        path: String,
    },
    /// The URL comes back, the auth token does not (ADR-0111) — the same split
    /// D1 makes, so the endpoint can be corrected without retyping the
    /// credential.
    TursoRemote {
        url: String,
    },
    D1 {
        account_id: String,
        database_id: String,
        base_url: Option<String>,
    },
    Postgres {},
    #[serde(rename = "mysql")]
    MySql {},
    Neon {},
    Supabase {},
    AuroraDsql {},
    /// Aurora DSQL with IAM auth (ADR-0103). All five plain fields come back:
    /// the AWS *access key id* is an identifier, not a credential — it is
    /// already in `connections.toml` in the clear — and the form cannot let the
    /// operator rotate it without showing which one is stored.
    AuroraDsqlIam {
        endpoint: String,
        region: String,
        database: String,
        username: String,
        access_key_id: String,
    },
    /// `use_emulator` is the read-back of "no stored credential" (ADR-0093).
    /// It is not a secret — it is which mode the connection is in — so unlike
    /// the service-account JSON it can be sent to the form, which needs it to
    /// open with the right box shown.
    Firestore {
        project_id: String,
        database_id: Option<String>,
        base_url: Option<String>,
        use_emulator: bool,
    },
    /// The URI is absent for the usual reason — it is the secret (ADR-0096).
    /// Only the explicit database name, which the TOML stores in the clear,
    /// comes back.
    #[serde(rename = "mongodb")]
    MongoDb {
        database: Option<String>,
    },
}

/// The non-secret parts of a stored DSN, so the edit form can offer the same
/// host/port/user/database inputs the add form does (ADR-0080).
///
/// There is deliberately no password field: the whole point of this DTO is
/// that the edit form can be structured *without* the credential ever
/// reaching the webview.
#[derive(serde::Serialize)]
pub(crate) struct DsnPartsDto {
    host: String,
    port: Option<u16>,
    user: String,
    database: String,
    /// The stored query string minus its `?`, so a `ssl-mode` the user chose
    /// earlier is still what the TLS select shows when they reopen the form.
    query: String,
}

/// The edit-form prefill payload: the kind's non-secret fields (flattened so
/// the `kind` discriminator sits at the top level, unchanged) plus the tunnel
/// block when one is configured, plus the DSN parts for URL-bearing kinds.
///
/// `dsn` is `None` both for kinds that store no DSN and when the stored one
/// could not be read or parsed; the form then opens its parts empty rather
/// than refusing to open.
#[derive(serde::Serialize)]
pub(crate) struct EditFieldsResponse {
    #[serde(flatten)]
    kind: EditFieldsDto,
    ssh: Option<SshEditFieldsDto>,
    dsn: Option<DsnPartsDto>,
    /// Whether the MCP server may write to this connection (ADR-0087). Not a
    /// secret — it is a permission the operator granted — so unlike the DSN
    /// password it can be read back and shown as the toggle's current state.
    mcp_write: bool,
    /// The agent-facing alias, or `None` when this connection has none
    /// (ADR-0088). Sent back so the form opens with the stored alias in the
    /// box: an alias input that always opened blank would send `Some("")` on
    /// the next save and silently drop the alias the operator set.
    mcp_alias: Option<String>,
    /// The identity colour, or `None` when the connection is unmarked (issue
    /// #192). Sent back for the same reason as the alias: a picker that always
    /// opened on "no colour" would clear the mark on the next save.
    color: Option<String>,
    /// The identity tag, or `None` when the connection is untagged (ADR-0126).
    /// Sent back for the same reason as the colour.
    tag: Option<String>,
}

/// Read the non-secret editable fields for `id` so the edit form can prefill.
#[tauri::command]
pub(crate) fn connection_edit_fields(
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<EditFieldsResponse, String> {
    let admin = state.admin.lock().map_err(|_| lock_poisoned())?;
    let entry = admin
        .entries()
        .iter()
        .find(|e| e.id == id)
        .ok_or_else(|| format!("no connection with id \"{id}\""))?;
    let ssh = entry.ssh.as_ref().map(ssh_edit_fields);
    let mcp_write = entry.mcp_write;
    let mcp_alias = entry.mcp_alias.clone();
    let color = entry.color.clone();
    let tag = entry.tag.clone();
    let dto = match &entry.kind {
        ConnectionKind::Turso { path } => EditFieldsDto::Turso { path: path.clone() },
        ConnectionKind::TursoRemote { url, .. } => EditFieldsDto::TursoRemote { url: url.clone() },
        ConnectionKind::D1 {
            account_id,
            database_id,
            base_url,
            ..
        } => EditFieldsDto::D1 {
            account_id: account_id.clone(),
            database_id: database_id.clone(),
            base_url: base_url.clone(),
        },
        ConnectionKind::Postgres { .. } => EditFieldsDto::Postgres {},
        ConnectionKind::MySql { .. } => EditFieldsDto::MySql {},
        ConnectionKind::Neon { .. } => EditFieldsDto::Neon {},
        ConnectionKind::Supabase { .. } => EditFieldsDto::Supabase {},
        ConnectionKind::AuroraDsql { .. } => EditFieldsDto::AuroraDsql {},
        ConnectionKind::Firestore {
            project_id,
            database_id,
            base_url,
            keyring_service_account_ref,
        } => EditFieldsDto::Firestore {
            project_id: project_id.clone(),
            database_id: database_id.clone(),
            base_url: base_url.clone(),
            use_emulator: keyring_service_account_ref.is_none(),
        },
        ConnectionKind::MongoDb { database, .. } => EditFieldsDto::MongoDb {
            database: database.clone(),
        },
        ConnectionKind::AuroraDsqlIam {
            endpoint,
            region,
            database,
            username,
            access_key_id,
            ..
        } => EditFieldsDto::AuroraDsqlIam {
            endpoint: endpoint.clone(),
            region: region.clone(),
            database: database.clone(),
            username: username.clone(),
            access_key_id: access_key_id.clone(),
        },
    };
    let dsn = admin
        .dsn_prefill(&id)
        .map_err(|e| e.to_string())?
        .map(|p| DsnPartsDto {
            host: p.host,
            port: p.port,
            user: p.user,
            database: p.database,
            query: p.query,
        });
    Ok(EditFieldsResponse {
        kind: dto,
        ssh,
        dsn,
        mcp_write,
        mcp_alias,
        color,
        tag,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn edit_fields_dto_matches_the_frontend_draft_shape() {
        // The `kind` tag must be snake_case (draft.ts keys off it), and D1
        // must carry its non-secret fields but never the token.
        let turso = serde_json::to_value(EditFieldsDto::Turso {
            path: ":memory:".to_string(),
        })
        .expect("serialize turso");
        assert_eq!(turso.get("kind").unwrap(), "turso");
        assert_eq!(turso.get("path").unwrap(), ":memory:");

        let d1 = serde_json::to_value(EditFieldsDto::D1 {
            account_id: "a".to_string(),
            database_id: "b".to_string(),
            base_url: None,
        })
        .expect("serialize d1");
        assert_eq!(d1.get("kind").unwrap(), "d1");
        assert!(
            d1.get("token").is_none(),
            "a secret must never be serialized"
        );

        // AuroraDsql collapses to the snake_case discriminator the form uses.
        let aurora = serde_json::to_value(EditFieldsDto::AuroraDsql {}).expect("serialize aurora");
        assert_eq!(aurora.get("kind").unwrap(), "aurora_dsql");
    }
}
