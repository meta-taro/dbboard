//! Query history (ADR-0014 Stage 1 + ADR-0017 Stage 2).
//!
//! [`HistoryStore`] is the bounded, newest-first ring buffer owned by
//! the egui app. [`PersistentHistoryStore`] wraps it with an append-only
//! JSON Lines writer so a session's history survives across runs and is
//! directly inspectable by `jq` / `tail -F` / `grep` (the ADR-0017 UX
//! differentiator).
//!
//! The on-disk record schema is the single source of truth shared with
//! the `dbboard-web` sibling — see ADR-0017 for the cross-repo policy.
//! Forward-compatibility is built in: lines whose `v` is not understood
//! by the current reader are skipped and counted in
//! [`PersistentHistoryStore::skipped_on_load`] so the binary can log a
//! single startup warning.

use std::collections::VecDeque;
use std::fs;
use std::io::{self, BufRead, BufReader, ErrorKind, Write};
use std::path::{Path, PathBuf};

use dbboard_config::secure_fs;
use serde::{Deserialize, Serialize};

/// Default cap used by [`HistoryStore::default`]. Chosen for UI
/// ergonomics, not correctness — 100 short SQL strings is plenty for a
/// session and stays well below any meaningful memory budget.
pub const DEFAULT_CAPACITY: usize = 100;

/// Rotation threshold (bytes). When the active file exceeds this at
/// **startup**, it is renamed to `history.jsonl.1` (overwriting any
/// existing `.1`) and a fresh `history.jsonl` is created. Rotation is
/// not triggered mid-session; a long-running session can grow the file
/// past the cap, and the cap only fires the next time the app starts.
pub const ROTATION_BYTES: u64 = 50 * 1024 * 1024;

/// Rotation threshold (lines). Same startup-only trigger as
/// [`ROTATION_BYTES`]. Either threshold rotates the file independently
/// so a stream of pathologically small records still gets cycled.
pub const ROTATION_LINES: usize = 100_000;

/// Schema version emitted by the writer (ADR-0017). A future on-disk
/// breaking change bumps this constant *and* lands a new ADR; readers
/// stay forward-compatible for purely additive fields without a bump.
pub const CURRENT_VERSION: u32 = 1;

/// Outcome category for one history record (`"ok"` | `"error"` on the
/// wire). Future additions (e.g. `"cancelled"`, `"timeout"`) would be
/// additive — readers default to skipping unknown values rather than
/// hard-failing the whole file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryStatus {
    Ok,
    Error,
}

impl HistoryStatus {
    fn as_wire(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Error => "error",
        }
    }

    fn from_wire(s: &str) -> Option<Self> {
        match s {
            "ok" => Some(Self::Ok),
            "error" => Some(Self::Error),
            _ => None,
        }
    }
}

/// Error payload attached to a history entry when `status="error"`.
/// Category matches the 5 `DbError` categories from ADR-0009 / ADR-0012.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryError {
    pub category: String,
    pub message: String,
}

/// One history record. Stage 1 callers populate only `sql` via
/// [`HistoryEntry::from_sql`]; persistence-aware callers populate every
/// field so the on-disk record carries full metadata (ADR-0017).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryEntry {
    pub sql: String,
    /// RFC 3339 UTC timestamp with millisecond precision (e.g.
    /// `"2026-06-04T14:22:01.123Z"`). Empty string for in-memory-only
    /// (Stage 1) entries; the persistence path is expected to fill it.
    pub ts: String,
    /// Connection id (TOML `[[connections]]` primary key, or a
    /// synthetic `"env:<kind>"` label when the env-only resolution path
    /// applies). Empty when the Stage 1 in-memory-only path is used.
    pub conn: String,
    pub status: HistoryStatus,
    pub duration_ms: u64,
    /// Row count for row-returning results; `None` for DML/DDL.
    pub rows: Option<u64>,
    /// Affected count for DML; `None` for row-returning results.
    pub rows_affected: Option<u64>,
    pub error: Option<HistoryError>,
}

