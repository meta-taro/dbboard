//! Passphrase-encrypted connection bundle (ADR-0038).
//!
//! A `connections.toml` on its own is portable but *useless* on another
//! machine: it stores only keyring *references*, and the actual secrets
//! (D1 token, Postgres URL, AWS secret key) live in the local OS
//! keychain. To hand a whole connection set to another machine — the
//! collector handoff (#14) — we need the metadata **and** the secrets in
//! one self-contained artifact, protected so it can travel over an
//! ordinary channel.
//!
//! This module is the crypto core: it turns a [`BundlePayload`]
//! (connections + resolved secrets) into an encrypted blob and back,
//! under a user-supplied passphrase. It hand-rolls no cryptography —
//! `age` (scrypt KDF + `ChaCha20-Poly1305` AEAD, authenticated envelope)
//! does the work. The plaintext is JSON and is **never** written to disk
//! in the clear; the intermediate buffer is zeroized after use.
//!
//! The orchestration on top of this (resolving every keyring reference on
//! export, seeding the keychain on import, merging into the live store)
//! lives alongside in the crate's export/import layer.

use std::io::{Read, Write};

use age::secrecy::SecretString;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use zeroize::Zeroize;

use crate::store::ConnectionFile;

/// The bundle schema version this build reads and writes.
///
/// Distinct from [`crate::store::CONFIG_VERSION`] (the `connections.toml`
/// schema) and from age's own on-the-wire format version: this is the
/// version of the JSON payload *inside* the encrypted envelope. A future
/// change to the payload shape bumps this and adds an explicit migration.
pub const BUNDLE_VERSION: u32 = 1;

/// Minimum passphrase length accepted when *creating* a bundle.
///
/// A deliberately low floor — the point is to reject an empty or
/// obviously-accidental passphrase, not to be a password-strength meter.
/// Enforced only on export ([`encrypt_bundle`]); decrypt accepts whatever
/// the bundle was made with so a bundle from another tool still opens.
pub const MIN_PASSPHRASE_LEN: usize = 8;

/// Alphabet a generated passphrase is drawn from: 32 characters with every
/// pair that is read wrong removed — no `l`/`1`, no `o`/`0`.
///
/// Exactly 32 so that five bits of entropy map onto one character with no
/// modulo bias, and so a character can be picked by masking a random byte
/// rather than by rejection sampling.
const PASSPHRASE_ALPHABET: &[u8; 32] = b"abcdefghijkmnpqrstuvwxyz23456789";

/// Characters per group in a generated passphrase, and groups per
/// passphrase: `abcde-fghij-...`, 30 characters of entropy in six runs of
/// five.
const PASSPHRASE_GROUP: usize = 5;
const PASSPHRASE_GROUPS: usize = 6;

/// Mint a passphrase nobody has chosen — 150 bits from the OS entropy
/// source, grouped for transcription.
///
/// This exists for the one caller that must produce a bundle *without* a
/// human present to think of a passphrase: the MCP export verb (ADR-0140),
/// which seals a bundle and puts the passphrase straight into the OS
/// credential store. Every other export takes the operator's own.
///
/// Grouped with hyphens because the operator reads this back off a
/// credential manager on one machine and may have to type it on another.
/// The hyphens are part of the passphrase, not decoration around it.
///
/// The return is a plain `String` to match the rest of this module's
/// interface (`encrypt_bundle` takes `&str`). It is secret material the
/// moment it exists: store it and drop it, never log it.
///
/// # Panics
///
/// Panics if the OS entropy source fails, which `getrandom` treats as
/// unrecoverable. A passphrase built from a degraded source would be worse
/// than no bundle at all.
#[must_use]
pub fn generate_passphrase() -> String {
    use rand::RngCore;
    use zeroize::Zeroize;

    let len = PASSPHRASE_GROUP * PASSPHRASE_GROUPS;
    let mut bytes = vec![0u8; len];
    // `OsRng` reads the OS source directly on every call: no seeding, no
    // userspace state to fork or snapshot wrong.
    rand::rngs::OsRng.fill_bytes(&mut bytes);

    let mut out = String::with_capacity(len + PASSPHRASE_GROUPS - 1);
    for (i, byte) in bytes.iter().enumerate() {
        if i > 0 && i % PASSPHRASE_GROUP == 0 {
            out.push('-');
        }
        // 256 is a whole multiple of 32, so the low five bits are uniform.
        out.push(char::from(PASSPHRASE_ALPHABET[usize::from(byte & 0x1f)]));
    }
    bytes.zeroize();
    out
}

