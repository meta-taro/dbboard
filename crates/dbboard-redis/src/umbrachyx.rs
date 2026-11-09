//! Umbrachyx's history, read through its `UMBRA.` commands (ADR-0165,
//! ADR-0170).
//!
//! Umbrachyx speaks the Redis protocol for one branch and answers five more
//! commands about its history, each with one string of JSON. A Redis
//! connection counts as Umbrachyx when the server answers `UMBRA.BRANCHES`
//! with the branch it serves; any other answer, an error included, means a
//! plain Redis-compatible store. Every command here reads.
//!
//! A "table" is a key pattern from the sidebar (`player/*`), a "row" is one
//! key, and its columns are the key and the value. The history shapes have
//! no place for who approved a change, for an email, or for a value whose
//! subject was erased: the first two are left out and an erased value reads
//! as `Null`, as ADR-0170 records.

use async_trait::async_trait;
use dbboard_core::{
    too_many_rows_error, BlameEntry, Column, ColumnInfo, Commit, CommitDiff, DbError, DbResult,
    HistoryReader, Row, RowChange, RowHistory, RowVersion, TableInfo, TableSchema, Value,
    MAX_RESULT_ROWS,
};
use serde::Deserialize;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use crate::adapter::{send, Connection};

/// Asked once when a connection opens; only Umbrachyx answers it.
pub(crate) const DETECT: &str = "UMBRA.BRANCHES";

/// Umbrachyx separates the parts of a key with `/`, and erases by the first
/// two of them (`player/42`), so its keys group by `/` rather than `:`.
pub(crate) const DELIMITER: char = '/';

/// The most `UMBRA.CELLS` pages one blame reads, each up to 1,000 keys.
const MAX_CELL_PAGES: usize = 20;

pub(crate) struct UmbrachyxHistory {
    connection: Connection,
    /// The branch the server writes to, whose history is shown.
    branch: String,
}

impl UmbrachyxHistory {
    /// The history of the server behind `connection`, or `None` when it is
    /// not Umbrachyx. Never an error: a server that does not know the
    /// command is simply another Redis-compatible store.
    pub(crate) async fn detect(connection: &Connection) -> Option<Self> {
        let reply = send(connection, &redis::cmd(DETECT)).await.ok()?;
        let branch = served_branch(&reply_text(reply).ok()?)?;
        Some(Self {
            connection: connection.clone(),
            branch,
        })
    }

    async fn ask(&self, name: &str, args: &[&[u8]]) -> DbResult<String> {
        let mut cmd = redis::cmd(name);
        for arg in args {
            cmd.arg(*arg);
        }
        reply_text(send(&self.connection, &cmd).await?)
    }
}

#[async_trait]
impl HistoryReader for UmbrachyxHistory {
    async fn commit_log(&self, limit: usize) -> DbResult<Vec<Commit>> {
        let reply = self.ask("UMBRA.LOG", &[self.branch.as_bytes()]).await?;
        commit_log(&reply, limit)
    }

    async fn row_history(
        &self,
        _table: &TableInfo,
        key: &[(String, Value)],
        limit: usize,
    ) -> DbResult<RowHistory> {
        let key = key_of(key)?;
        let reply = self
            .ask("UMBRA.HISTORY", &[self.branch.as_bytes(), key.as_bytes()])
            .await?;
        row_history(&reply, limit)
    }

    async fn diff(&self, table: &TableInfo, from: &str, to: &str) -> DbResult<CommitDiff> {
        let reply = self
            .ask("UMBRA.DIFF", &[from.as_bytes(), to.as_bytes()])
            .await?;
        diff(&reply, &table.name)
    }

    async fn blame(&self, table: &TableInfo) -> DbResult<Vec<BlameEntry>> {
        let pattern = &table.name;
        let mut after = start_before(&literal_prefix(pattern));
        let mut entries = Vec::new();
        for _ in 0..MAX_CELL_PAGES {
            let mut args: Vec<&[u8]> = vec![self.branch.as_bytes()];
            if let Some(after) = &after {
                args.push(after);
            }
            let page = cells(&self.ask("UMBRA.CELLS", &args).await?)?;
            let (found, next) = blame_page(&page, pattern);
            entries.extend(found);
            if entries.len() > MAX_RESULT_ROWS {
                return Err(too_many_rows_error());
            }
            let Some(next) = next else {
                return Ok(entries);
            };
            after = Some(next);
        }
        Err(DbError::Query(format!(
            "more than {MAX_CELL_PAGES} pages of keys come before the end of {pattern}: \
             pick a narrower prefix"
        )))
    }
}