impl HistoryEntry {
    /// Bare in-memory entry that carries only the SQL text. Equivalent
    /// to what Stage 1 produced before ADR-0017; persistence-aware
    /// callers should construct an entry with `ts` / `conn` / `status`
    /// / `duration_ms` / etc. populated from the query reply.
    #[must_use]
    pub fn from_sql(sql: impl Into<String>) -> Self {
        Self {
            sql: sql.into(),
            ts: String::new(),
            conn: String::new(),
            status: HistoryStatus::Ok,
            duration_ms: 0,
            rows: None,
            rows_affected: None,
            error: None,
        }
    }
}

/// Bounded, newest-first ring buffer of [`HistoryEntry`].
#[derive(Debug)]
pub struct HistoryStore {
    entries: VecDeque<HistoryEntry>,
    capacity: usize,
}

impl HistoryStore {
    /// Build a store with the given cap. A `capacity` of 0 is clamped to
    /// 1 — a zero-capacity history is a footgun, not a feature.
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        let capacity = capacity.max(1);
        Self {
            entries: VecDeque::with_capacity(capacity),
            capacity,
        }
    }

    /// Push a bare SQL statement onto the front of the history (Stage 1
    /// path). Equivalent to
    /// `push_entry(HistoryEntry::from_sql(sql))`; preserved verbatim
    /// because the in-memory-only call sites do not have the richer
    /// metadata to hand at submission time.
    pub fn push(&mut self, sql: impl Into<String>) {
        self.push_entry(HistoryEntry::from_sql(sql));
    }

    /// Push a fully-populated [`HistoryEntry`] onto the front of the
    /// history. Empty / whitespace SQL is dropped; an entry whose
    /// `sql` matches the most recent entry's `sql` is treated as an
    /// adjacent duplicate and collapsed (ADR-0014). Non-adjacent
    /// repeats are kept. When the buffer is full, the oldest entry is
    /// dropped.
    pub fn push_entry(&mut self, entry: HistoryEntry) {
        if entry.sql.trim().is_empty() {
            return;
        }
        if self
            .entries
            .front()
            .is_some_and(|head| head.sql == entry.sql)
        {
            return;
        }
        if self.entries.len() == self.capacity {
            self.entries.pop_back();
        }
        self.entries.push_front(entry);
    }

    /// Iterate over entries newest-first.
    pub fn iter(&self) -> impl Iterator<Item = &HistoryEntry> {
        self.entries.iter()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    #[must_use]
    pub fn capacity(&self) -> usize {
        self.capacity
    }
}

impl Default for HistoryStore {
    fn default() -> Self {
        Self::new(DEFAULT_CAPACITY)
    }
}

// --- On-disk wire shape (ADR-0017) ---------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ErrorWire {
    category: String,
    message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RecordWire {
    v: u32,
    ts: String,
    conn: String,
    // Desktop always emits null (single-user, single-process — ADR-0016
    // / ADR-0017); reserved so a future web record with a populated
    // `actor` parses cleanly here.
    #[serde(default)]
    actor: Option<String>,
    sql: String,
    status: String,
    duration_ms: u64,
    #[serde(default)]
    rows: Option<u64>,
    #[serde(default)]
    rows_affected: Option<u64>,
    #[serde(default)]
    error: Option<ErrorWire>,
}

impl RecordWire {
    fn from_entry(e: &HistoryEntry) -> Self {
        Self {
            v: CURRENT_VERSION,
            ts: e.ts.clone(),
            conn: e.conn.clone(),
            actor: None,
            sql: e.sql.clone(),
            status: e.status.as_wire().to_owned(),
            duration_ms: e.duration_ms,
            rows: e.rows,
            rows_affected: e.rows_affected,
            error: e.error.as_ref().map(|err| ErrorWire {
                category: err.category.clone(),
                message: err.message.clone(),
            }),
        }
    }

