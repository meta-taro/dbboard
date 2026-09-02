//! Moving connections between machines (ADR-0038, ADR-0105).
//!
//! The bundle is encrypted with a passphrase the frontend collects and this
//! process never stores. Import reports what it refused rather than failing
//! whole: a store that is already malformed is the one that most needs the
//! parts of a backup that are still good.

use dbboard_config::ImportMode;

use crate::{lock_poisoned, AppState};

/// Export connections (entries + secrets) to a passphrase-encrypted `.dbbx`
/// bundle at `path` (ADR-0038, ADR-0105). The frontend picks `path` with the
/// native save dialog; the encrypted blob and passphrase never cross back
/// through the WebView — we write the file here. Refuses a passphrase weaker
/// than the bundle minimum before touching the keychain.
///
/// `ids` names which connections to include. An empty list is refused by the
/// config layer rather than treated as "all": the two readings of an empty
/// selection are opposites, and guessing wrong ships either an empty bundle
/// or every credential on the machine. `None` — the field absent from the
/// IPC payload — is the explicit whole-store export.
///
/// Also reports any entry in the exported selection whose keychain slot
/// belongs to a different connection (issue #194). That is a warning, not a
/// refusal: the bundle is written either way, because an operator whose store
/// is already malformed is the one who most needs a backup of it.
#[tauri::command]
pub(crate) fn export_connections(
    state: tauri::State<'_, AppState>,
    path: String,
    passphrase: String,
    ids: Option<Vec<String>>,
) -> Result<ExportReportDto, String> {
    let admin = state.admin.lock().map_err(|_| lock_poisoned())?;
    let (blob, exported, foreign) = match &ids {
        Some(ids) => (
            admin
                .export_bundle_of(ids, &passphrase)
                .map_err(|e| e.to_string())?,
            ids.len(),
            admin.foreign_refs_of(ids).map_err(|e| e.to_string())?,
        ),
        None => (
            admin
                .export_bundle(&passphrase)
                .map_err(|e| e.to_string())?,
            admin.entries().len(),
            admin.foreign_refs(),
        ),
    };
    std::fs::write(&path, &blob).map_err(|e| e.to_string())?;
    Ok(ExportReportDto {
        exported,
        foreign_refs: foreign.into_iter().map(ForeignRefDto::from).collect(),
    })
}

/// Import connections from a `.dbbx` bundle at `path` (ADR-0038, ADR-0105).
/// `overwrite` decides what an incoming id that already exists does: replace
/// the entry and its secrets, or be skipped and reported. It defaults to
/// skipping, because that is the choice that cannot lose a credential.
/// Returns the per-outcome id lists for the UI to report; the three
/// not-imported reasons stay apart all the way to the message (ADR-0112).
#[tauri::command]
pub(crate) fn import_connections(
    state: tauri::State<'_, AppState>,
    path: String,
    passphrase: String,
    overwrite: Option<bool>,
) -> Result<ImportReportDto, String> {
    let mode = if overwrite.unwrap_or(false) {
        ImportMode::Overwrite
    } else {
        ImportMode::Skip
    };
    let blob = std::fs::read(&path).map_err(|e| e.to_string())?;
    let mut admin = state.admin.lock().map_err(|_| lock_poisoned())?;
    let report = admin
        .import_bundle(&blob, &passphrase, mode)
        .map_err(|e| e.to_string())?;
    Ok(ImportReportDto {
        imported: report.imported,
        overwritten: report.overwritten,
        skipped_existing: report.skipped_existing,
        duplicate_in_bundle: report.duplicate_in_bundle,
        refused: report
            .refused
            .into_iter()
            .map(RefusedEntryDto::from)
            .collect(),
    })
}

/// Write a UTF-8 text file to `path` (ADR-0035 result-set export). The
/// frontend builds the delimited body (with its leading BOM for the `.csv`
/// form) and picks `path` with the native save dialog, so this is a thin,
/// deliberate writer — nothing here is fabricated and the path is always a
/// destination the user just chose. Kept in Rust (rather than a WebView blob
/// download) so the save lands at the chosen path with a real "Save As"
/// dialog, mirroring the connection-bundle export.
#[tauri::command]
pub(crate) fn save_text_file(path: String, contents: String) -> Result<(), String> {
    std::fs::write(&path, contents.as_bytes()).map_err(|e| e.to_string())
}