// -- The replies ----------------------------------------------------------

#[derive(Deserialize)]
struct Branches {
    served: String,
}

#[derive(Deserialize)]
struct LogReply {
    commits: Vec<CommitReply>,
}

#[derive(Deserialize)]
struct CommitReply {
    commit: String,
    time_us: u64,
    who: String,
    why: String,
}

#[derive(Deserialize)]
struct HistoryReply {
    key: serde_json::Value,
    changes: Vec<ChangeReply>,
}

/// One change to a key, as `UMBRA.HISTORY` and `UMBRA.CELLS` give it.
#[derive(Deserialize)]
struct ChangeReply {
    #[serde(default)]
    key: Option<serde_json::Value>,
    value: serde_json::Value,
    commit: String,
    time_us: u64,
    who: String,
    why: String,
}

#[derive(Deserialize)]
struct DiffReply {
    changes: Vec<ValueChangeReply>,
    more: bool,
}

#[derive(Deserialize)]
struct ValueChangeReply {
    key: serde_json::Value,
    before: serde_json::Value,
    after: serde_json::Value,
}

#[derive(Deserialize)]
struct CellsReply {
    cells: Vec<ChangeReply>,
    next: Option<serde_json::Value>,
}

fn parse<'a, T: Deserialize<'a>>(reply: &'a str, command: &str) -> DbResult<T> {
    serde_json::from_str(reply)
        .map_err(|e| DbError::TypeConversion(format!("{command} answered unreadable JSON: {e}")))
}

fn served_branch(reply: &str) -> Option<String> {
    serde_json::from_str::<Branches>(reply)
        .ok()
        .map(|b| b.served)
}

fn commit_log(reply: &str, limit: usize) -> DbResult<Vec<Commit>> {
    let log: LogReply = parse(reply, "UMBRA.LOG")?;
    Ok(log
        .commits
        .iter()
        .take(limit)
        .map(|c| commit(&c.commit, c.time_us, &c.who, &c.why))
        .collect())
}

fn row_history(reply: &str, limit: usize) -> DbResult<RowHistory> {
    let history: HistoryReply = parse(reply, "UMBRA.HISTORY")?;
    let key = value(&history.key);
    let versions = history
        .changes
        .iter()
        .take(limit)
        .map(|change| RowVersion {
            commit: commit(&change.commit, change.time_us, &change.who, &change.why),
            values: Row::new(vec![key.clone(), value(&change.value)]),
        })
        .collect();
    Ok(RowHistory {
        columns: columns(),
        versions,
    })
}

/// The keys of the diff that `pattern` matches. Umbrachyx gives one page of
/// changed keys; a diff with more is refused rather than shown in part as
/// if it were whole.
fn diff(reply: &str, pattern: &str) -> DbResult<CommitDiff> {
    let diff: DiffReply = parse(reply, "UMBRA.DIFF")?;
    if diff.more {
        return Err(DbError::Query(
            "more keys changed between these commits than Umbrachyx answers at once (1,000), \
             and part of a diff would read as the whole of it"
                .into(),
        ));
    }
    let changes = diff
        .changes
        .iter()
        .filter(|change| {
            change
                .key
                .as_str()
                .is_some_and(|k| glob_matches(pattern, k))
        })
        .filter_map(|change| {
            let key = value(&change.key);
            let side = |shown: &serde_json::Value| {
                (!shown.is_null()).then(|| Row::new(vec![key.clone(), value(shown)]))
            };
            RowChange::between(side(&change.before), side(&change.after))
        })
        .collect();
    Ok(CommitDiff {
        columns: columns(),
        changes,
    })
}

fn cells(reply: &str) -> DbResult<CellsReply> {
    parse(reply, "UMBRA.CELLS")
}

/// The entries of one page that `pattern` matches, and the key to go on
/// from: `None` once the page is the last, or has gone past every key the
/// pattern can match.
fn blame_page(page: &CellsReply, pattern: &str) -> (Vec<BlameEntry>, Option<Vec<u8>>) {
    let prefix = literal_prefix(pattern);
    let mut entries = Vec::new();
    for cell in &page.cells {
        // Keys that are not text cannot be named in a pattern.
        let Some(key) = cell.key.as_ref().and_then(serde_json::Value::as_str) else {
            continue;
        };
        if key > prefix.as_str() && !key.starts_with(&prefix) {
            return (entries, None);
        }
        if glob_matches(pattern, key) {
            entries.push(BlameEntry {
                key: Row::new(vec![Value::Text(key.to_owned())]),
                commit: commit(&cell.commit, cell.time_us, &cell.who, &cell.why),
            });
        }
    }
    (entries, page.next.as_ref().and_then(key_bytes))
}