/// Decrypted bundle contents: the full connection store plus every secret
/// it references, keyed by keyring reference.
///
/// This is the plaintext `age` encrypts. It holds real secret material in
/// [`BundlePayload::secrets`], so it must never be logged, serialized to
/// disk, or included in a `Debug` dump verbatim — hence the hand-written
/// [`std::fmt::Debug`] below that redacts the secret values.
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BundlePayload {
    /// Payload schema version; see [`BUNDLE_VERSION`].
    pub version: u32,
    /// The connection entries, byte-identical to what `connections.toml`
    /// would hold (still only keyring *references*, no secrets inline).
    pub connections: ConnectionFile,
    /// `keyring_ref` -> secret material. `BTreeMap` for a stable,
    /// deterministic serialization order (reproducible test fixtures and
    /// smaller diffs when a bundle is regenerated).
    pub secrets: std::collections::BTreeMap<String, String>,
}

impl BundlePayload {
    /// Build a payload at the current [`BUNDLE_VERSION`].
    #[must_use]
    pub fn new(
        connections: ConnectionFile,
        secrets: std::collections::BTreeMap<String, String>,
    ) -> Self {
        Self {
            version: BUNDLE_VERSION,
            connections,
            secrets,
        }
    }
}

// Redact secret *values* so a stray `{:?}` (a log line, a panic message)
// can never leak them. Keys (keyring references) are non-secret and kept
// visible because they aid debugging and already appear in the TOML.
impl std::fmt::Debug for BundlePayload {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BundlePayload")
            .field("version", &self.version)
            .field("connections", &self.connections)
            .field(
                "secrets",
                &format_args!("<{} redacted>", self.secrets.len()),
            )
            .finish()
    }
}

// Scrub the secret values when a payload is dropped (ADR-0038 threat
// model). The intermediate JSON buffer is already zeroized inside
// `encrypt_bundle` / `decrypt_bundle`, but the payload itself outlives
// that buffer — it is built before encryption and handed back after
// decryption — so its `String` secret values would otherwise linger in
// freed heap (recoverable via a core dump or a swap-out) after the
// operation finishes. Keys are non-secret refs; only values are scrubbed.
//
// Implementing `Drop` means a `BundlePayload` can no longer be partially
// moved out of, so consumers take its fields via `std::mem::take` instead
// of destructuring.
impl Drop for BundlePayload {
    fn drop(&mut self) {
        for value in self.secrets.values_mut() {
            value.zeroize();
        }
    }
}

/// Which of the three checks in [`decrypt_bundle`] refused a bundle.
///
/// Kept separate from the message so a caller can branch on it: a `Header`
/// failure means the file is not a dbboard bundle at all (the wrong file was
/// picked), while `Body` means the bytes were a bundle and stopped being one
/// (a truncated copy, a mangled transfer).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CorruptStage {
    /// `Decryptor::new` refused the bytes before the passphrase was
    /// consulted: not a well-formed age file.
    Header,
    /// The passphrase-derived key could not be unwrapped for a structural
    /// reason — a broken stanza, or work the decryptor declines to do
    /// (`ExcessiveWork`). A wrong passphrase is reported as
    /// [`BundleError::IncorrectPassphrase`] instead and never lands here.
    FileKey,
    /// The ciphertext body failed its AEAD check.
    Body,
}

