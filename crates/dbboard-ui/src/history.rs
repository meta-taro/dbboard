//! Query + AI history (ADR-0014 Stage 1 + ADR-0017 Stage 2 + ADR-0027
//! Stage 2 Group C).
//!
//! [`HistoryStore`] is the bounded, newest-first ring buffer owned by
//! the egui app. [`PersistentHistoryStore`] wraps it with an append-only
//! JSON Lines writer so a session's history survives across runs and is
//! directly inspectable by `jq` / `tail -F` / `grep` (the ADR-0017 UX
//! differentiator).
//!
//! The on-disk record schema is the single source of truth shared with
//! the `dbboard-web` sibling. ADR-0017 specifies v:1 (SQL-only records);
//! ADR-0027 bumps to v:2 and adds AI-call records under a top-level
//! `"kind"` discriminator. v:2 readers accept v:1 records transparently
//! (implicit `kind="query"`); v:1 readers skip v:2 records and count
//! the skip in [`PersistentHistoryStore::skipped_on_load`].
//! Forward-compatibility for additive fields is built in — unknown
//! top-level fields are ignored; records whose `v` / `kind` / `status`
//! / `intent` are not understood are skipped (counter ticks).

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

/// Schema version emitted by the writer. ADR-0027 bumped from 1 → 2 to
/// introduce the AI-record `kind` discriminator. A future breaking
/// change bumps this constant *and* lands a new ADR; readers stay
/// forward-compatible for purely additive fields without a bump.
pub const CURRENT_VERSION: u32 = 2;

/// Persisted-text cap for AI prompts and responses (ADR-0027 §Decision
/// 10). Beyond this, the writer truncates at the last UTF-8 char
/// boundary at-or-below the cap and appends [`AI_TEXT_TRUNCATED_MARKER`]
/// to the persisted value so downstream readers see the truncation
/// rather than guessing. The cap protects the line writer from a
/// pathological 1 MiB pasted prompt blowing up the file size budget
/// the ADR-0017 rotation thresholds assume.
pub const AI_TEXT_CAP_BYTES: usize = 64 * 1024;

/// Marker appended after a truncated AI prompt or response so a reader
/// (`jq`, `grep`, or a future viewer) can distinguish "the user wrote
/// exactly this many bytes" from "the writer trimmed at the cap." The
/// trailing space + bracket-form is deliberate — it cannot appear in a
/// well-formed JSON string literal naturally and survives a single
/// `jq -r` unwrap unchanged.
pub const AI_TEXT_TRUNCATED_MARKER: &str = " [truncated at 64 KiB]";

/// Outcome category for one **query** history record (`"ok"` |
/// `"error"` on the wire). For AI records see [`AiStatus`]. Future
/// additions (e.g. `"timeout"`) would be additive — readers default to
/// skipping unknown values rather than hard-failing the whole file.
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

/// Outcome category for one **AI** history record. `Cancelled` is a
/// top-level status, not an error category — it corresponds to the
/// user pressing Stop mid-stream (ADR-0026 Decision 12 carried through
/// to persistence per ADR-0027 §Decision 5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AiStatus {
    Ok,
    Error,
    Cancelled,
}

impl AiStatus {
    fn as_wire(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Error => "error",
            Self::Cancelled => "cancelled",
        }
    }

    fn from_wire(s: &str) -> Option<Self> {
        match s {
            "ok" => Some(Self::Ok),
            "error" => Some(Self::Error),
            "cancelled" => Some(Self::Cancelled),
            _ => None,
        }
    }
}

/// What the user asked the AI to do. `Explain` = explain user-supplied
/// SQL. `SuggestSql` = generate SQL from a natural-language request.
/// A new intent is a coordinated cross-repo ADR (ADR-0027 §Decision 4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AiIntent {
    Explain,
    SuggestSql,
}

impl AiIntent {
    fn as_wire(self) -> &'static str {
        match self {
            Self::Explain => "explain",
            Self::SuggestSql => "suggest_sql",
        }
    }

    fn from_wire(s: &str) -> Option<Self> {
        match s {
            "explain" => Some(Self::Explain),
            "suggest_sql" => Some(Self::SuggestSql),
            _ => None,
        }
    }
}

/// Error payload attached to a history entry when `status="error"`.
/// Category matches the 5 `DbError` categories from ADR-0009 / ADR-0012
/// for query records, and the 3 `AiError` categories (`network` /
/// `provider` / `configuration`) from ADR-0023 §5 for AI records.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryError {
    pub category: String,
    pub message: String,
}

/// One query history record (the v:1 / v:2 `kind="query"` shape).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryEntry {
    pub sql: String,
    /// RFC 3339 UTC timestamp with millisecond precision (e.g.
    /// `"2026-06-04T14:22:01.123Z"`). Empty string for in-memory-only
    /// (Stage 1) entries; the persistence path is expected to fill it.
    pub ts: String,
    /// Connection id (TOML `[[connections]]` primary key, or a
    /// synthetic `"env:<kind>"` label when the env-only resolution
    /// path applies). Empty when the Stage 1 in-memory-only path is
    /// used.
    pub conn: String,
    pub status: HistoryStatus,
    pub duration_ms: u64,
    /// Row count for row-returning results; `None` for DML/DDL.
    pub rows: Option<u64>,
    /// Affected count for DML; `None` for row-returning results.
    pub rows_affected: Option<u64>,
    pub error: Option<HistoryError>,
}

impl QueryEntry {
    /// Bare entry that carries only the SQL text. Equivalent to what
    /// Stage 1 produced before ADR-0017; persistence-aware callers
    /// should construct an entry with `ts` / `conn` / `status` /
    /// `duration_ms` / etc. populated from the query reply.
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

/// One AI history record (the v:2 `kind="ai"` shape, ADR-0027). The
/// `prompt` and `response` carry the user-visible text verbatim — the
/// privacy stance from ADR-0017 §7 carries through unchanged; ADR-0024
/// file permissions are the at-rest threat model. Persistence applies
/// the [`AI_TEXT_CAP_BYTES`] truncation at write time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AiEntry {
    /// RFC 3339 UTC timestamp with millisecond precision.
    pub ts: String,
    /// Connection id captured at submit time, or `None` when the AI
    /// call was made without an active DB context.
    pub conn: Option<String>,
    pub intent: AiIntent,
    pub prompt: String,
    pub response: String,
    pub status: AiStatus,
    pub duration_ms: u64,
    /// Cumulative input-token count at terminal time, or `None` when
    /// the provider did not surface a usage event (atomic-default-impl
    /// path, or cancel before the first usage tick).
    pub tokens_in: Option<u64>,
    /// Cumulative output-token count at terminal time, or `None` (see
    /// `tokens_in`).
    pub tokens_out: Option<u64>,
    /// Provider id (lowercase short name) captured at submit time.
    pub provider: String,
    /// Model id as the provider reports it, captured at submit time.
    pub model: String,
    /// Free-form provider-reported stop reason, or `None` for non-
    /// terminal-stop outcomes (error / cancelled). Informational, not
    /// gated by an enum on read — see the cross-repo brief 0008.
    pub stop_reason: Option<String>,
    pub error: Option<HistoryError>,
}