/// Outcome of `export_connections`. The count alone used to be the whole
/// return value; the warning list rides alongside it so a successful export
/// can still say something is wrong with what it just wrote (issue #194).
#[derive(serde::Serialize)]
pub(crate) struct ExportReportDto {
    exported: usize,
    foreign_refs: Vec<ForeignRefDto>,
}

/// Serialize-only mirror of `dbboard_config::ForeignRef`. Like
/// `RefusedEntryDto`, every field is an id or a keychain slot name, never a
/// secret value, so it is safe to show verbatim.
#[derive(serde::Serialize)]
pub(crate) struct ForeignRefDto {
    id: String,
    key_ref: String,
    owner: String,
}

impl From<dbboard_config::ForeignRef> for ForeignRefDto {
    fn from(r: dbboard_config::ForeignRef) -> Self {
        Self {
            id: r.id,
            key_ref: r.key_ref,
            owner: r.owner,
        }
    }
}

/// Serialize-only mirror of `dbboard_config::ImportReport` (which is
/// Deserialize-oriented internally) so the frontend gets a stable JSON shape.
#[derive(serde::Serialize)]
pub(crate) struct ImportReportDto {
    imported: Vec<String>,
    overwritten: Vec<String>,
    skipped_existing: Vec<String>,
    duplicate_in_bundle: Vec<String>,
    refused: Vec<RefusedEntryDto>,
}

/// Serialize-only mirror of `dbboard_config::RefusedEntry` (ADR-0112). All
/// three fields are connection ids or keychain slot names, never a secret
/// value, so this is safe to put in front of the user verbatim.
#[derive(serde::Serialize)]
pub(crate) struct RefusedEntryDto {
    id: String,
    key_ref: String,
    owner: String,
}

impl From<dbboard_config::RefusedEntry> for RefusedEntryDto {
    fn from(r: dbboard_config::RefusedEntry) -> Self {
        Self {
            id: r.id,
            key_ref: r.key_ref,
            owner: r.owner,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::input::{to_add_draft, KindInput, MarkInput};
    use super::super::testing::admin_over_temp;
    use super::*;
    #[test]
    fn export_then_import_roundtrips_through_a_file() {
        // Mirrors the export_connections/import_connections command bodies:
        // encrypt one store to a `.dbbx` file, import it into a fresh store,
        // and confirm the entry crosses over intact.
        let (src_dir, mut src) = admin_over_temp();
        src.add(to_add_draft(
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
        .expect("seed source");

        let passphrase = "correct horse battery staple";
        let blob = src.export_bundle(passphrase).expect("export");
        let bundle_path = src_dir.path().join("bundle.dbbx");
        std::fs::write(&bundle_path, &blob).expect("write bundle");

        let (_dst_dir, mut dst) = admin_over_temp();
        let disk = std::fs::read(&bundle_path).expect("read bundle");
        let report = dst
            .import_bundle(&disk, passphrase, ImportMode::Skip)
            .expect("import");
        assert_eq!(report.imported, vec!["t".to_string()]);
        assert!(report.overwritten.is_empty());
        assert!(report.skipped_existing.is_empty());
        assert_eq!(dst.entries().len(), 1);
    }

    #[test]
    fn import_report_dto_keeps_its_frontend_json_shape() {
        let dto = ImportReportDto {
            imported: vec!["a".to_string()],
            overwritten: vec!["c".to_string()],
            skipped_existing: vec!["b".to_string()],
            duplicate_in_bundle: vec!["d".to_string()],
            refused: vec![RefusedEntryDto {
                id: "e".to_string(),
                key_ref: "dbboard.owner.token".to_string(),
                owner: "owner".to_string(),
            }],
        };
        let json = serde_json::to_value(&dto).expect("serialize");
        for key in [
            "imported",
            "overwritten",
            "skipped_existing",
            "duplicate_in_bundle",
            "refused",
        ] {
            assert_eq!(
                json.get(key).and_then(|v| v.as_array()).unwrap().len(),
                1,
                "{key} must reach the frontend as an array"
            );
        }
        // A refusal is only actionable with both sides of the collision, so
        // the three fields are part of the contract, not decoration.
        let refused = &json.get("refused").unwrap().as_array().unwrap()[0];
        assert_eq!(refused.get("id").unwrap(), "e");
        assert_eq!(refused.get("key_ref").unwrap(), "dbboard.owner.token");
        assert_eq!(refused.get("owner").unwrap(), "owner");
    }
}