    /// Forward-compatibility gate: a `v` we do not recognise means the
    /// writer's schema is strictly newer in a breaking way; skip the
    /// record (the caller counts the skip) rather than mis-parsing it.
    /// An unknown `status` string is the same case at a finer grain.
    fn into_entry(self) -> Option<HistoryEntry> {
        if self.v != CURRENT_VERSION {
            return None;
        }
        let status = HistoryStatus::from_wire(&self.status)?;
        Some(HistoryEntry {
            sql: self.sql,
            ts: self.ts,
            conn: self.conn,
            status,
            duration_ms: self.duration_ms,
            rows: self.rows,
            rows_affected: self.rows_affected,
            error: self.error.map(|e| HistoryError {
                category: e.category,
                message: e.message,
            }),
        })
    }
}

// --- Persistent store wrapper (ADR-0017) ---------------------------------

/// [`HistoryStore`] backed by an append-only `history.jsonl` file.
///
/// On construction via [`load_tail`](Self::load_tail), the file is
/// rotated if it has outgrown either of the [`ROTATION_BYTES`] /
/// [`ROTATION_LINES`] caps; then the most recent `capacity` well-formed
/// records are hydrated into the in-memory ring so the UI sees the same
/// API surface as the Stage 1 `HistoryStore`. On every
/// [`push_entry`](Self::push_entry), the record is serialised and
/// appended (`O_APPEND` / Windows append handle — ADR-0017 §3) and the
/// in-memory ring is updated; a disk write failure does **not** block
/// the in-memory update so the user always sees their query.
pub struct PersistentHistoryStore {
    inner: HistoryStore,
    /// `None` when the OS reported no per-user config dir — the
    /// resolver picked the in-memory-only fallback path. Push still
    /// works; it simply does not touch disk.
    path: Option<PathBuf>,
    skipped_on_load: usize,
}

impl PersistentHistoryStore {
    /// Open the persistent store at `path`, rotating if needed and
    /// hydrating the in-memory ring with up to `capacity` of the most
    /// recent well-formed records.
    ///
    /// A missing file is fine: the store starts empty and the file is
    /// created lazily on the first
    /// [`push_entry`](Self::push_entry) call.
    ///
    /// Lines that fail to parse (truncated by a crash mid-write,
    /// future schema version, unknown `status`) are silently skipped
    /// during hydration and counted in
    /// [`skipped_on_load`](Self::skipped_on_load) so the caller can
    /// emit one startup-time warning.
    ///
    /// # Errors
    ///
    /// Returns the underlying [`io::Error`] only for filesystem
    /// failures that prevent reading or rotating the file.
    pub fn load_tail(path: PathBuf, capacity: usize) -> io::Result<Self> {
        let mut store = HistoryStore::new(capacity);
        let mut skipped = 0usize;
        if path.exists() {
            maybe_rotate(&path)?;
            // After rotation the original path is gone; on a no-rotate
            // pass it is still there — re-check before reading.
            if path.exists() {
                let contents = fs::read_to_string(&path)?;
                let lines: Vec<&str> = contents.lines().collect();
                let mut entries: Vec<HistoryEntry> = Vec::with_capacity(capacity);
                // Walk from the end so we keep the newest `capacity`
                // valid records when the file is larger than the cap.
                for line in lines.iter().rev() {
                    if entries.len() >= capacity {
                        break;
                    }
                    let trimmed = line.trim();
                    if trimmed.is_empty() {
                        continue;
                    }
                    match serde_json::from_str::<RecordWire>(trimmed) {
                        Ok(rec) => match rec.into_entry() {
                            Some(entry) => entries.push(entry),
                            None => skipped += 1,
                        },
                        Err(_) => skipped += 1,
                    }
                }
                // `entries` was collected newest-first; replay
                // oldest-first into the ring so the resulting front is
                // the newest entry (matching Stage 1 semantics).
                for entry in entries.into_iter().rev() {
                    store.push_entry(entry);
                }
            }
        }
        Ok(Self {
            inner: store,
            path: Some(path),
            skipped_on_load: skipped,
        })
    }

    /// Build an in-memory-only store with no file backing. Used as the
    /// graceful fallback when path resolution fails (CI, headless, or
    /// any environment where the OS reports no per-user config dir).
    #[must_use]
    pub fn in_memory_only(capacity: usize) -> Self {
        Self {
            inner: HistoryStore::new(capacity),
            path: None,
            skipped_on_load: 0,
        }
    }

