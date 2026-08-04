# 0017: aurora-dsql-iam connections are editable only by hand-editing TOML

- **Status**: open
- **Opened**: 2026-07-31
- **Owner**: unassigned
- **Related ADRs**: ADR-0036 (Aurora DSQL IAM), ADR-0074 (TOML-only kinds
  disabled in the list), ADR-0076 (`connections.toml` path shown on the row)

## Problem

Selecting an `aurora-dsql-iam` connection in the desktop connection manager
shows a disabled Edit button and the note "configured in `connections.toml` —
edit it there", followed by the file's resolved path.

That tells the user *why* and *where*, but the only way to actually change
anything is to open the TOML in a text editor. For the Aurora DSQL deployment
that matters here, the person who would need to change it is **not** the
maintainer and is running the app unattended — hand-editing a TOML file is not
an answer for them. The concrete trigger is AWS access-key rotation: when the
key changes, the connection dies and there is no in-app path to fix it.

## Why it is like this

`dbboard-config`'s `EditableKind::AuroraDsqlIam` (`crates/dbboard-config/src/admin.rs`)
is deliberately a **fieldless** variant. Its doc comment says so: the kind is
config-file-only in v1, the variant exists only to keep the edit state machine
total, and any `update()` targeting it falls through `apply_update_kind`'s
catch-all as `ConfigError::KindMismatch`. `update_aurora_dsql_iam_kind_is_rejected_as_mismatch`
locks that behaviour in.

This was a v1 scope decision, not a technical obstacle. The kind carries six
fields (`crates/dbboard-config/src/store.rs`):

| Field | Secret? |
|---|---|
| `endpoint` | no |
| `region` | no |
| `database` | no |
| `username` | no |
| `access_key_id` | no |
| `keyring_secret_key_ref` | **yes** — AWS secret access key, via the keyring |

Five plain strings and one keyring-backed secret. The existing form already
handles exactly that shape for other kinds (D1 has three plain fields plus a
token).

## Scope of the fix

1. `EditableKind::AuroraDsqlIam` gains the five plain fields plus a
   `SecretField` for the secret access key.
2. `apply_update_kind` grows a real branch instead of falling through to
   `KindMismatch`; `update_aurora_dsql_iam_kind_is_rejected_as_mismatch` is
   replaced by tests asserting the update applies, and that leaving the secret
   blank means `SecretField::Keep`.
3. `delete_aurora_dsql_iam_purges_the_secret_key_ref` must keep passing.
4. Desktop: `connection_edit_fields` returns the field list; add/update
   commands accept the kind.
5. Form: a new `aurora_dsql_iam` entry in `ConnectionKind` (underscored
   namespace) with field list, validation, and i18n labels for all six.
6. `TOML_ONLY_KIND_SLUGS` in `apps/desktop/src/lib/connections/draft.ts`
   becomes empty — and `isEditableInApp` should stay, with its tests, because
   the next config-file-only kind will want it.

## Not doing yet, and why

Deferred on 2026-07-31 in favour of the #42 MySQL-over-SSH-tunnel live check,
which is what the queued internal use cases actually need. Revisit before the
next AWS key rotation, whichever comes first.