// -- Pieces ---------------------------------------------------------------

/// A pattern described as the history views see it: a key, which is the
/// primary key, and its value. The views offer a row's history and blame
/// only on a table with a primary key, so without this both stayed closed.
pub(crate) fn schema(table: &TableInfo) -> TableSchema {
    let columns = columns()
        .into_iter()
        .zip(1u32..)
        .map(|(column, ordinal)| {
            let key = ordinal == 1;
            ColumnInfo {
                name: column.name,
                declared_type: None,
                nullable: !key,
                primary_key: key,
                ordinal,
                default_value: None,
            }
        })
        .collect();
    TableSchema {
        table: table.clone(),
        columns,
        primary_key: vec!["key".into()],
    }
}

fn columns() -> Vec<Column> {
    ["key", "value"]
        .into_iter()
        .map(|name| Column {
            name: name.into(),
            declared_type: None,
        })
        .collect()
}

/// A commit as the history views show it. Umbrachyx keeps no email, and who
/// approved a change has no field here (ADR-0170).
fn commit(id: &str, time_us: u64, who: &str, why: &str) -> Commit {
    Commit {
        hash: id.into(),
        author: who.into(),
        email: String::new(),
        date: date(time_us),
        message: why.into(),
    }
}

/// Microseconds since 1970 as an RFC 3339 time in UTC.
fn date(time_us: u64) -> String {
    i128::from(time_us)
        .checked_mul(1_000)
        .and_then(|ns| OffsetDateTime::from_unix_timestamp_nanos(ns).ok())
        .and_then(|t| t.format(&Rfc3339).ok())
        .unwrap_or_else(|| time_us.to_string())
}

/// A value as Umbrachyx writes it: text, `{"hex": ...}` for bytes, or
/// `null` for none. `null` also stands for a value whose subject was
/// erased (ADR-0170).
fn value(shown: &serde_json::Value) -> Value {
    match shown {
        serde_json::Value::String(text) => Value::Text(text.clone()),
        serde_json::Value::Object(_) => key_bytes(shown).map_or(Value::Null, Value::Blob),
        _ => Value::Null,
    }
}

/// The bytes of a key or value as Umbrachyx writes them.
fn key_bytes(shown: &serde_json::Value) -> Option<Vec<u8>> {
    if let Some(text) = shown.as_str() {
        return Some(text.as_bytes().to_vec());
    }
    let hex = shown.get("hex")?.as_str()?;
    if hex.len() % 2 != 0 {
        return None;
    }
    (0..hex.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(hex.get(at..at + 2)?, 16).ok())
        .collect()
}

fn key_of(key: &[(String, Value)]) -> DbResult<String> {
    match key {
        [(_, Value::Text(key))] => Ok(key.clone()),
        _ => Err(DbError::Query(
            "an Umbrachyx row is named by one text key".into(),
        )),
    }
}

/// The reply to a command, which Umbrachyx sends as one string.
fn reply_text(reply: redis::Value) -> DbResult<String> {
    match reply {
        redis::Value::BulkString(bytes) => String::from_utf8(bytes)
            .map_err(|_| DbError::TypeConversion("the reply is not UTF-8".into())),
        redis::Value::SimpleString(text) => Ok(text),
        _ => Err(DbError::TypeConversion(
            "Umbrachyx answered something other than a string".into(),
        )),
    }
}

/// The part of a `MATCH` pattern before its first wildcard, unescaped.
fn literal_prefix(pattern: &str) -> String {
    let mut prefix = String::new();
    let mut chars = pattern.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => prefix.extend(chars.next()),
            '*' | '?' | '[' => break,
            c => prefix.push(c),
        }
    }
    prefix
}

/// A key just before every key that starts with `prefix`, for `CELLS` to go
/// on from: the prefix without its last byte. `None` starts at the first
/// key.
fn start_before(prefix: &str) -> Option<Vec<u8>> {
    let bytes = prefix.as_bytes();
    (!bytes.is_empty()).then(|| bytes[..bytes.len() - 1].to_vec())
}

/// Whether `key` matches a `MATCH` pattern of the kind the sidebar makes:
/// `*` for any run, `?` for one character, `\` to take the next one as it
/// is. The sidebar escapes `[` and `]`, so classes are not needed.
fn glob_matches(pattern: &str, key: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    let key: Vec<char> = key.chars().collect();
    matches_from(&pattern, &key)
}