    /// Number of lines that the loader could not parse. The caller
    /// surfaces this as a single startup-time warning rather than one
    /// log line per skipped line.
    #[must_use]
    pub fn skipped_on_load(&self) -> usize {
        self.skipped_on_load
    }

    /// Read-only view of the in-memory ring. The Stage 1 UI panel reads
    /// from here verbatim.
    #[must_use]
    pub fn store(&self) -> &HistoryStore {
        &self.inner
    }

    /// Append a fully-populated entry to disk and push it into the
    /// in-memory ring. Whitespace-only SQL is dropped, matching the
    /// Stage 1 behaviour of [`HistoryStore::push`].
    ///
    /// The in-memory ring is updated **before** propagating any disk
    /// error so the user always sees their query in the UI history
    /// panel, even when the disk is full or the file was removed
    /// out-from-under us.
    ///
    /// # Errors
    ///
    /// Returns the underlying [`io::Error`] when the disk append fails.
    /// Callers should log and otherwise swallow — the contract is
    /// best-effort (ADR-0017 §6).
    pub fn push_entry(&mut self, entry: HistoryEntry) -> io::Result<()> {
        if entry.sql.trim().is_empty() {
            return Ok(());
        }
        // Capture-then-propagate so the in-memory update happens
        // regardless of disk state. Cloning the entry is cheap (short
        // SQL strings).
        let disk_result = match self.path.as_ref() {
            Some(path) => append_record(path, &entry),
            None => Ok(()),
        };
        self.inner.push_entry(entry);
        disk_result
    }

    /// Submit-time path: push a bare SQL statement into the in-memory
    /// ring **only**. Disk is not touched (the completion record lands
    /// later via [`record_completion`](Self::record_completion) once the
    /// reply carries duration / rows / status).
    ///
    /// Used by `DbboardApp::run_sql` so the history panel updates as
    /// soon as the user clicks Run — matching the UX expectation of
    /// `DataGrip` / `pgAdmin` / mainstream SQL clients — without waiting on
    /// the query reply.
    pub fn record_submit(&mut self, sql: impl Into<String>) {
        self.inner.push(sql);
    }

    /// Completion-time path: append a fully-populated record to disk
    /// **only**. The in-memory ring was already updated at submit time
    /// via [`record_submit`](Self::record_submit) so this is a pure
    /// disk write.
    ///
    /// # Errors
    ///
    /// Returns the underlying [`io::Error`] when the disk append fails.
    /// Callers should log and otherwise swallow — the contract is
    /// best-effort (ADR-0017 §6). When the store is in-memory-only
    /// (no path) this is an immediate `Ok(())` with no work done.
    pub fn record_completion(&self, entry: &HistoryEntry) -> io::Result<()> {
        if entry.sql.trim().is_empty() {
            return Ok(());
        }
        match self.path.as_ref() {
            Some(path) => append_record(path, entry),
            None => Ok(()),
        }
    }
}

fn maybe_rotate(path: &Path) -> io::Result<()> {
    let metadata = fs::metadata(path)?;
    if metadata.len() > ROTATION_BYTES {
        return rotate(path);
    }
    // Size is under the byte cap — count lines as a second gate so a
    // stream of tiny records still rotates. Stop scanning past the cap
    // (no need to count every line of an under-rotation file).
    let file = fs::File::open(path)?;
    let reader = BufReader::new(file);
    let mut count = 0usize;
    for line in reader.lines() {
        line?;
        count += 1;
        if count > ROTATION_LINES {
            return rotate(path);
        }
    }
    Ok(())
}