/// One history record. ADR-0017 originally defined a struct shape (now
/// [`QueryEntry`]); ADR-0027 lifts it into a discriminated enum so AI
/// records share the file. Match on the variant in callers that need to
/// distinguish the two.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HistoryEntry {
    Query(QueryEntry),
    Ai(AiEntry),
}

impl HistoryEntry {
    /// Build a bare in-memory query entry from a SQL string. Returns a
    /// [`HistoryEntry::Query`] variant — AI entries always carry the
    /// richer per-record metadata and have no equivalent shortcut.
    #[must_use]
    pub fn from_sql(sql: impl Into<String>) -> Self {
        Self::Query(QueryEntry::from_sql(sql))
    }

    /// Wire-level discriminator string (`"query"` | `"ai"`). Mirrors
    /// the `kind` field on disk.
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Query(_) => "query",
            Self::Ai(_) => "ai",
        }
    }

    /// RFC 3339 UTC timestamp, regardless of variant.
    #[must_use]
    pub fn ts(&self) -> &str {
        match self {
            Self::Query(q) => &q.ts,
            Self::Ai(a) => &a.ts,
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
    /// Build a store with the given cap. A `capacity` of 0 is clamped
    /// to 1 — a zero-capacity history is a footgun, not a feature.
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
    /// history. Variant-specific rules apply:
    ///
    /// - **Query**: whitespace-only `sql` is dropped, and an entry whose
    ///   `sql` matches the most recent **query** entry's `sql` is
    ///   collapsed (adjacent dedup, ADR-0014). Non-adjacent repeats are
    ///   kept.
    /// - **AI**: every entry is appended; AI records carry no natural
    ///   dedup key (a user re-asking the same prompt is still a
    ///   separate event), and an empty prompt or response is still
    ///   meaningful (e.g. cancel-before-first-chunk).
    ///
    /// When the buffer is full, the oldest entry is dropped.
    pub fn push_entry(&mut self, entry: HistoryEntry) {
        if let HistoryEntry::Query(q) = &entry {
            if q.sql.trim().is_empty() {
                return;
            }
            if self
                .entries
                .front()
                .is_some_and(|head| matches!(head, HistoryEntry::Query(prev) if prev.sql == q.sql))
            {
                return;
            }
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

    /// Remove the entry at `index` (0 = newest, matching [`iter`](Self::iter)
    /// order) and return it, or `None` when `index` is out of range.
    ///
    /// This touches the in-memory ring only. The on-disk append-only log is
    /// deliberately left untouched (friction 2026-07-14 decision: the × in
    /// the history panel declutters the current view but preserves the audit
    /// trail), so a removed entry re-hydrates on the next launch.
    pub fn remove(&mut self, index: usize) -> Option<HistoryEntry> {
        self.entries.remove(index)
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

// --- On-disk wire shapes (ADR-0017 v:1 / ADR-0027 v:2) -------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ErrorWire {
    category: String,
    message: String,
}

/// Read shape: every field optional except the always-present envelope
/// (`v`, `ts`, `status`, `duration_ms`). Dispatch on `(v, kind)` after
/// parsing. A v:1 record has no `kind` field and is interpreted as
/// `kind="query"`; a v:2 record without a known `kind` is dropped.
///
/// Unknown top-level fields are silently ignored (serde default
/// behaviour without `deny_unknown_fields`), preserving the
/// forward-compat policy from ADR-0017 §6.
#[derive(Debug, Clone, Deserialize)]
struct ParseRecord {
    v: u32,
    #[serde(default)]
    kind: Option<String>,
    ts: String,
    #[serde(default)]
    conn: Option<String>,
    // `actor` is always `null` on desktop (single-user, single-process —
    // ADR-0016 / ADR-0017). It must still parse so the cross-repo
    // fixture's actor-override line round-trips; the runtime never
    // consults the parsed value, hence the explicit dead_code allow.
    #[serde(default)]
    #[allow(dead_code)]
    actor: Option<String>,
    // Query-only fields ----
    #[serde(default)]
    sql: Option<String>,
    status: String,
    duration_ms: u64,
    #[serde(default)]
    rows: Option<u64>,
    #[serde(default)]
    rows_affected: Option<u64>,
    #[serde(default)]
    error: Option<ErrorWire>,
    // AI-only fields ----
    #[serde(default)]
    intent: Option<String>,
    #[serde(default)]
    prompt: Option<String>,
    #[serde(default)]
    response: Option<String>,
    #[serde(default)]
    tokens_in: Option<u64>,
    #[serde(default)]
    tokens_out: Option<u64>,
    #[serde(default)]
    provider: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    stop_reason: Option<String>,
}

impl ParseRecord {
    /// Forward-compatibility gate. Unrecognised `v` / `kind` / `status`
    /// / `intent` returns `None`; the caller counts the skip rather
    /// than mis-parsing the line.
    fn into_entry(self) -> Option<HistoryEntry> {
        let kind = match self.v {
            1 => "query",
            2 => match self.kind.as_deref()? {
                k @ ("query" | "ai") => k,
                _ => return None,
            },
            _ => return None,
        };
        match kind {
            "query" => {
                let status = HistoryStatus::from_wire(&self.status)?;
                Some(HistoryEntry::Query(QueryEntry {
                    sql: self.sql.unwrap_or_default(),
                    ts: self.ts,
                    conn: self.conn.unwrap_or_default(),
                    status,
                    duration_ms: self.duration_ms,
                    rows: self.rows,
                    rows_affected: self.rows_affected,
                    error: self.error.map(|e| HistoryError {
                        category: e.category,
                        message: e.message,
                    }),
                }))
            }
            "ai" => {
                let intent = AiIntent::from_wire(self.intent.as_deref()?)?;
                let status = AiStatus::from_wire(&self.status)?;
                Some(HistoryEntry::Ai(AiEntry {
                    ts: self.ts,
                    conn: self.conn,
                    intent,
                    prompt: self.prompt.unwrap_or_default(),
                    response: self.response.unwrap_or_default(),
                    status,
                    duration_ms: self.duration_ms,
                    tokens_in: self.tokens_in,
                    tokens_out: self.tokens_out,
                    provider: self.provider.unwrap_or_default(),
                    model: self.model.unwrap_or_default(),
                    stop_reason: self.stop_reason,
                    error: self.error.map(|e| HistoryError {
                        category: e.category,
                        message: e.message,
                    }),
                }))
            }
            _ => unreachable!("kind dispatch is exhaustive"),
        }
    }
}

/// Write shape for v:2 `kind="query"` records. Field order is
/// deliberate — the web sibling's byte-equivalence test in brief 0003
/// depends on declaration order. After the v:1 → v:2 bump, `kind` slots
/// in right after `v` (per the brief 0008 example).
#[derive(Debug, Clone, Serialize)]
struct QueryRecordWire<'a> {
    v: u32,
    kind: &'static str,
    ts: &'a str,
    conn: &'a str,
    actor: Option<&'a str>,
    sql: &'a str,
    status: &'static str,
    duration_ms: u64,
    rows: Option<u64>,
    rows_affected: Option<u64>,
    error: Option<ErrorWire>,
}

impl<'a> QueryRecordWire<'a> {
    fn from_entry(e: &'a QueryEntry, actor: Option<&'a str>) -> Self {
        Self {
            v: CURRENT_VERSION,
            kind: "query",
            ts: &e.ts,
            conn: &e.conn,
            actor,
            sql: &e.sql,
            status: e.status.as_wire(),
            duration_ms: e.duration_ms,
            rows: e.rows,
            rows_affected: e.rows_affected,
            error: e.error.as_ref().map(|err| ErrorWire {
                category: err.category.clone(),
                message: err.message.clone(),
            }),
        }
    }
}

/// Write shape for v:2 `kind="ai"` records. Field order again
/// deliberate — see [`QueryRecordWire`]. `prompt` and `response` are
/// owned `String` because they are truncated at write time per
/// ADR-0027 §Decision 10.
#[derive(Debug, Clone, Serialize)]
struct AiRecordWire<'a> {
    v: u32,
    kind: &'static str,
    ts: &'a str,
    conn: Option<&'a str>,
    actor: Option<&'a str>,
    intent: &'static str,
    prompt: String,
    response: String,
    status: &'static str,
    duration_ms: u64,
    tokens_in: Option<u64>,
    tokens_out: Option<u64>,
    provider: &'a str,
    model: &'a str,
    stop_reason: Option<&'a str>,
    error: Option<ErrorWire>,
}

impl<'a> AiRecordWire<'a> {
    fn from_entry(e: &'a AiEntry, actor: Option<&'a str>) -> Self {
        Self {
            v: CURRENT_VERSION,
            kind: "ai",
            ts: &e.ts,
            conn: e.conn.as_deref(),
            actor,
            intent: e.intent.as_wire(),
            prompt: truncate_for_persistence(&e.prompt),
            response: truncate_for_persistence(&e.response),
            status: e.status.as_wire(),
            duration_ms: e.duration_ms,
            tokens_in: e.tokens_in,
            tokens_out: e.tokens_out,
            provider: &e.provider,
            model: &e.model,
            stop_reason: e.stop_reason.as_deref(),
            error: e.error.as_ref().map(|err| ErrorWire {
                category: err.category.clone(),
                message: err.message.clone(),
            }),
        }
    }
}

/// Serialise an entry to a JSON line, dispatching on the variant.
/// `actor` overrides the desktop runtime's `actor: null` for fixture
/// emission and stays `None` on the runtime path.
fn serialize_entry(entry: &HistoryEntry, actor: Option<&str>) -> String {
    match entry {
        HistoryEntry::Query(q) => serde_json::to_string(&QueryRecordWire::from_entry(q, actor))
            .expect("QueryRecordWire is always serialisable"),
        HistoryEntry::Ai(a) => serde_json::to_string(&AiRecordWire::from_entry(a, actor))
            .expect("AiRecordWire is always serialisable"),
    }
}

/// Truncate at a UTF-8 char boundary at-or-below
/// [`AI_TEXT_CAP_BYTES`], appending [`AI_TEXT_TRUNCATED_MARKER`] when
/// truncation actually fires. Short strings round-trip unchanged.
fn truncate_for_persistence(s: &str) -> String {
    if s.len() <= AI_TEXT_CAP_BYTES {
        return s.to_owned();
    }
    let mut end = AI_TEXT_CAP_BYTES;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    let mut out = String::with_capacity(end + AI_TEXT_TRUNCATED_MARKER.len());
    out.push_str(&s[..end]);
    out.push_str(AI_TEXT_TRUNCATED_MARKER);
    out
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
    /// future schema version, unknown `status` / `kind` / `intent`)
    /// are silently skipped during hydration and counted in
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
                    match serde_json::from_str::<ParseRecord>(trimmed) {
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

    /// Remove one entry from the in-memory view **only** (0 = newest),
    /// returning it. The append-only `history.jsonl` is left untouched, so
    /// the removed entry re-hydrates on the next launch — this backs the
    /// history panel's per-entry × (friction 2026-07-14): it declutters the
    /// current session without rewriting the shared audit log.
    pub fn remove_from_view(&mut self, index: usize) -> Option<HistoryEntry> {
        self.inner.remove(index)
    }

    /// Append a fully-populated entry to disk and push it into the
    /// in-memory ring. Whitespace-only-SQL query entries are dropped,
    /// matching the Stage 1 behaviour of [`HistoryStore::push`]; AI
    /// entries are always appended (an AI call with an empty prompt
    /// or response is still a meaningful event).
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
        if let HistoryEntry::Query(q) = &entry {
            if q.sql.trim().is_empty() {
                return Ok(());
            }
        }
        // Capture-then-propagate so the in-memory update happens
        // regardless of disk state. Cloning the entry is cheap (short
        // SQL strings; AI prompts/responses are capped at 64 KiB).
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
    /// `DataGrip` / `pgAdmin` / mainstream SQL clients — without
    /// waiting on the query reply.
    pub fn record_submit(&mut self, sql: impl Into<String>) {
        self.inner.push(sql);
    }

    /// Completion-time path: append a fully-populated record to disk
    /// **only**. The in-memory ring was already updated at submit time
    /// via [`record_submit`](Self::record_submit) for the query path;
    /// for the AI path see [`record_ai`](Self::record_ai).
    ///
    /// # Errors
    ///
    /// Returns the underlying [`io::Error`] when the disk append fails.
    /// Callers should log and otherwise swallow — the contract is
    /// best-effort (ADR-0017 §6). When the store is in-memory-only
    /// (no path) this is an immediate `Ok(())` with no work done.
    pub fn record_completion(&self, entry: &HistoryEntry) -> io::Result<()> {
        if let HistoryEntry::Query(q) = entry {
            if q.sql.trim().is_empty() {
                return Ok(());
            }
        }
        match self.path.as_ref() {
            Some(path) => append_record(path, entry),
            None => Ok(()),
        }
    }

    /// AI-record completion: append a `HistoryEntry::Ai` to disk and
    /// push it into the in-memory ring, symmetric to the SQL path's
    /// `record_submit` → `record_completion` split. AI calls do not
    /// have a useful submit-time view (the prompt is in the panel, the
    /// response is not yet known), so this single call covers both
    /// halves.
    ///
    /// # Errors
    ///
    /// Returns the underlying [`io::Error`] when the disk append
    /// fails; the in-memory ring is updated first.
    pub fn record_ai(&mut self, entry: AiEntry) -> io::Result<()> {
        let wrapped = HistoryEntry::Ai(entry);
        let disk_result = match self.path.as_ref() {
            Some(path) => append_record(path, &wrapped),
            None => Ok(()),
        };
        self.inner.push_entry(wrapped);
        disk_result
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
    let mut bytes = serialize_entry(entry, None).into_bytes();
    bytes.push(b'\n');
    file.write_all(&bytes)?;
    Ok(())
}

// --- Fixture-emission helpers (ADR-0017 cross-implementation round-trip) -----

/// Fixture-emission shim used by the `emit_history_fixture` example to
/// generate the cross-implementation round-trip fixture consumed by the
/// `dbboard-web` sibling. Extended for ADR-0027 to dispatch on variant.
///
/// Not part of the runtime surface — exposed only so the example can
/// drive the same wire serialisers the persistent writer uses (instead
/// of a hand-rolled stand-in that would defeat the purpose of the
/// byte-equivalence check). Hidden from rustdoc; do not call from
/// production code.
#[doc(hidden)]
pub mod fixture {
    use super::{serialize_entry, HistoryEntry};

    /// Serialise an entry exactly as the persistent writer would, with
    /// an optional `actor` override. The desktop runtime always emits
    /// `actor: null` (single-user, single-process — ADR-0016 /
    /// ADR-0017); the round-trip fixture needs at least one populated
    /// line so the web side can verify the field carries through.
    #[must_use]
    pub fn serialize(entry: &HistoryEntry, actor: Option<&str>) -> String {
        serialize_entry(entry, actor)
    }

    /// Serialise an entry with one extra string-valued top-level field
    /// appended after the standard envelope. Field order is preserved:
    /// the declared wire fields come first in declaration order, then
    /// `extra_key`. Used for the forward-compat fixture case
    /// (ADR-0017 §6 / ADR-0027) where the reader on either side must
    /// ignore unknown fields.
    ///
    /// Implemented by surgically appending `,"key":"value"` before the
    /// closing `}` of the serialised base record rather than going
    /// through a `#[serde(flatten)]` wrapper — the concatenation is
    /// transparent about field order and does not depend on serde's
    /// flatten ordering semantics. `extra_key` / `extra_value` are
    /// JSON-encoded through `serde_json::to_string` so embedded quotes
    /// and backslashes are escaped correctly.
    #[must_use]
    pub fn serialize_with_extra(
        entry: &HistoryEntry,
        extra_key: &str,
        extra_value: &str,
    ) -> String {
        let base = serialize_entry(entry, None);
        debug_assert!(base.ends_with('}'), "wire JSON must end with '}}'");
        let body = &base[..base.len() - 1];
        let k = serde_json::to_string(extra_key).expect("string is always serialisable");
        let v = serde_json::to_string(extra_value).expect("string is always serialisable");
        format!("{body},{k}:{v}}}")
    }
}

#[cfg(test)]
mod tests {
    use super::{
        fixture, serialize_entry, AiEntry, AiIntent, AiStatus, HistoryEntry, HistoryError,
        HistoryStatus, HistoryStore, ParseRecord, PersistentHistoryStore, QueryEntry,
        AI_TEXT_CAP_BYTES, AI_TEXT_TRUNCATED_MARKER, CURRENT_VERSION, DEFAULT_CAPACITY,
        ROTATION_LINES,
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
        let front = h.iter().next().unwrap();
        let HistoryEntry::Query(q) = front else {
            panic!("expected Query")
        };
        assert_eq!(q.sql, "SELECT 1");
    }

    #[test]
    fn iter_is_newest_first() {
        let mut h = HistoryStore::new(10);
        h.push("first");
        h.push("second");
        h.push("third");

        let collected: Vec<&str> = h.iter().map(query_sql).collect();
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

        let collected: Vec<&str> = h.iter().map(query_sql).collect();
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
        let collected: Vec<&str> = h.iter().map(query_sql).collect();
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
        let front = h.iter().next().unwrap();
        assert_eq!(query_sql(front), "second");
    }

    #[test]
    fn remove_deletes_the_indexed_entry_newest_first() {
        let mut h = HistoryStore::new(10);
        h.push("first");
        h.push("second");
        h.push("third");
        // iter() order is [third, second, first]; index 1 = "second".
        let removed = h.remove(1).expect("index 1 is in range");
        assert_eq!(query_sql(&removed), "second");
        let collected: Vec<&str> = h.iter().map(query_sql).collect();
        assert_eq!(collected, vec!["third", "first"]);
    }

    #[test]
    fn remove_out_of_range_is_a_no_op_returning_none() {
        let mut h = HistoryStore::new(10);
        h.push("only");
        assert!(h.remove(5).is_none());
        assert_eq!(h.len(), 1);
    }

    #[test]
    fn remove_from_view_leaves_the_disk_log_untouched() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("history.jsonl");
        let mut store = PersistentHistoryStore::load_tail(path.clone(), 100).expect("load");
        store.push_entry(sample_entry("SELECT 1")).expect("append");
        store.push_entry(sample_entry("SELECT 2")).expect("append");

        // Drop the newest in-memory entry (index 0 = "SELECT 2").
        let removed = store.remove_from_view(0).expect("in range");
        assert_eq!(query_sql(&removed), "SELECT 2");
        assert_eq!(store.store().len(), 1);

        // The append-only log still holds both records — deletion is a
        // view-only operation, so a reload re-hydrates the removed entry.
        let reloaded = PersistentHistoryStore::load_tail(path, 100).expect("reload");
        assert_eq!(reloaded.store().len(), 2);
    }

    #[test]
    fn ai_entry_pushes_without_dedup_or_empty_filter() {
        let mut h = HistoryStore::new(10);
        let ai = HistoryEntry::Ai(sample_ai_entry());
        h.push_entry(ai.clone());
        h.push_entry(ai.clone());
        assert_eq!(h.len(), 2, "AI entries never dedup");

        // An AI entry with empty prompt/response is still a meaningful event.
        let mut empty = sample_ai_entry();
        empty.prompt.clear();
        empty.response.clear();
        h.push_entry(HistoryEntry::Ai(empty));
        assert_eq!(h.len(), 3);
    }

    #[test]
    fn ai_then_query_then_same_query_still_dedups_against_query_head() {
        // Adjacent dedup compares against the *front of the ring*; an
        // AI entry between two identical query SQLs breaks the
        // adjacency so the second query is kept (matches the existing
        // non-adjacent-duplicate rule).
        let mut h = HistoryStore::new(10);
        h.push("SELECT 1");
        h.push_entry(HistoryEntry::Ai(sample_ai_entry()));
        h.push("SELECT 1");
        assert_eq!(h.len(), 3);
    }

    // ---- Stage 2: serde and persistence (ADR-0017 + ADR-0027) ----

    fn sample_entry(sql: &str) -> HistoryEntry {
        HistoryEntry::Query(sample_query_entry(sql))
    }

    fn sample_query_entry(sql: &str) -> QueryEntry {
        QueryEntry {
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
        HistoryEntry::Query(QueryEntry {
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
        })
    }

    fn sample_ai_entry() -> AiEntry {
        AiEntry {
            ts: "2026-06-30T05:12:01.456Z".to_string(),
            conn: Some("prod-pg".to_string()),
            intent: AiIntent::Explain,
            prompt: "SELECT * FROM users LIMIT 10".to_string(),
            response: "This query reads the first 10 users.".to_string(),
            status: AiStatus::Ok,
            duration_ms: 4231,
            tokens_in: Some(412),
            tokens_out: Some(218),
            provider: "anthropic".to_string(),
            model: "claude-sonnet-4-6".to_string(),
            stop_reason: Some("end_turn".to_string()),
            error: None,
        }
    }

    /// Pull SQL out of an entry the tests know is a Query variant.
    fn query_sql(entry: &HistoryEntry) -> &str {
        match entry {
            HistoryEntry::Query(q) => q.sql.as_str(),
            HistoryEntry::Ai(_) => panic!("expected Query"),
        }
    }

    #[test]
    fn current_version_is_two() {
        // Cross-repo schema contract guard. A future bump is a
        // deliberate breaking change — this test forces the author to
        // touch the ADR + the web sibling together. v:2 was introduced
        // by ADR-0027.
        assert_eq!(CURRENT_VERSION, 2);
    }

    #[test]
    fn query_record_serializes_with_kind_and_null_actor_on_desktop() {
        let entry = sample_entry("SELECT 1");
        let serialized = serialize_entry(&entry, None);
        assert!(serialized.contains(r#""v":2"#), "{serialized}");
        assert!(serialized.contains(r#""kind":"query""#), "{serialized}");
        assert!(serialized.contains(r#""actor":null"#), "{serialized}");
        assert!(serialized.contains(r#""status":"ok""#), "{serialized}");
    }

    #[test]
    fn ai_record_serializes_with_kind_and_full_field_set() {
        let entry = HistoryEntry::Ai(sample_ai_entry());
        let serialized = serialize_entry(&entry, None);
        assert!(serialized.contains(r#""v":2"#), "{serialized}");
        assert!(serialized.contains(r#""kind":"ai""#), "{serialized}");
        assert!(serialized.contains(r#""intent":"explain""#), "{serialized}");
        assert!(serialized.contains(r#""status":"ok""#), "{serialized}");
        assert!(
            serialized.contains(r#""provider":"anthropic""#),
            "{serialized}"
        );
        assert!(
            serialized.contains(r#""model":"claude-sonnet-4-6""#),
            "{serialized}"
        );
        assert!(serialized.contains(r#""tokens_in":412"#), "{serialized}");
        assert!(serialized.contains(r#""tokens_out":218"#), "{serialized}");
        assert!(
            serialized.contains(r#""stop_reason":"end_turn""#),
            "{serialized}"
        );
        assert!(serialized.contains(r#""error":null"#), "{serialized}");
    }

    #[test]
    fn ai_cancelled_record_carries_null_error() {
        // ADR-0027 §Decision 5: cancelled is a status, never an error
        // category. The error envelope stays null on the wire.
        let mut entry = sample_ai_entry();
        entry.status = AiStatus::Cancelled;
        entry.tokens_out = None;
        entry.stop_reason = None;
        let serialized = serialize_entry(&HistoryEntry::Ai(entry), None);
        assert!(
            serialized.contains(r#""status":"cancelled""#),
            "{serialized}"
        );
        assert!(serialized.contains(r#""error":null"#), "{serialized}");
        assert!(serialized.contains(r#""stop_reason":null"#), "{serialized}");
    }

    #[test]
    fn query_record_round_trips_through_json() {
        let entry = sample_entry("SELECT * FROM users");
        let json = serialize_entry(&entry, None);
        let parsed: ParseRecord = serde_json::from_str(&json).unwrap();
        let back = parsed.into_entry().expect("v=2 status=ok must parse");
        assert_eq!(back, entry);
    }

    #[test]
    fn error_record_round_trips_through_json() {
        let entry = sample_error_entry("SELCT 1");
        let json = serialize_entry(&entry, None);
        let parsed: ParseRecord = serde_json::from_str(&json).unwrap();
        let back = parsed.into_entry().expect("error record must parse");
        assert_eq!(back, entry);
    }

    #[test]
    fn ai_record_round_trips_through_json() {
        let entry = HistoryEntry::Ai(sample_ai_entry());
        let json = serialize_entry(&entry, None);
        let parsed: ParseRecord = serde_json::from_str(&json).unwrap();
        let back = parsed.into_entry().expect("v=2 kind=ai must parse");
        assert_eq!(back, entry);
    }

    #[test]
    fn v1_record_reads_transparently_as_query_kind() {
        // A v:1 fixture (no `kind` field) must still load — it is the
        // back-compat path for files written before ADR-0027.
        let line = r#"{"v":1,"ts":"2026-06-04T14:22:01.000Z","conn":"prod-pg","actor":null,"sql":"SELECT 1","status":"ok","duration_ms":1,"rows":1,"rows_affected":null,"error":null}"#;
        let parsed: ParseRecord = serde_json::from_str(line).unwrap();
        let entry = parsed.into_entry().expect("v=1 must load");
        let HistoryEntry::Query(q) = entry else {
            panic!("v=1 must dispatch to Query");
        };
        assert_eq!(q.sql, "SELECT 1");
        assert_eq!(q.status, HistoryStatus::Ok);
    }

    #[test]
    fn record_with_unknown_v_is_dropped_during_hydration() {
        // A future v=3 line. A v=2 reader must NOT mis-parse it.
        let line = r#"{"v":3,"kind":"query","ts":"2026-06-04T14:22:01.000Z","conn":"prod-pg","actor":null,"sql":"SELECT 1","status":"ok","duration_ms":1,"rows":1,"rows_affected":null,"error":null}"#;
        let parsed: ParseRecord = serde_json::from_str(line).unwrap();
        assert!(parsed.into_entry().is_none());
    }

    #[test]
    fn v2_record_without_kind_is_dropped() {
        // v:2 explicitly requires a `kind`. Missing it is a malformed
        // record (the v:1 back-compat path only kicks in at v == 1).
        let line = r#"{"v":2,"ts":"2026-06-04T14:22:01.000Z","conn":"prod-pg","actor":null,"sql":"SELECT 1","status":"ok","duration_ms":1,"rows":null,"rows_affected":null,"error":null}"#;
        let parsed: ParseRecord = serde_json::from_str(line).unwrap();
        assert!(parsed.into_entry().is_none());
    }

    #[test]
    fn v2_record_with_unknown_kind_is_dropped() {
        let line = r#"{"v":2,"kind":"telemetry","ts":"2026-06-04T14:22:01.000Z","conn":"prod-pg","actor":null,"status":"ok","duration_ms":1}"#;
        let parsed: ParseRecord = serde_json::from_str(line).unwrap();
        assert!(parsed.into_entry().is_none());
    }

    #[test]
    fn query_record_with_unknown_status_is_dropped_during_hydration() {
        // `cancelled` is a valid AI status but invalid for a query.
        let line = r#"{"v":2,"kind":"query","ts":"2026-06-04T14:22:01.000Z","conn":"prod-pg","actor":null,"sql":"SELECT 1","status":"cancelled","duration_ms":1,"rows":null,"rows_affected":null,"error":null}"#;
        let parsed: ParseRecord = serde_json::from_str(line).unwrap();
        assert!(parsed.into_entry().is_none());
    }

    #[test]
    fn ai_record_with_unknown_intent_is_dropped_during_hydration() {
        let line = r#"{"v":2,"kind":"ai","ts":"2026-06-30T05:12:01.456Z","conn":null,"actor":null,"intent":"summarize","prompt":"x","response":"y","status":"ok","duration_ms":1,"tokens_in":null,"tokens_out":null,"provider":"anthropic","model":"claude-sonnet-4-6","stop_reason":null,"error":null}"#;
        let parsed: ParseRecord = serde_json::from_str(line).unwrap();
        assert!(parsed.into_entry().is_none());
    }

    #[test]
    fn ai_record_with_unknown_status_is_dropped_during_hydration() {
        let line = r#"{"v":2,"kind":"ai","ts":"2026-06-30T05:12:01.456Z","conn":null,"actor":null,"intent":"explain","prompt":"x","response":"y","status":"timeout","duration_ms":1,"tokens_in":null,"tokens_out":null,"provider":"anthropic","model":"claude-sonnet-4-6","stop_reason":null,"error":null}"#;
        let parsed: ParseRecord = serde_json::from_str(line).unwrap();
        assert!(parsed.into_entry().is_none());
    }

    #[test]
    fn record_ignores_unknown_json_fields_for_forward_compat() {
        // A future writer may add a `user_agent` field. The current
        // reader must keep parsing the known fields rather than
        // hard-failing the whole line.
        let line = r#"{"v":2,"kind":"query","ts":"2026-06-04T14:22:01.000Z","conn":"prod-pg","actor":null,"sql":"SELECT 1","status":"ok","duration_ms":1,"rows":1,"rows_affected":null,"error":null,"user_agent":"dbboard/0.2"}"#;
        let parsed: ParseRecord =
            serde_json::from_str(line).expect("unknown field must be ignored");
        let entry = parsed.into_entry().expect("known fields must round-trip");
        let HistoryEntry::Query(q) = entry else {
            panic!("expected Query");
        };
        assert_eq!(q.sql, "SELECT 1");
    }

    #[test]
    fn ai_prompt_truncation_at_64_kib_appends_marker() {
        let mut entry = sample_ai_entry();
        // Exceed the cap by a hair so the truncation actually fires.
        entry.prompt = "x".repeat(AI_TEXT_CAP_BYTES + 100);
        let serialized = serialize_entry(&HistoryEntry::Ai(entry), None);
        let parsed: serde_json::Value =
            serde_json::from_str(&serialized).expect("serializer output must be valid JSON");
        let prompt = parsed["prompt"].as_str().expect("prompt is a string");
        assert!(prompt.ends_with(AI_TEXT_TRUNCATED_MARKER));
        assert_eq!(
            prompt.len(),
            AI_TEXT_CAP_BYTES + AI_TEXT_TRUNCATED_MARKER.len()
        );
    }

    #[test]
    fn ai_response_truncation_at_64_kib_appends_marker() {
        let mut entry = sample_ai_entry();
        entry.response = "y".repeat(AI_TEXT_CAP_BYTES + 100);
        let serialized = serialize_entry(&HistoryEntry::Ai(entry), None);
        let parsed: serde_json::Value =
            serde_json::from_str(&serialized).expect("serializer output must be valid JSON");
        let response = parsed["response"].as_str().expect("response is a string");
        assert!(response.ends_with(AI_TEXT_TRUNCATED_MARKER));
        assert_eq!(
            response.len(),
            AI_TEXT_CAP_BYTES + AI_TEXT_TRUNCATED_MARKER.len()
        );
    }

    #[test]
    fn ai_text_at_or_below_cap_is_not_truncated() {
        let mut entry = sample_ai_entry();
        entry.prompt = "x".repeat(AI_TEXT_CAP_BYTES);
        let serialized = serialize_entry(&HistoryEntry::Ai(entry), None);
        let parsed: serde_json::Value = serde_json::from_str(&serialized).unwrap();
        let prompt = parsed["prompt"].as_str().unwrap();
        assert_eq!(prompt.len(), AI_TEXT_CAP_BYTES);
        assert!(!prompt.contains("[truncated"));
    }

    #[test]
    fn ai_text_truncation_respects_utf8_char_boundary() {
        // 4-byte char repeated; the cap won't land on a boundary, so
        // the truncator must back off rather than panic on slice.
        let mut entry = sample_ai_entry();
        let pad = "\u{1F600}".repeat((AI_TEXT_CAP_BYTES / 4) + 20);
        entry.prompt = pad;
        let serialized = serialize_entry(&HistoryEntry::Ai(entry), None);
        // Must round-trip as valid JSON — a slice through a multibyte
        // char would corrupt the string and serde_json would still
        // emit (because String tolerates lone bytes via Utf8Error
        // panic during slice). The real guard is "no panic at write
        // time", asserted by the serialize call returning.
        let parsed: serde_json::Value = serde_json::from_str(&serialized).unwrap();
        assert!(parsed["prompt"]
            .as_str()
            .unwrap()
            .ends_with(AI_TEXT_TRUNCATED_MARKER));
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
            let parsed: serde_json::Value = serde_json::from_str(line).expect("valid json");
            assert_eq!(parsed["v"], 2);
            assert_eq!(parsed["kind"], "query");
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
        let blank = HistoryEntry::Query(QueryEntry {
            sql: "   ".to_string(),
            ..sample_query_entry("placeholder")
        });
        store.push_entry(blank).expect("ok");
        assert!(store.store().is_empty());
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
    fn record_ai_appends_to_disk_and_ring() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("history.jsonl");
        let mut store = PersistentHistoryStore::load_tail(path.clone(), 100).expect("load");
        store.record_ai(sample_ai_entry()).expect("append");
        assert_eq!(store.store().len(), 1, "ring updated");

        let contents = fs::read_to_string(&path).expect("read");
        let parsed: serde_json::Value =
            serde_json::from_str(contents.lines().next().unwrap()).expect("valid json");
        assert_eq!(parsed["v"], 2);
        assert_eq!(parsed["kind"], "ai");
        assert_eq!(parsed["intent"], "explain");
        assert_eq!(parsed["provider"], "anthropic");
    }

    #[test]
    fn record_ai_with_no_path_updates_ring_only() {
        let mut store = PersistentHistoryStore::in_memory_only(100);
        store.record_ai(sample_ai_entry()).expect("ok");
        assert_eq!(store.store().len(), 1);
    }

    #[test]
    fn load_tail_reads_last_capacity_lines_newest_first() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("history.jsonl");
        let mut store = PersistentHistoryStore::load_tail(path.clone(), 100).expect("load");
        for i in 0u64..5 {
            let entry = HistoryEntry::Query(QueryEntry {
                duration_ms: i,
                ..sample_query_entry(&format!("SELECT {i}"))
            });
            store.push_entry(entry).expect("append");
        }

        // Reopen with capacity 3 — only the newest 3 should land in memory.
        let reopened = PersistentHistoryStore::load_tail(path, 3).expect("reload");
        let sqls: Vec<&str> = reopened.store().iter().map(query_sql).collect();
        assert_eq!(sqls, vec!["SELECT 4", "SELECT 3", "SELECT 2"]);
        assert_eq!(reopened.skipped_on_load(), 0);
    }

    #[test]
    fn load_tail_skips_malformed_lines_and_counts_them() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("history.jsonl");
        let mut hand_written = String::new();
        // Two valid records, one truncated mid-write, one with v=3,
        // one totally junk. Loader keeps the two valid ones and reports
        // 3 skips.
        hand_written.push_str(&serialize_entry(&sample_entry("SELECT 1"), None));
        hand_written.push('\n');
        hand_written.push_str(r#"{"v":1,"ts":"2026-06-04T14:22:01"#); // truncated
        hand_written.push('\n');
        hand_written.push_str(r#"{"v":3,"kind":"query","ts":"2026-06-04T14:22:01.000Z","conn":"x","actor":null,"sql":"S","status":"ok","duration_ms":1,"rows":null,"rows_affected":null,"error":null}"#);
        hand_written.push('\n');
        hand_written.push_str("not-json-at-all");
        hand_written.push('\n');
        hand_written.push_str(&serialize_entry(&sample_entry("SELECT 2"), None));
        hand_written.push('\n');
        fs::write(&path, hand_written).expect("seed");

        let store = PersistentHistoryStore::load_tail(path, 100).expect("load");
        assert_eq!(store.store().len(), 2);
        assert_eq!(store.skipped_on_load(), 3);
        let sqls: Vec<&str> = store.store().iter().map(query_sql).collect();
        assert_eq!(sqls, vec!["SELECT 2", "SELECT 1"]);
    }

    #[test]
    fn load_tail_back_compat_reads_v1_and_v2_records_together() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("history.jsonl");
        // A v:1 line (legacy) and a v:2 line (current). Both must
        // load; the v:1 line dispatches as Query implicitly.
        let mut seed = String::new();
        seed.push_str(r#"{"v":1,"ts":"2026-06-04T14:22:01.000Z","conn":"legacy","actor":null,"sql":"SELECT legacy","status":"ok","duration_ms":1,"rows":1,"rows_affected":null,"error":null}"#);
        seed.push('\n');
        seed.push_str(&serialize_entry(&sample_entry("SELECT current"), None));
        seed.push('\n');
        fs::write(&path, seed).expect("seed");

        let store = PersistentHistoryStore::load_tail(path, 100).expect("load");
        assert_eq!(store.store().len(), 2);
        assert_eq!(store.skipped_on_load(), 0);
        let sqls: Vec<&str> = store.store().iter().map(query_sql).collect();
        assert_eq!(sqls, vec!["SELECT current", "SELECT legacy"]);
    }

    #[test]
    fn load_tail_rotates_when_line_cap_is_exceeded() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("history.jsonl");
        // Seed ROTATION_LINES + 1 minimal valid records cheaply so the
        // line-count threshold trips without producing a 50 MiB file.
        let mut seed = String::with_capacity(ROTATION_LINES * 16);
        let one = serialize_entry(&sample_entry("SELECT 1"), None);
        for _ in 0..=ROTATION_LINES {
            seed.push_str(&one);
            seed.push('\n');
        }
        fs::write(&path, seed).expect("seed");

        let store = PersistentHistoryStore::load_tail(path.clone(), 100).expect("load");
        assert!(!path.exists(), "history.jsonl must be rotated away");
        let backup = path.with_extension("jsonl.1");
        assert!(backup.exists(), "history.jsonl.1 must hold the old data");
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

        let mut seed = String::new();
        let one = serialize_entry(&sample_entry("SELECT 1"), None);
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
        let two = path.with_extension("jsonl.1.1");
        assert!(!two.exists());
    }

    #[test]
    fn record_submit_pushes_to_memory_without_touching_disk() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("history.jsonl");
        let mut store = PersistentHistoryStore::load_tail(path.clone(), 100).expect("load");
        store.record_submit("SELECT 1");
        assert_eq!(store.store().len(), 1);
        assert_eq!(query_sql(store.store().iter().next().unwrap()), "SELECT 1");
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
        assert_eq!(parsed["v"], 2);
        assert_eq!(parsed["kind"], "query");
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
        let blank = HistoryEntry::Query(QueryEntry {
            sql: "   ".to_string(),
            ..sample_query_entry("placeholder")
        });
        store.record_completion(&blank).expect("ok");
        assert!(!path.exists() || fs::read_to_string(&path).unwrap().is_empty());
    }

    // ---- Fixture-emission helpers (ADR-0017 cross-implementation round-trip) ----

    #[test]
    fn fixture_serialize_writes_null_actor_by_default() {
        let entry = sample_entry("SELECT 1");
        let line = fixture::serialize(&entry, None);
        assert!(
            line.contains(r#""actor":null"#),
            "default actor must serialise as null: {line}"
        );
    }

    #[test]
    fn fixture_serialize_overrides_actor() {
        let entry = sample_entry("SELECT 1");
        let line = fixture::serialize(&entry, Some("alice@example.com"));
        assert!(
            line.contains(r#""actor":"alice@example.com""#),
            "actor override must propagate: {line}"
        );
        let parsed: ParseRecord = serde_json::from_str(&line).expect("valid json");
        assert_eq!(parsed.actor.as_deref(), Some("alice@example.com"));
    }

    #[test]
    fn fixture_serialize_query_preserves_declaration_field_order() {
        // The web sibling's byte-equivalence check depends on
        // declaration order. v:2 inserts `kind` after `v`; the rest of
        // the order is unchanged from v:1.
        let entry = sample_entry("SELECT 1");
        let line = fixture::serialize(&entry, None);
        let expected_order = [
            r#""v":"#,
            r#""kind":"#,
            r#""ts":"#,
            r#""conn":"#,
            r#""actor":"#,
            r#""sql":"#,
            r#""status":"#,
            r#""duration_ms":"#,
            r#""rows":"#,
            r#""rows_affected":"#,
            r#""error":"#,
        ];
        assert_field_order(&line, &expected_order);
    }

    #[test]
    fn fixture_serialize_ai_preserves_declaration_field_order() {
        let entry = HistoryEntry::Ai(sample_ai_entry());
        let line = fixture::serialize(&entry, None);
        let expected_order = [
            r#""v":"#,
            r#""kind":"#,
            r#""ts":"#,
            r#""conn":"#,
            r#""actor":"#,
            r#""intent":"#,
            r#""prompt":"#,
            r#""response":"#,
            r#""status":"#,
            r#""duration_ms":"#,
            r#""tokens_in":"#,
            r#""tokens_out":"#,
            r#""provider":"#,
            r#""model":"#,
            r#""stop_reason":"#,
            r#""error":"#,
        ];
        assert_field_order(&line, &expected_order);
    }

    fn assert_field_order(line: &str, expected_order: &[&str]) {
        let mut last_pos = 0usize;
        for needle in expected_order {
            let pos = line[last_pos..]
                .find(needle)
                .unwrap_or_else(|| panic!("missing {needle} after offset {last_pos} in {line}"));
            last_pos += pos + needle.len();
        }
    }

    #[test]
    fn fixture_serialize_with_extra_appends_after_standard_envelope() {
        let entry = sample_entry("SELECT 1");
        let line = fixture::serialize_with_extra(&entry, "unknown_field", "value-from-the-future");
        assert!(
            line.ends_with(r#","unknown_field":"value-from-the-future"}"#),
            "extra field must come at the end: {line}"
        );
        let error_pos = line.find(r#""error":null"#).expect("error field present");
        let extra_pos = line
            .find(r#""unknown_field":"#)
            .expect("extra field present");
        assert!(
            error_pos < extra_pos,
            "extra field must follow declaration-order fields: {line}"
        );
    }

    #[test]
    fn fixture_serialize_with_extra_round_trips_through_parser_ignoring_extra() {
        // The current reader's ParseRecord silently drops unknown
        // fields (matches ADR-0017 §6 / ADR-0027 forward-compat). The
        // fixture must continue to parse so future drift surfaces in
        // the byte comparison rather than at parse time.
        let entry = sample_entry("SELECT 1");
        let line = fixture::serialize_with_extra(&entry, "unknown_field", "value-from-the-future");
        let parsed: ParseRecord = serde_json::from_str(&line).expect("known fields must parse");
        let back = parsed.into_entry().expect("v=2 status=ok must round-trip");
        assert_eq!(back, entry);
    }

    #[test]
    fn fixture_serialize_with_extra_escapes_special_characters() {
        let entry = sample_entry("SELECT 1");
        let line = fixture::serialize_with_extra(&entry, "with\"quote", "with\\backslash");
        let parsed: serde_json::Value = serde_json::from_str(&line).expect("valid json");
        assert_eq!(parsed["with\"quote"], "with\\backslash");
    }

    #[test]
    fn fixture_serialize_with_extra_works_for_ai_records() {
        let entry = HistoryEntry::Ai(sample_ai_entry());
        let line = fixture::serialize_with_extra(&entry, "telemetry_id", "abc123");
        assert!(line.ends_with(r#","telemetry_id":"abc123"}"#));
        let parsed: ParseRecord = serde_json::from_str(&line).expect("known fields must parse");
        let back = parsed.into_entry().expect("v=2 kind=ai must round-trip");
        assert_eq!(back, entry);
    }
}