impl std::fmt::Display for CorruptStage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Header => "header",
            Self::FileKey => "file key",
            Self::Body => "body",
        })
    }
}

/// Failure modes for bundle encryption / decryption.
#[derive(Debug, Error)]
pub enum BundleError {
    /// The passphrase supplied to [`encrypt_bundle`] is shorter than
    /// [`MIN_PASSPHRASE_LEN`] (an empty passphrase counts here too).
    #[error("passphrase must be at least {MIN_PASSPHRASE_LEN} characters")]
    WeakPassphrase,

    /// Serializing the payload to JSON before encryption failed. Only a
    /// programming error (non-serializable payload) can trigger this.
    #[error("failed to serialize bundle payload: {0}")]
    Serialize(#[source] serde_json::Error),

    /// The passphrase did not match the bundle — the overwhelmingly
    /// common decrypt failure, surfaced separately so the UI can say
    /// "wrong passphrase" instead of "corrupt file".
    #[error("incorrect passphrase")]
    IncorrectPassphrase,

    /// The bundle is malformed, truncated, or tampered with (bad MAC,
    /// unknown format, corrupted ciphertext).
    ///
    /// `stage` says which of the three checks refused it and `reason` keeps
    /// `age`'s own words. Both exist because "corrupt" is the reading that is
    /// usually wrong: a `.dbbx` is made to be carried to another machine, and
    /// the first thing a person does with a one-word failure is conclude the
    /// file is damaged.
    #[error("bundle is corrupt or was not produced by dbboard ({stage} stage: {reason})")]
    Corrupt {
        /// Which check refused the bundle.
        stage: CorruptStage,
        /// What `age` said, verbatim. Structural only — no key material
        /// passes through `age`'s error text.
        reason: String,
    },

    /// The decrypted payload declares a schema version this build does
    /// not understand.
    #[error("unsupported bundle version: {0}")]
    UnsupportedVersion(u32),

    /// The decrypted bytes were not the JSON payload we expect — e.g. a
    /// valid age file that isn't a dbboard bundle.
    #[error("bundle contents are not a valid dbboard payload: {0}")]
    Parse(#[source] serde_json::Error),

    /// An I/O error crossing the age reader/writer boundary. In-memory
    /// buffers make this practically unreachable, but the age API is
    /// `io::Result`-shaped, so it is surfaced rather than swallowed.
    #[error("bundle I/O error: {0}")]
    Io(#[source] std::io::Error),
}

/// Encrypt `payload` under `passphrase`, returning the `age` binary blob
/// ready to write to a `.dbbx` file.
///
/// # Errors
///
/// - [`BundleError::WeakPassphrase`] if `passphrase` is shorter than
///   [`MIN_PASSPHRASE_LEN`].
/// - [`BundleError::Serialize`] if the payload cannot be JSON-encoded.
/// - [`BundleError::Io`] if the age writer fails (in practice unreachable
///   for the in-memory buffer used here).
pub fn encrypt_bundle(payload: &BundlePayload, passphrase: &str) -> Result<Vec<u8>, BundleError> {
    if passphrase.len() < MIN_PASSPHRASE_LEN {
        return Err(BundleError::WeakPassphrase);
    }

    let _kdf = kdf_guard();

    let mut plaintext = serde_json::to_vec(payload).map_err(BundleError::Serialize)?;

    let encryptor = age::Encryptor::with_user_passphrase(SecretString::from(passphrase.to_owned()));
    let mut encrypted = Vec::new();
    let result = (|| {
        let mut writer = encryptor.wrap_output(&mut encrypted)?;
        writer.write_all(&plaintext)?;
        writer.finish()?;
        Ok(())
    })()
    .map_err(BundleError::Io);

    // Scrub the plaintext regardless of success — it briefly held every
    // secret in the store.
    plaintext.zeroize();
    result?;

    Ok(encrypted)
}

/// Decrypt an `age` blob produced by [`encrypt_bundle`] under
/// `passphrase` and parse the [`BundlePayload`].
///
/// # Errors
///
/// - [`BundleError::IncorrectPassphrase`] if the passphrase does not open
///   the bundle.
/// - [`BundleError::Corrupt`] if the blob is malformed, truncated, or
///   tampered with.
/// - [`BundleError::Parse`] if the decrypted bytes are not a dbboard
///   payload.
/// - [`BundleError::UnsupportedVersion`] if the payload's schema version
///   is newer than this build understands.
/// - [`BundleError::Io`] for a reader failure at the age boundary.
pub fn decrypt_bundle(blob: &[u8], passphrase: &str) -> Result<BundlePayload, BundleError> {
    let _kdf = kdf_guard();

    // Header stage: a failure here is a malformed/non-age file — the
    // passphrase has not been consulted yet.
    let decryptor = age::Decryptor::new(blob).map_err(|e| map_header_err(&e))?;

    // File-key unwrap stage: the passphrase is used here. age cannot tell
    // a wrong passphrase from a corrupted key stanza (both fail the same
    // AEAD check), so `DecryptionFailed` / `NoMatchingKeys` are reported
    // as a wrong passphrase — the actionable common case.
    let identity = age::scrypt::Identity::new(SecretString::from(passphrase.to_owned()));
    let mut reader = decryptor
        .decrypt(std::iter::once(&identity as &dyn age::Identity))
        .map_err(map_unwrap_err)?;

    // Payload stage: the passphrase already unwrapped the file key, so a
    // read failure here is a tampered body (the STREAM AEAD tag), not a
    // wrong passphrase and not a real I/O fault on an in-memory buffer.
    let mut plaintext = Vec::new();
    let outcome = match reader.read_to_end(&mut plaintext) {
        Ok(_) => parse_payload(&plaintext),
        Err(err) => Err(BundleError::Corrupt {
            stage: CorruptStage::Body,
            reason: err.to_string(),
        }),
    };
    // Scrub the decrypted JSON — it held the secrets in the clear even
    // after we parsed them into the returned payload.
    plaintext.zeroize();
    outcome
}

/// Validate a passphrase against the export policy without encrypting
/// anything — lets the UI reject a weak passphrase before doing work.
///
/// # Errors
///
/// [`BundleError::WeakPassphrase`] if shorter than [`MIN_PASSPHRASE_LEN`].
pub fn validate_passphrase(passphrase: &str) -> Result<(), BundleError> {
    if passphrase.len() < MIN_PASSPHRASE_LEN {
        return Err(BundleError::WeakPassphrase);
    }
    Ok(())
}

/// Serialise the scrypt key derivation — **in tests only**.
///
/// age derives the file key with scrypt at `log_n` = 18, r = 8, so one encrypt
/// or decrypt asks the allocator for a single contiguous 256 MiB buffer. The
/// test harness runs one thread per core, and this crate has roughly twenty
/// tests that go through the bundle, so a parallel run can demand several
/// gigabytes at the same instant. When the machine cannot supply it, Rust's
/// allocation-failure handler aborts the whole test binary and Windows
/// reports the abort as `STATUS_STACK_BUFFER_OVERRUN` (0xc0000409) — which
/// reads like a memory-safety bug and is not one. Whether it happens depends
/// on how much RAM is free at that moment, so the same commit fails a push on
/// a busy machine and passes on an idle one.
///
/// Production takes no lock. An export or an import is one deliberate,
/// user-initiated operation, never twenty at once, and slowing it down to
/// suit the test harness would be the tail wagging the dog.
#[cfg(test)]
fn kdf_guard() -> std::sync::MutexGuard<'static, ()> {
    static KDF: std::sync::Mutex<()> = std::sync::Mutex::new(());
    // A panic in one test must not poison the lock for every later one.
    KDF.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Outside tests the guard exists only so the call sites read the same in both
/// builds. It is a zero-sized value rather than `()` so the binding is not a
/// unit-typed local.
#[cfg(not(test))]
struct KdfGuard;

#[cfg(not(test))]
fn kdf_guard() -> KdfGuard {
    KdfGuard
}

fn parse_payload(plaintext: &[u8]) -> Result<BundlePayload, BundleError> {
    let payload: BundlePayload = serde_json::from_slice(plaintext).map_err(BundleError::Parse)?;
    if payload.version != BUNDLE_VERSION {
        return Err(BundleError::UnsupportedVersion(payload.version));
    }
    Ok(payload)
}

// Header parse (`Decryptor::new`) failure: the passphrase has not been
// used yet, so anything but a real I/O fault means the bytes are not a
// well-formed age file.
fn map_header_err(err: &age::DecryptError) -> BundleError {
    // Every arm lands on `Corrupt`, including `Io`: the input is a `&[u8]`,
    // so there is no device that can fault. An I/O error here is `age`
    // reaching past the end of the slice — the bundle stops early — and
    // reporting that as an I/O fault sends the reader to look at their disk.
    BundleError::Corrupt {
        stage: CorruptStage::Header,
        reason: err.to_string(),
    }
}

// File-key unwrap (`Decryptor::decrypt`) failure: the passphrase-derived
// key could not open the scrypt stanza. age reports both a wrong
// passphrase and a corrupted stanza as `DecryptionFailed` /
// `NoMatchingKeys`; we call that "incorrect passphrase" because that is
// the action the user should take first. Structural failures
// (`InvalidMac`, `InvalidHeader`, `UnknownFormat`, `ExcessiveWork`, …)
// mean the file itself is broken.
fn map_unwrap_err(err: age::DecryptError) -> BundleError {
    match err {
        age::DecryptError::DecryptionFailed | age::DecryptError::NoMatchingKeys => {
            BundleError::IncorrectPassphrase
        }
        // As in `map_header_err`: no device, so no I/O fault.
        other => BundleError::Corrupt {
            stage: CorruptStage::FileKey,
            reason: other.to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{ConnectionEntry, ConnectionKind};

    const GOOD_PASS: &str = "correct horse battery";

    fn sample_payload() -> BundlePayload {
        let connections = ConnectionFile {
            mcp_export: None,
            version: crate::store::CONFIG_VERSION,
            connections: vec![
                ConnectionEntry {
                    mcp_alias: None,
                    color: None,
                    tag: None,
                    mcp_write: false,
                    ssh: None,
                    id: "store-a".to_string(),
                    name: "store-a".to_string(),
                    kind: ConnectionKind::D1 {
                        account_id: "acct".to_string(),
                        database_id: "db".to_string(),
                        base_url: None,
                        keyring_token_ref: "dbboard.store-a.token".to_string(),
                    },
                },
                ConnectionEntry {
                    mcp_alias: None,
                    color: None,
                    tag: None,
                    mcp_write: false,
                    ssh: None,
                    id: "store-c".to_string(),
                    name: "store-c".to_string(),
                    kind: ConnectionKind::Supabase {
                        keyring_url_ref: "dbboard.store-c.url".to_string(),
                    },
                },
            ],
        };
        let mut secrets = std::collections::BTreeMap::new();
        secrets.insert(
            "dbboard.store-a.token".to_string(),
            "cf-token-123".to_string(),
        );
        secrets.insert(
            "dbboard.store-c.url".to_string(),
            "postgres://user:pw@host/db".to_string(),
        );
        BundlePayload::new(connections, secrets)
    }

    #[test]
    fn round_trips_a_populated_payload() {
        let payload = sample_payload();
        let blob = encrypt_bundle(&payload, GOOD_PASS).expect("encrypt");
        let recovered = decrypt_bundle(&blob, GOOD_PASS).expect("decrypt");
        assert_eq!(recovered, payload);
    }

    #[test]
    fn round_trips_an_empty_store() {
        let payload =
            BundlePayload::new(ConnectionFile::empty(), std::collections::BTreeMap::new());
        let blob = encrypt_bundle(&payload, GOOD_PASS).expect("encrypt");
        let recovered = decrypt_bundle(&blob, GOOD_PASS).expect("decrypt");
        assert_eq!(recovered, payload);
        assert!(recovered.secrets.is_empty());
    }

    #[test]
    fn ciphertext_does_not_contain_the_plaintext_secrets() {
        // The whole point: the on-disk blob must not leak secret bytes.
        let payload = sample_payload();
        let blob = encrypt_bundle(&payload, GOOD_PASS).expect("encrypt");
        let haystack = String::from_utf8_lossy(&blob);
        assert!(!haystack.contains("cf-token-123"));
        assert!(!haystack.contains("postgres://user:pw@host/db"));
        assert!(!haystack.contains("dbboard.store-a.token"));
    }

    #[test]
    fn wrong_passphrase_is_reported_distinctly() {
        let blob = encrypt_bundle(&sample_payload(), GOOD_PASS).expect("encrypt");
        let err = decrypt_bundle(&blob, "the wrong passphrase").expect_err("must fail");
        assert!(
            matches!(err, BundleError::IncorrectPassphrase),
            "got {err:?}"
        );
    }

    #[test]
    fn tampered_ciphertext_is_rejected() {
        let mut blob = encrypt_bundle(&sample_payload(), GOOD_PASS).expect("encrypt");
        // Flip a byte deep in the payload body (past the age header) so
        // the AEAD tag, not the passphrase check, is what fails.
        let last = blob.len() - 1;
        blob[last] ^= 0xff;
        let err = decrypt_bundle(&blob, GOOD_PASS).expect_err("must fail");
        assert!(
            matches!(
                err,
                BundleError::Corrupt { .. } | BundleError::IncorrectPassphrase
            ),
            "tampered blob must not decrypt cleanly, got {err:?}"
        );
    }

    #[test]
    fn garbage_names_the_header_as_the_stage_that_refused_it() {
        let err = decrypt_bundle(b"this is not an age file at all", GOOD_PASS)
            .expect_err("garbage must fail");
        let BundleError::Corrupt { stage, .. } = &err else {
            panic!("expected Corrupt, got {err:?}");
        };
        assert_eq!(*stage, CorruptStage::Header);
    }

    #[test]
    fn a_flipped_body_byte_names_the_body_not_the_header() {
        let mut blob = encrypt_bundle(&sample_payload(), GOOD_PASS).expect("encrypt");
        let last = blob.len() - 1;
        blob[last] ^= 0xff;
        let err = decrypt_bundle(&blob, GOOD_PASS).expect_err("must fail");
        let BundleError::Corrupt { stage, .. } = &err else {
            panic!("expected Corrupt, got {err:?}");
        };
        assert_eq!(*stage, CorruptStage::Body);
    }

    /// The whole point of splitting the stage out: the message a person sees
    /// when a bundle will not open on the machine they moved it to has to say
    /// more than "corrupt", because "corrupt" is the one reading that is
    /// usually wrong.
    #[test]
    fn the_message_carries_both_the_stage_and_what_age_said() {
        let err =
            decrypt_bundle(b"this is not an age file at all", GOOD_PASS).expect_err("must fail");
        let text = err.to_string();
        assert!(text.contains("header"), "no stage in {text:?}");
        let BundleError::Corrupt { reason, .. } = &err else {
            panic!("expected Corrupt, got {err:?}");
        };
        assert!(!reason.is_empty(), "age's own words were dropped");
    }

    /// A bundle that stops early is the failure this format is most likely
    /// to meet in the wild — it exists to be copied to another machine, and a
    /// copy is what gets interrupted. `age` reports the short read as an I/O
    /// error, but `decrypt_bundle` is handed a `&[u8]`: there is no device to
    /// fault, so "I/O error" would be a diagnosis of something that cannot
    /// have happened.
    #[test]
    fn a_bundle_cut_short_is_corrupt_rather_than_an_io_fault() {
        let blob = encrypt_bundle(&sample_payload(), GOOD_PASS).expect("encrypt");

        let err = decrypt_bundle(&blob[..20], GOOD_PASS).expect_err("must fail");
        let BundleError::Corrupt { stage, .. } = &err else {
            panic!("a truncated header must not be an I/O fault, got {err:?}");
        };
        assert_eq!(*stage, CorruptStage::Header);

        let half = blob.len() / 2;
        let err = decrypt_bundle(&blob[..half], GOOD_PASS).expect_err("must fail");
        assert!(
            matches!(err, BundleError::Corrupt { .. }),
            "a body cut in half must not be an I/O fault, got {err:?}"
        );
    }

    #[test]
    fn non_age_bytes_are_corrupt_not_a_panic() {
        let err = decrypt_bundle(b"this is not an age file at all", GOOD_PASS)
            .expect_err("garbage must fail");
        assert!(matches!(err, BundleError::Corrupt { .. }), "got {err:?}");
    }

    #[test]
    fn short_passphrase_is_refused_before_encrypting() {
        let err = encrypt_bundle(&sample_payload(), "short").expect_err("must refuse");
        assert!(matches!(err, BundleError::WeakPassphrase), "got {err:?}");
    }

    #[test]
    fn validate_passphrase_matches_the_encrypt_policy() {
        assert!(validate_passphrase("1234567").is_err());
        assert!(validate_passphrase("12345678").is_ok());
    }

    #[test]
    fn a_future_version_payload_is_rejected() {
        // Encrypt a hand-built payload whose version is one ahead.
        let mut payload = sample_payload();
        payload.version = BUNDLE_VERSION + 1;
        let blob = encrypt_bundle(&payload, GOOD_PASS).expect("encrypt");
        let err = decrypt_bundle(&blob, GOOD_PASS).expect_err("must reject");
        match err {
            BundleError::UnsupportedVersion(v) => assert_eq!(v, BUNDLE_VERSION + 1),
            other => panic!("expected UnsupportedVersion, got {other:?}"),
        }
    }

    #[test]
    fn debug_redacts_secret_values() {
        let payload = sample_payload();
        let rendered = format!("{payload:?}");
        assert!(
            !rendered.contains("cf-token-123"),
            "secret leaked: {rendered}"
        );
        assert!(rendered.contains("redacted"), "expected redaction marker");
    }

    // --- generated passphrases (ADR-0140) ----------------------------

    #[test]
    fn a_generated_passphrase_is_grouped_and_unambiguous() {
        let pass = generate_passphrase();
        let groups: Vec<&str> = pass.split('-').collect();
        assert_eq!(groups.len(), PASSPHRASE_GROUPS, "grouping: {pass}");
        for group in &groups {
            assert_eq!(group.len(), PASSPHRASE_GROUP, "group length: {pass}");
        }
        for ch in pass.chars().filter(|c| *c != '-') {
            assert!(
                PASSPHRASE_ALPHABET.contains(&u8::try_from(ch).expect("ascii")),
                "character {ch:?} is outside the alphabet: {pass}"
            );
        }
        // The floor `encrypt_bundle` enforces; a generated passphrase that
        // its own encrypt path would reject is a bug, not a short one.
        assert!(pass.len() >= MIN_PASSPHRASE_LEN);
    }

    #[test]
    fn generated_passphrases_differ() {
        // Not a randomness test — 150 bits cannot repeat by accident, so a
        // repeat here means the source is not being read at all.
        let first = generate_passphrase();
        let second = generate_passphrase();
        assert_ne!(first, second);
    }

    #[test]
    fn a_generated_passphrase_opens_the_bundle_it_sealed() {
        let pass = generate_passphrase();
        let payload = sample_payload();
        let blob = encrypt_bundle(&payload, &pass).expect("encrypt");
        let back = decrypt_bundle(&blob, &pass).expect("decrypt");
        assert_eq!(back, payload);
    }
}