fn rotate(path: &Path) -> io::Result<()> {
    let backup = path.with_extension("jsonl.1");
    match fs::rename(path, &backup) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

fn append_record(path: &Path, entry: &HistoryEntry) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    // ADR-0024: open with user-only permissions on first creation, and
    // defensively tighten any pre-existing legacy file on Unix.
    let mut file = secure_fs::open_append_user_only(path)?;
    let rec = RecordWire::from_entry(entry);
    let mut bytes =
        serde_json::to_vec(&rec).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    bytes.push(b'\n');
    file.write_all(&bytes)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        HistoryEntry, HistoryError, HistoryStatus, HistoryStore, PersistentHistoryStore,
        RecordWire, CURRENT_VERSION, DEFAULT_CAPACITY, ROTATION_LINES,
    };
    use std::fs;
    use tempfile::TempDir;

    // ---- Stage 1: in-memory ring buffer (unchanged behaviour) ----

    #[test]
    fn empty_by_default() {
        let h = HistoryStore::default();
        assert!(h.is_empty());
        assert_eq!(h.len(), 0);
        assert_eq!(h.capacity(), DEFAULT_CAPACITY);
    }

    #[test]
    fn push_adds_one_entry() {
        let mut h = HistoryStore::new(10);
        h.push("SELECT 1");
        assert_eq!(h.len(), 1);
        assert_eq!(h.iter().next().unwrap().sql, "SELECT 1");
    }

    #[test]
    fn iter_is_newest_first() {
        let mut h = HistoryStore::new(10);
        h.push("first");
        h.push("second");
        h.push("third");

        let collected: Vec<&str> = h.iter().map(|e| e.sql.as_str()).collect();
        assert_eq!(collected, vec!["third", "second", "first"]);
    }

    #[test]
    fn adjacent_duplicate_is_collapsed() {
        let mut h = HistoryStore::new(10);
        h.push("SELECT 1");
        h.push("SELECT 1");
        h.push("SELECT 1");
        assert_eq!(h.len(), 1);
    }

    #[test]
    fn non_adjacent_duplicate_is_kept() {
        let mut h = HistoryStore::new(10);
        h.push("SELECT 1");
        h.push("SELECT 2");
        h.push("SELECT 1");
        assert_eq!(h.len(), 3);

        let collected: Vec<&str> = h.iter().map(|e| e.sql.as_str()).collect();
        assert_eq!(collected, vec!["SELECT 1", "SELECT 2", "SELECT 1"]);
    }

    #[test]
    fn capacity_drops_oldest_entry() {
        let mut h = HistoryStore::new(3);
        h.push("one");
        h.push("two");
        h.push("three");
        h.push("four");

        assert_eq!(h.len(), 3);
        let collected: Vec<&str> = h.iter().map(|e| e.sql.as_str()).collect();
        assert_eq!(collected, vec!["four", "three", "two"]);
    }

    #[test]
    fn empty_or_whitespace_input_is_ignored() {
        let mut h = HistoryStore::new(10);
        h.push("");
        h.push("   ");
        h.push("\t\n");
        assert!(h.is_empty());
    }

    #[test]
    fn zero_capacity_is_clamped_to_one() {
        let mut h = HistoryStore::new(0);
        assert_eq!(h.capacity(), 1);
        h.push("first");
        h.push("second");
        assert_eq!(h.len(), 1);
        assert_eq!(h.iter().next().unwrap().sql, "second");
    }

    // ---- Stage 2: serde and persistence (ADR-0017) ----

    fn sample_entry(sql: &str) -> HistoryEntry {
        HistoryEntry {
            sql: sql.to_string(),
            ts: "2026-06-04T14:22:01.123Z".to_string(),
            conn: "prod-pg".to_string(),
            status: HistoryStatus::Ok,
            duration_ms: 42,
            rows: Some(10),
            rows_affected: None,
            error: None,
        }
    }

    fn sample_error_entry(sql: &str) -> HistoryEntry {
        HistoryEntry {
            sql: sql.to_string(),
            ts: "2026-06-04T14:22:05.000Z".to_string(),
            conn: "prod-pg".to_string(),
            status: HistoryStatus::Error,
            duration_ms: 7,
            rows: None,
            rows_affected: None,
            error: Some(HistoryError {
                category: "query".to_string(),
                message: "syntax error at or near 'FROM'".to_string(),
            }),
        }
    }

    #[test]
    fn record_serializes_with_v_envelope_and_null_actor_on_desktop() {
        let entry = sample_entry("SELECT 1");
        let serialized = serde_json::to_string(&RecordWire::from_entry(&entry)).unwrap();
        // v=1 envelope present, actor always null on desktop.
        assert!(serialized.contains(r#""v":1"#), "serialized: {serialized}");
        assert!(
            serialized.contains(r#""actor":null"#),
            "serialized: {serialized}"
        );
        assert!(
            serialized.contains(r#""status":"ok""#),
            "serialized: {serialized}"
        );
    }

    #[test]
    fn record_round_trips_through_json() {
        let entry = sample_entry("SELECT * FROM users");
        let json = serde_json::to_string(&RecordWire::from_entry(&entry)).unwrap();
        let parsed: RecordWire = serde_json::from_str(&json).unwrap();
        let back = parsed.into_entry().expect("v=1 status=ok must parse");
        assert_eq!(back, entry);
    }

    #[test]
    fn error_record_round_trips_through_json() {
        let entry = sample_error_entry("SELCT 1");
        let json = serde_json::to_string(&RecordWire::from_entry(&entry)).unwrap();
        let parsed: RecordWire = serde_json::from_str(&json).unwrap();
        let back = parsed.into_entry().expect("error record must parse");
        assert_eq!(back, entry);
    }

    #[test]
    fn record_with_unknown_v_is_dropped_during_hydration() {
        // Hand-craft a v=2 line. A future breaking schema must NOT be
        // mis-parsed by a v=1 reader.
        let line = r#"{"v":2,"ts":"2026-06-04T14:22:01.000Z","conn":"prod-pg","actor":null,"sql":"SELECT 1","status":"ok","duration_ms":1,"rows":1,"rows_affected":null,"error":null}"#;
        let parsed: RecordWire = serde_json::from_str(line).unwrap();
        assert!(parsed.into_entry().is_none());
    }

    #[test]
    fn record_with_unknown_status_is_dropped_during_hydration() {
        let line = r#"{"v":1,"ts":"2026-06-04T14:22:01.000Z","conn":"prod-pg","actor":null,"sql":"SELECT 1","status":"cancelled","duration_ms":1,"rows":null,"rows_affected":null,"error":null}"#;
        let parsed: RecordWire = serde_json::from_str(line).unwrap();
        assert!(parsed.into_entry().is_none());
    }

    #[test]
    fn record_ignores_unknown_json_fields_for_forward_compat() {
        // A future writer may add a `user_agent` field. The current
        // reader must keep parsing the known fields rather than
        // hard-failing the whole line.
        let line = r#"{"v":1,"ts":"2026-06-04T14:22:01.000Z","conn":"prod-pg","actor":null,"sql":"SELECT 1","status":"ok","duration_ms":1,"rows":1,"rows_affected":null,"error":null,"user_agent":"dbboard/0.2"}"#;
        let parsed: RecordWire = serde_json::from_str(line).expect("unknown field must be ignored");
        let entry = parsed.into_entry().expect("known fields must round-trip");
        assert_eq!(entry.sql, "SELECT 1");
    }

    #[test]
    fn load_tail_on_missing_file_yields_empty_store_with_zero_skipped() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("history.jsonl");
        let store = PersistentHistoryStore::load_tail(path, 100).expect("load");
        assert!(store.store().is_empty());
        assert_eq!(store.skipped_on_load(), 0);
    }

    #[test]
    fn push_entry_appends_one_jsonl_line_to_disk() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("history.jsonl");
        let mut store = PersistentHistoryStore::load_tail(path.clone(), 100).expect("load");
        store.push_entry(sample_entry("SELECT 1")).expect("append");
        store.push_entry(sample_entry("SELECT 2")).expect("append");

        let contents = fs::read_to_string(&path).expect("read history");
        let lines: Vec<&str> = contents.lines().collect();
        assert_eq!(lines.len(), 2);
        for line in &lines {
            // Every line is valid JSON terminated by LF (file ends with \n).
            let parsed: serde_json::Value = serde_json::from_str(line).expect("valid json");
            assert_eq!(parsed["v"], 1);
            assert_eq!(parsed["actor"], serde_json::Value::Null);
        }
        // LF-only newlines (ADR-0017) — no CRLF anywhere.
        assert!(!contents.contains('\r'));
    }

    #[test]
    fn push_entry_with_no_path_does_not_touch_disk() {
        let mut store = PersistentHistoryStore::in_memory_only(100);
        store.push_entry(sample_entry("SELECT 1")).expect("ok");
        assert_eq!(store.store().len(), 1);
    }

    #[test]
    fn push_entry_drops_whitespace_only_sql() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("history.jsonl");
        let mut store = PersistentHistoryStore::load_tail(path.clone(), 100).expect("load");
        let mut blank = sample_entry("   \t\n");
        blank.sql = "   ".to_string();
        store.push_entry(blank).expect("ok");
        assert!(store.store().is_empty());
        // File should not have been created either.
        assert!(!path.exists() || fs::read_to_string(&path).unwrap().is_empty());
    }

    #[test]
    fn push_entry_collapses_adjacent_duplicate_in_memory_but_still_writes_to_disk() {
        // The file is a full log — every query reply gets a record
        // even when the UI dedupes the display (ADR-0017 §6).
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("history.jsonl");
        let mut store = PersistentHistoryStore::load_tail(path.clone(), 100).expect("load");
        store.push_entry(sample_entry("SELECT 1")).expect("append");
        store.push_entry(sample_entry("SELECT 1")).expect("append");

        assert_eq!(store.store().len(), 1, "in-memory dedupes");
        let contents = fs::read_to_string(&path).expect("read");
        assert_eq!(contents.lines().count(), 2, "disk keeps both");
    }

    #[test]
    fn load_tail_reads_last_capacity_lines_newest_first() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("history.jsonl");
        let mut store = PersistentHistoryStore::load_tail(path.clone(), 100).expect("load");
        for i in 0u64..5 {
            let mut e = sample_entry(&format!("SELECT {i}"));
            // Vary sql so adjacent dedup does not collapse them.
            e.duration_ms = i;
            store.push_entry(e).expect("append");
        }

        // Reopen with capacity 3 — only the newest 3 should land in memory.
        let reopened = PersistentHistoryStore::load_tail(path, 3).expect("reload");
        let sqls: Vec<&str> = reopened.store().iter().map(|e| e.sql.as_str()).collect();
        assert_eq!(sqls, vec!["SELECT 4", "SELECT 3", "SELECT 2"]);
        assert_eq!(reopened.skipped_on_load(), 0);
    }

    #[test]
    fn load_tail_skips_malformed_lines_and_counts_them() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("history.jsonl");
        let mut hand_written = String::new();
        // Two valid records, one truncated mid-write, one with v=2,
        // one totally junk. Loader keeps the two valid ones and
        // reports 3 skips.
        hand_written.push_str(
            &serde_json::to_string(&RecordWire::from_entry(&sample_entry("SELECT 1"))).unwrap(),
        );
        hand_written.push('\n');
        hand_written.push_str(r#"{"v":1,"ts":"2026-06-04T14:22:01"#); // truncated
        hand_written.push('\n');
        hand_written.push_str(r#"{"v":2,"ts":"2026-06-04T14:22:01.000Z","conn":"x","actor":null,"sql":"S","status":"ok","duration_ms":1,"rows":null,"rows_affected":null,"error":null}"#);
        hand_written.push('\n');
        hand_written.push_str("not-json-at-all");
        hand_written.push('\n');
        hand_written.push_str(
            &serde_json::to_string(&RecordWire::from_entry(&sample_entry("SELECT 2"))).unwrap(),
        );
        hand_written.push('\n');
        fs::write(&path, hand_written).expect("seed");

        let store = PersistentHistoryStore::load_tail(path, 100).expect("load");
        assert_eq!(store.store().len(), 2);
        assert_eq!(store.skipped_on_load(), 3);
        let sqls: Vec<&str> = store.store().iter().map(|e| e.sql.as_str()).collect();
        assert_eq!(sqls, vec!["SELECT 2", "SELECT 1"]);
    }

    #[test]
    fn load_tail_rotates_when_line_cap_is_exceeded() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("history.jsonl");
        // Seed ROTATION_LINES + 1 minimal valid records cheaply so the
        // line-count threshold trips without producing a 50 MiB file.
        let mut seed = String::with_capacity(ROTATION_LINES * 16);
        let one =
            serde_json::to_string(&RecordWire::from_entry(&sample_entry("SELECT 1"))).unwrap();
        for _ in 0..=ROTATION_LINES {
            seed.push_str(&one);
            seed.push('\n');
        }
        fs::write(&path, seed).expect("seed");

        let store = PersistentHistoryStore::load_tail(path.clone(), 100).expect("load");
        // Original file is gone; backup exists.
        assert!(!path.exists(), "history.jsonl must be rotated away");
        let backup = path.with_extension("jsonl.1");
        assert!(backup.exists(), "history.jsonl.1 must hold the old data");
        // Fresh ring is empty after rotation.
        assert!(store.store().is_empty());
    }

    #[test]
    fn rotation_preserves_only_one_generation() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("history.jsonl");
        let backup = path.with_extension("jsonl.1");
        // Pre-existing stale .1 — must be overwritten by rotation, not
        // promoted to .2.
        fs::write(&backup, "STALE\n").expect("seed backup");

        // Same line-cap-overflow setup as the previous test, smaller.
        let mut seed = String::new();
        let one =
            serde_json::to_string(&RecordWire::from_entry(&sample_entry("SELECT 1"))).unwrap();
        for _ in 0..=ROTATION_LINES {
            seed.push_str(&one);
            seed.push('\n');
        }
        fs::write(&path, seed).expect("seed");

        let _store = PersistentHistoryStore::load_tail(path.clone(), 100).expect("load");
        let backup_contents = fs::read_to_string(&backup).expect("read backup");
        assert!(
            !backup_contents.contains("STALE"),
            "stale backup must be overwritten"
        );
        // No .2 ever appears.
        let two = path.with_extension("jsonl.1.1");
        assert!(!two.exists());
    }

    #[test]
    fn current_version_is_one() {
        // Cross-repo schema contract guard. A future bump is a
        // deliberate breaking change — this test forces the author to
        // touch the ADR + the web sibling together.
        assert_eq!(CURRENT_VERSION, 1);
    }

    #[test]
    fn record_submit_pushes_to_memory_without_touching_disk() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("history.jsonl");
        let mut store = PersistentHistoryStore::load_tail(path.clone(), 100).expect("load");
        store.record_submit("SELECT 1");
        assert_eq!(store.store().len(), 1);
        assert_eq!(store.store().iter().next().unwrap().sql, "SELECT 1");
        assert!(
            !path.exists() || fs::read_to_string(&path).unwrap().is_empty(),
            "record_submit must not touch disk"
        );
    }

    #[test]
    fn record_completion_appends_to_disk_without_touching_memory() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("history.jsonl");
        let store = PersistentHistoryStore::load_tail(path.clone(), 100).expect("load");
        store
            .record_completion(&sample_entry("SELECT 1"))
            .expect("append");
        assert!(store.store().is_empty(), "in-memory ring must stay empty");
        let contents = fs::read_to_string(&path).expect("read");
        assert_eq!(contents.lines().count(), 1);
        let parsed: serde_json::Value =
            serde_json::from_str(contents.lines().next().unwrap()).expect("valid json");
        assert_eq!(parsed["v"], 1);
        assert_eq!(parsed["status"], "ok");
    }

    #[test]
    fn record_completion_with_no_path_is_ok() {
        let store = PersistentHistoryStore::in_memory_only(100);
        store
            .record_completion(&sample_entry("SELECT 1"))
            .expect("in-memory-only must accept the call");
    }

    #[test]
    fn record_completion_drops_whitespace_only_sql() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("history.jsonl");
        let store = PersistentHistoryStore::load_tail(path.clone(), 100).expect("load");
        let mut blank = sample_entry("   ");
        blank.sql = "   ".to_string();
        store.record_completion(&blank).expect("ok");
        assert!(!path.exists() || fs::read_to_string(&path).unwrap().is_empty());
    }
}