fn matches_from(pattern: &[char], key: &[char]) -> bool {
    match pattern.split_first() {
        None => key.is_empty(),
        Some(('*', rest)) => (0..=key.len()).any(|skip| matches_from(rest, &key[skip..])),
        Some(('?', rest)) => !key.is_empty() && matches_from(rest, &key[1..]),
        Some(('\\', rest)) if !rest.is_empty() => {
            key.first() == Some(&rest[0]) && matches_from(&rest[1..], &key[1..])
        }
        Some((c, rest)) => key.first() == Some(c) && matches_from(rest, &key[1..]),
    }
}

#[cfg(test)]
mod tests {
    // The history views ask for a table's primary key before they offer a
    // row's history or blame, and a table with none gets neither. Without
    // this the two views Umbrachyx answers stayed closed on every pattern.
    #[test]
    fn a_pattern_is_keyed_by_its_key_column() {
        let table = TableInfo::unqualified("player/*");
        let schema = schema(&table);
        assert_eq!(schema.table, table);
        assert_eq!(schema.primary_key, vec!["key".to_string()]);
        let names: Vec<_> = schema.columns.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["key", "value"]);
        assert!(schema.columns[0].primary_key && !schema.columns[0].nullable);
        assert!(!schema.columns[1].primary_key && schema.columns[1].nullable);
        // The same names, in the same order, as the rows the history returns.
        let row_names: Vec<_> = columns().into_iter().map(|c| c.name).collect();
        assert_eq!(
            names,
            row_names.iter().map(String::as_str).collect::<Vec<_>>()
        );
    }

    use super::*;
    use dbboard_core::ChangeKind;

    // Replies recorded from `umbrachyx serve agent` on a synthetic store:
    // two commits on main, a branch `agent` with one more, and player/42
    // erased. The data is fictional.
    const BRANCHES: &str = r#"{"branches":[{"head":"e03e88ddd219","name":"agent","time_us":1793939613801526},{"head":"5fe75ebd219b","name":"main","time_us":1793939613779943}],"served":"agent"}"#;
    const LOG: &str = r#"{"branch":"agent","commits":[{"approved_by":null,"branch":"agent","commit":"e03e88ddd219","merged":null,"parent":"5fe75ebd219b","time_us":1793939613801526,"who":"agent:support","why":"refund for a lost item"},{"approved_by":"ops:lead","branch":"main","commit":"5fe75ebd219b","merged":null,"parent":"d5b580eee802","time_us":1793939613779943,"who":"game:shop","why":"bought a potion"},{"approved_by":null,"branch":"main","commit":"d5b580eee802","merged":null,"parent":null,"time_us":1793939613767077,"who":"game:onboarding","why":"starter grant"}],"more":false}"#;
    const HISTORY: &str = r#"{"branch":"agent","changes":[{"approved_by":null,"commit":"e03e88ddd219","erased":false,"time_us":1793939613801526,"value":"45","who":"agent:support","why":"refund for a lost item"},{"approved_by":null,"commit":"d5b580eee802","erased":false,"time_us":1793939613767077,"value":"30","who":"game:onboarding","why":"starter grant"}],"key":"player/7/gems","more":false}"#;
    const ERASED_HISTORY: &str = r#"{"branch":"agent","changes":[{"approved_by":"ops:lead","commit":"5fe75ebd219b","erased":true,"time_us":1793939613779943,"value":null,"who":"game:shop","why":"bought a potion"},{"approved_by":null,"commit":"d5b580eee802","erased":true,"time_us":1793939613767077,"value":null,"who":"game:onboarding","why":"starter grant"}],"key":"player/42/gems","more":false}"#;
    const DIFF: &str = r#"{"changes":[{"after":"45","before":"30","erased":false,"key":"player/7/gems"}],"from":"agent~1","more":false,"to":"agent"}"#;
    const CELLS: &str = r#"{"branch":"agent","cells":[{"approved_by":null,"commit":"d5b580eee802","erased":true,"key":"player/42/email","time_us":1793939613767077,"value":null,"who":"game:onboarding","why":"starter grant"},{"approved_by":"ops:lead","commit":"5fe75ebd219b","erased":true,"key":"player/42/gems","time_us":1793939613779943,"value":null,"who":"game:shop","why":"bought a potion"},{"approved_by":null,"commit":"e03e88ddd219","erased":false,"key":"player/7/gems","time_us":1793939613801526,"value":"45","who":"agent:support","why":"refund for a lost item"}],"next":null}"#;

    fn text(value: &str) -> Value {
        Value::Text(value.into())
    }

    #[test]
    fn the_served_branch_is_what_detection_reads() {
        assert_eq!(served_branch(BRANCHES).as_deref(), Some("agent"));
        assert_eq!(served_branch("ERR unknown command"), None);
        assert_eq!(served_branch(r#"{"branches":[]}"#), None);
    }

    #[test]
    fn the_log_reads_who_why_and_when() {
        let log = commit_log(LOG, 2).expect("parses");
        assert_eq!(log.len(), 2);
        assert_eq!(log[0].hash, "e03e88ddd219");
        assert_eq!(log[0].author, "agent:support");
        assert_eq!(log[0].message, "refund for a lost item");
        assert_eq!(log[0].email, "");
        assert_eq!(log[0].date, "2026-11-06T04:33:33.801526Z");
    }

    #[test]
    fn a_keys_history_is_its_values_newest_first() {
        let history = row_history(HISTORY, 10).expect("parses");
        assert_eq!(history.columns.len(), 2);
        let values: Vec<_> = history.versions.iter().map(|v| v.values.clone()).collect();
        assert_eq!(
            values,
            [
                Row::new(vec![text("player/7/gems"), text("45")]),
                Row::new(vec![text("player/7/gems"), text("30")]),
            ]
        );
    }

    #[test]
    fn an_erased_value_reads_as_null_and_never_as_its_value() {
        let history = row_history(ERASED_HISTORY, 10).expect("parses");
        let erased = Row::new(vec![text("player/42/gems"), Value::Null]);
        assert!(history.versions.iter().all(|v| v.values == erased));
        assert_eq!(history.versions[0].commit.message, "bought a potion");
    }

    #[test]
    fn a_diff_marks_the_changed_value_of_the_keys_the_pattern_matches() {
        let inside = diff(DIFF, "player/*").expect("parses");
        assert_eq!(inside.changes.len(), 1);
        assert_eq!(inside.changes[0].kind, ChangeKind::Modified);
        assert_eq!(inside.changes[0].changed_columns, vec![1]);
        let outside = diff(DIFF, "order/*").expect("parses");
        assert!(outside.changes.is_empty());
    }

    #[test]
    fn a_diff_longer_than_a_page_is_refused_not_cut() {
        let long = DIFF.replace(r#""more":false"#, r#""more":true"#);
        assert!(matches!(diff(&long, "player/*"), Err(DbError::Query(_))));
    }

    #[test]
    fn blame_names_the_last_commit_of_each_matching_key() {
        let page = cells(CELLS).expect("parses");
        let (entries, next) = blame_page(&page, "player/*");
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[1].key, Row::new(vec![text("player/42/gems")]));
        assert_eq!(entries[1].commit.author, "game:shop");
        assert_eq!(next, None);
        let (none, next) = blame_page(&page, "order/*");
        assert!(none.is_empty());
        assert_eq!(next, None);
    }

    #[test]
    fn a_blame_walk_stops_at_the_first_key_past_the_prefix() {
        let more = CELLS.replace(r#""next":null"#, r#""next":"player/7/gems""#);
        let page = cells(&more).expect("parses");
        let (entries, next) = blame_page(&page, "player/4*");
        assert_eq!(entries.len(), 2);
        assert_eq!(next, None, "player/7 is past every player/4 key");
        let (_, next) = blame_page(&page, "player/*");
        assert_eq!(next.as_deref(), Some(&b"player/7/gems"[..]));
    }

    #[test]
    fn patterns_match_as_the_sidebar_writes_them() {
        assert!(glob_matches("player/*", "player/7/gems"));
        assert!(glob_matches("player/*", "player/"));
        assert!(!glob_matches("player/*", "players/7"));
        assert!(glob_matches("a\\*b/*", "a*b/1"));
        assert!(!glob_matches("a\\*b/*", "axb/1"));
        assert!(glob_matches("config", "config"));
        assert!(!glob_matches("config", "configs"));
        assert_eq!(literal_prefix("a\\*b/*"), "a*b/");
        assert_eq!(start_before("player/"), Some(b"player".to_vec()));
        assert_eq!(start_before(""), None);
    }

    #[test]
    fn values_that_are_bytes_come_back_as_bytes() {
        assert_eq!(
            value(&serde_json::json!({"hex": "ff00"})),
            Value::Blob(vec![0xff, 0x00])
        );
        assert_eq!(key_bytes(&serde_json::json!({"hex": "f"})), None);
    }

    #[test]
    fn a_row_is_named_by_one_text_key() {
        assert!(key_of(&[("key".into(), text("player/7/gems"))]).is_ok());
        assert!(key_of(&[]).is_err());
        assert!(key_of(&[("key".into(), Value::Integer(1))]).is_err());
    }
}
