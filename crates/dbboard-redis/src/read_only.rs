//! The read-only classifier: the one gate between text a person or an agent
//! typed and the server (ADR-0166).
//!
//! RESP gives no hint which commands write, so, as with `MongoDB`
//! (ADR-0095), the guarantee is an allowlist. A command is sent only when its
//! name is on the list *and* every argument is one this module understands;
//! anything else fails closed. Unknown is a refusal, not a pass.
//!
//! Two more rules come from what is cheap for the server rather than from
//! what writes:
//!
//! - `KEYS` is refused outright. It walks the whole keyspace in one call and
//!   blocks every other client while it does; `SCAN` is the same answer a
//!   page at a time.
//! - A command that returns a whole collection (`HGETALL`, `SMEMBERS`,
//!   `LRANGE 0 -1`, …) carries a [`SizeGuard`]: the adapter asks for the
//!   collection's size first and refuses when it is over the row cap, so a
//!   million-member set never reaches the wire.
//!
//! A refusal names the command, never its arguments: key names are data,
//! and data can identify people (ADR-0055).

use std::fmt;

use dbboard_core::{DbError, MAX_RESULT_ROWS};

use crate::command::{split, SplitError};

/// The most a `COUNT` hint may ask for in one `SCAN`-family call. The server
/// treats it as a hint, so this bounds the work per call, not the answer.
pub const MAX_SCAN_COUNT: usize = 1000;

/// How the reply is laid out, which decides its columns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    /// One value: `GET`, `TTL`, `HLEN`, …
    Value,
    /// A flat list of values, one per row: `MGET`, `LRANGE`, `SMEMBERS`, …
    Values,
    /// Alternating field and value: `HGETALL`.
    FieldValues,
    /// Alternating member and score: `ZRANGE … WITHSCORES`.
    MemberScores,
    /// A `SCAN` reply: a cursor and a page of keys.
    ScanKeys,
    /// An `HSCAN` reply: a cursor and alternating fields and values.
    ScanFieldValues,
    /// An `SSCAN` reply, or `HSCAN … NOVALUES`: a cursor and members.
    ScanMembers,
    /// A `ZSCAN` reply: a cursor and alternating members and scores.
    ScanMemberScores,
    /// Stream entries: an id and its field-value pairs.
    Entries,
    /// `INFO`: `name:value` lines.
    Info,
}

/// The size a command must check before it runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SizeGuard {
    /// The command that answers the collection's size: `HLEN`, `SCARD`, ….
    pub command: &'static str,
    pub key: String,
}

/// A command the classifier approved, ready to send as it stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadPlan {
    /// The command name, upper-cased, then its arguments untouched.
    pub words: Vec<String>,
    pub shape: Shape,
    pub guard: Option<SizeGuard>,
}

impl ReadPlan {
    #[must_use]
    pub fn name(&self) -> &str {
        &self.words[0]
    }
}

/// Why a command was not sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Violation {
    Split(SplitError),
    Empty,
    /// Not on the allowlist: a write, an admin command, or one this version
    /// does not know.
    NotAllowed(String),
    /// `KEYS`: a read, but one that blocks the server.
    Keys,
    WrongArguments(String),
    UnknownOption {
        command: String,
        option: String,
    },
    CountTooLarge {
        command: String,
        max: usize,
    },
    /// A stream range must say how many entries it wants.
    CountRequired(String),
}

impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Split(e) => write!(f, "the command could not be read: {e}"),
            Self::Empty => f.write_str("there is no command to run"),
            Self::NotAllowed(name) => write!(
                f,
                "{name} is not a command dbboard sends: only reads from its allowlist reach the server (ADR-0166)"
            ),
            Self::Keys => f.write_str(
                "KEYS reads every key in one call and blocks the server while it does: use SCAN 0 MATCH <pattern> COUNT 100",
            ),
            Self::WrongArguments(name) => write!(f, "{name} was given the wrong number of arguments"),
            Self::UnknownOption { command, option } => {
                write!(f, "{command} does not take {option} here, so it was not sent")
            }
            Self::CountTooLarge { command, max } => {
                write!(f, "{command} may ask for at most {max} at a time")
            }
            Self::CountRequired(name) => write!(
                f,
                "{name} needs COUNT <n> (at most {MAX_RESULT_ROWS}) so a long stream is not read whole"
            ),
        }
    }
}

impl From<Violation> for DbError {
    /// A refusal is a query error: the command was rejected before anything
    /// was sent.
    fn from(violation: Violation) -> Self {
        DbError::Query(violation.to_string())
    }
}

/// Decide whether `line` may be sent, and how to read its reply.
///
/// # Errors
///
/// A [`Violation`] for anything not proven to be an allowed read.
pub fn classify(line: &str) -> Result<ReadPlan, Violation> {
    let mut words = split(line).map_err(Violation::Split)?;
    let Some(first) = words.first_mut() else {
        return Err(Violation::Empty);
    };
    *first = first.to_ascii_uppercase();
    let name = first.clone();
    let args = &words[1..];
    let (shape, guard) = rule(&name, args)?;
    Ok(ReadPlan {
        words,
        shape,
        guard,
    })
}

type Rule = Result<(Shape, Option<SizeGuard>), Violation>;

fn rule(name: &str, args: &[String]) -> Rule {
    match name {
        "PING" => arity(name, args, 0, 1).map(|()| plain(Shape::Value)),
        "DBSIZE" => arity(name, args, 0, 0).map(|()| plain(Shape::Value)),
        "INFO" => info(args),
        "TYPE" | "TTL" | "PTTL" | "EXPIRETIME" | "PEXPIRETIME" | "STRLEN" | "GET" | "HLEN"
        | "LLEN" | "SCARD" | "ZCARD" | "XLEN" => {
            arity(name, args, 1, 1).map(|()| plain(Shape::Value))
        }
        "HGET" | "HEXISTS" | "HSTRLEN" | "LINDEX" | "SISMEMBER" | "ZSCORE" => {
            arity(name, args, 2, 2).map(|()| plain(Shape::Value))
        }
        "GETRANGE" | "ZCOUNT" => arity(name, args, 3, 3).map(|()| plain(Shape::Value)),
        "EXISTS" | "MGET" => many(name, args, 1).map(|()| {
            plain(if name == "MGET" {
                Shape::Values
            } else {
                Shape::Value
            })
        }),
        "HMGET" | "SMISMEMBER" => many(name, args, 2).map(|()| plain(Shape::Values)),
        "ZRANK" | "ZREVRANK" => rank(name, args),
        "OBJECT" => object(args),
        "MEMORY" => memory(args),
        "HGETALL" => whole(name, args, "HLEN", Shape::FieldValues),
        "HKEYS" | "HVALS" => whole(name, args, "HLEN", Shape::Values),
        "SMEMBERS" => whole(name, args, "SCARD", Shape::Values),
        "LRANGE" => range(name, args, "LLEN"),
        "ZRANGE" => zrange(args),
        "SCAN" => scan(name, args, 0),
        "HSCAN" | "SSCAN" | "ZSCAN" => scan(name, args, 1),
        "XRANGE" | "XREVRANGE" => xrange(name, args),
        "XINFO" => xinfo(args),
        "KEYS" => Err(Violation::Keys),
        _ => Err(Violation::NotAllowed(printable(name))),
    }
}

fn plain(shape: Shape) -> (Shape, Option<SizeGuard>) {
    (shape, None)
}

fn arity(name: &str, args: &[String], min: usize, max: usize) -> Result<(), Violation> {
    if (min..=max).contains(&args.len()) {
        Ok(())
    } else {
        Err(Violation::WrongArguments(name.to_owned()))
    }
}

/// At least `min` arguments, and no more than one page of them.
fn many(name: &str, args: &[String], min: usize) -> Result<(), Violation> {
    arity(name, args, min, MAX_RESULT_ROWS)
}

fn info(args: &[String]) -> Rule {
    // A section name is a word. Anything else is not something INFO takes,
    // and refusing it costs nothing.
    if let Some(odd) = args
        .iter()
        .find(|a| a.is_empty() || !a.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))
    {
        return Err(unknown("INFO", odd));
    }
    Ok(plain(Shape::Info))
}

fn rank(name: &str, args: &[String]) -> Rule {
    arity(name, args, 2, 3)?;
    if let Some(option) = args.get(2) {
        if !option.eq_ignore_ascii_case("WITHSCORE") {
            return Err(unknown(name, option));
        }
        return Ok(plain(Shape::Values));
    }
    Ok(plain(Shape::Value))
}

fn object(args: &[String]) -> Rule {
    arity("OBJECT", args, 2, 2)?;
    let sub = args[0].to_ascii_uppercase();
    if matches!(sub.as_str(), "ENCODING" | "FREQ" | "IDLETIME" | "REFCOUNT") {
        Ok(plain(Shape::Value))
    } else {
        Err(unknown("OBJECT", &args[0]))
    }
}

fn memory(args: &[String]) -> Rule {
    // Only `MEMORY USAGE key [SAMPLES n]`. The other subcommands are server
    // administration (`PURGE`, `DOCTOR`, …).
    if args
        .first()
        .is_none_or(|s| !s.eq_ignore_ascii_case("USAGE"))
    {
        return Err(Violation::NotAllowed("MEMORY".into()));
    }
    match args.len() {
        2 => Ok(plain(Shape::Value)),
        4 if args[2].eq_ignore_ascii_case("SAMPLES") && args[3].parse::<u32>().is_ok() => {
            Ok(plain(Shape::Value))
        }
        4 => Err(unknown("MEMORY", &args[2])),
        _ => Err(Violation::WrongArguments("MEMORY".into())),
    }
}

fn whole(name: &str, args: &[String], len: &'static str, shape: Shape) -> Rule {
    arity(name, args, 1, 1)?;
    Ok((shape, Some(guard(len, &args[0]))))
}

fn guard(command: &'static str, key: &str) -> SizeGuard {
    SizeGuard {
        command,
        key: key.to_owned(),
    }
}

/// `LRANGE key start stop`. A range that is plainly small needs no size
/// check; one that counts from the end (`0 -1`) could be anything.
fn range(name: &str, args: &[String], len: &'static str) -> Rule {
    arity(name, args, 3, 3)?;
    let (start, stop) = indexes(name, &args[1], &args[2])?;
    let small = start >= 0 && stop >= start && stop - start < bound();
    let guard = (!small).then(|| guard(len, &args[0]));
    Ok((Shape::Values, guard))
}

/// `ZRANGE key start stop [REV] [WITHSCORES]`, by index only. `BYSCORE`,
/// `BYLEX` and `LIMIT` are refused rather than half-understood.
fn zrange(args: &[String]) -> Rule {
    if args.len() < 3 {
        return Err(Violation::WrongArguments("ZRANGE".into()));
    }
    let mut scores = false;
    for option in &args[3..] {
        match option.to_ascii_uppercase().as_str() {
            "WITHSCORES" if !scores => scores = true,
            "REV" => {}
            _ => return Err(unknown("ZRANGE", option)),
        }
    }
    let (_, guard) = range("ZRANGE", &args[..3], "ZCARD")?;
    let shape = if scores {
        Shape::MemberScores
    } else {
        Shape::Values
    };
    Ok((shape, guard))
}

fn indexes(name: &str, start: &str, stop: &str) -> Result<(i64, i64), Violation> {
    match (start.parse::<i64>(), stop.parse::<i64>()) {
        (Ok(a), Ok(b)) => Ok((a, b)),
        _ => Err(Violation::WrongArguments(name.to_owned())),
    }
}

fn bound() -> i64 {
    i64::try_from(MAX_RESULT_ROWS).unwrap_or(i64::MAX)
}

/// `SCAN cursor …` or `HSCAN key cursor …`: `keys` is how many key arguments
/// come before the cursor.
fn scan(name: &str, args: &[String], keys: usize) -> Rule {
    if args.len() < keys + 1 || args[keys].parse::<u64>().is_err() {
        return Err(Violation::WrongArguments(name.to_owned()));
    }
    let mut no_values = false;
    let mut rest = args[keys + 1..].iter();
    while let Some(option) = rest.next() {
        match option.to_ascii_uppercase().as_str() {
            "MATCH" => {
                rest.next()
                    .ok_or_else(|| Violation::WrongArguments(name.to_owned()))?;
            }
            "COUNT" => {
                let n = rest
                    .next()
                    .and_then(|n| n.parse::<usize>().ok())
                    .ok_or_else(|| Violation::WrongArguments(name.to_owned()))?;
                if n > MAX_SCAN_COUNT {
                    return Err(Violation::CountTooLarge {
                        command: name.to_owned(),
                        max: MAX_SCAN_COUNT,
                    });
                }
            }
            "TYPE" if name == "SCAN" => {
                rest.next()
                    .ok_or_else(|| Violation::WrongArguments(name.to_owned()))?;
            }
            "NOVALUES" if name == "HSCAN" => no_values = true,
            _ => return Err(unknown(name, option)),
        }
    }
    let shape = match name {
        "SCAN" => Shape::ScanKeys,
        "HSCAN" if no_values => Shape::ScanMembers,
        "HSCAN" => Shape::ScanFieldValues,
        "ZSCAN" => Shape::ScanMemberScores,
        _ => Shape::ScanMembers,
    };
    Ok(plain(shape))
}

/// `XRANGE key start end COUNT n`, where `COUNT` is required.
fn xrange(name: &str, args: &[String]) -> Rule {
    match args.len() {
        3 => Err(Violation::CountRequired(name.to_owned())),
        5 if args[3].eq_ignore_ascii_case("COUNT") => {
            let n = args[4]
                .parse::<usize>()
                .map_err(|_| Violation::WrongArguments(name.to_owned()))?;
            if n > MAX_RESULT_ROWS {
                return Err(Violation::CountTooLarge {
                    command: name.to_owned(),
                    max: MAX_RESULT_ROWS,
                });
            }
            Ok(plain(Shape::Entries))
        }
        5 => Err(unknown(name, &args[3])),
        _ => Err(Violation::WrongArguments(name.to_owned())),
    }
}

/// `XINFO STREAM key` only: `FULL` reads every entry, and the other
/// subcommands list consumers rather than data.
fn xinfo(args: &[String]) -> Rule {
    if args.len() == 2 && args[0].eq_ignore_ascii_case("STREAM") {
        Ok(plain(Shape::FieldValues))
    } else if args.len() > 2 && args[0].eq_ignore_ascii_case("STREAM") {
        Err(unknown("XINFO", &args[2]))
    } else {
        Err(Violation::NotAllowed("XINFO".into()))
    }
}

fn unknown(command: &str, option: &str) -> Violation {
    Violation::UnknownOption {
        command: command.to_owned(),
        option: printable(option),
    }
}

/// An option or command name as it may appear in a message: short, and
/// nothing but the characters a real one is made of, so a key someone pasted
/// in the wrong place is not echoed back.
fn printable(word: &str) -> String {
    let clean: String = word
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
        .take(32)
        .collect::<String>()
        .to_ascii_uppercase();
    if clean.is_empty() {
        "that command".into()
    } else {
        clean
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(line: &str) -> ReadPlan {
        classify(line).unwrap_or_else(|e| panic!("{line}: {e}"))
    }

    fn refused(line: &str) -> Violation {
        classify(line).expect_err(line)
    }

    #[test]
    fn the_name_is_case_insensitive_and_sent_upper_cased() {
        let plan = ok("get user:1");
        assert_eq!(plan.words, ["GET", "user:1"]);
        assert_eq!(plan.shape, Shape::Value);
        assert!(plan.guard.is_none());
    }

    #[test]
    fn arguments_are_sent_exactly_as_typed() {
        assert_eq!(
            ok(r#"HGET "User:1" "Display Name""#).words,
            ["HGET", "User:1", "Display Name"]
        );
    }

    #[test]
    fn every_write_and_admin_command_is_refused() {
        for line in [
            "SET a 1",
            "DEL a",
            "UNLINK a",
            "EXPIRE a 10",
            "HSET h f v",
            "LPUSH l v",
            "SADD s m",
            "ZADD z 1 m",
            "XADD s * f v",
            "FLUSHALL",
            "FLUSHDB",
            "CONFIG SET maxmemory 1",
            "EVAL \"return 1\" 0",
            "FUNCTION LIST",
            "SCRIPT FLUSH",
            "MONITOR",
            "SUBSCRIBE c",
            "BLPOP l 0",
            "XREAD BLOCK 0 STREAMS s $",
            "SELECT 1",
            "MULTI",
            "CLIENT KILL ID 1",
            "SHUTDOWN",
            "DEBUG SLEEP 1",
            "MIGRATE h 1 k 0 1",
            "RENAME a b",
            "GETDEL a",
            "GETEX a EX 1",
            "SORT l",
            "PFCOUNT h",
        ] {
            assert!(
                matches!(refused(line), Violation::NotAllowed(_)),
                "{line} must not be sent"
            );
        }
    }

    #[test]
    fn keys_is_refused_with_scan_named_instead() {
        let e = refused("KEYS *");
        assert_eq!(e, Violation::Keys);
        assert!(e.to_string().contains("SCAN"));
    }

    #[test]
    fn an_unknown_command_is_refused_not_passed() {
        assert!(matches!(refused("FROBNICATE x"), Violation::NotAllowed(n) if n == "FROBNICATE"));
    }

    #[test]
    fn a_refusal_never_repeats_the_key() {
        let e = refused("SET customer:alice@example.com 1").to_string();
        assert!(!e.contains("alice"), "{e}");
        let e = refused("GET a b c").to_string();
        assert!(!e.contains(" a "), "{e}");
        let e = refused("ZRANGE k 0 1 customer:alice").to_string();
        assert!(!e.contains("alice@"), "{e}");
    }

    #[test]
    fn empty_and_unsplittable_lines_are_refused() {
        assert_eq!(refused("   "), Violation::Empty);
        assert!(matches!(refused("GET \"a"), Violation::Split(_)));
    }

    #[test]
    fn the_number_of_arguments_is_checked() {
        for line in ["GET", "GET a b", "HGET h", "DBSIZE x", "TTL", "LRANGE l 0"] {
            assert!(
                matches!(refused(line), Violation::WrongArguments(_)),
                "{line}"
            );
        }
    }

    #[test]
    fn whole_collection_reads_check_the_size_first() {
        for (line, len) in [
            ("HGETALL h", "HLEN"),
            ("HKEYS h", "HLEN"),
            ("HVALS h", "HLEN"),
            ("SMEMBERS s", "SCARD"),
            ("LRANGE l 0 -1", "LLEN"),
            ("ZRANGE z 0 -1", "ZCARD"),
        ] {
            let plan = ok(line);
            assert_eq!(
                plan.guard,
                Some(guard(len, line.split(' ').nth(1).unwrap())),
                "{line}"
            );
        }
    }

    #[test]
    fn a_plainly_small_range_needs_no_size_check() {
        assert!(ok("LRANGE l 0 99").guard.is_none());
        assert!(ok("ZRANGE z 10 19 WITHSCORES").guard.is_none());
        // Counting from the end could be anything.
        assert!(ok("LRANGE l -10 -1").guard.is_some());
    }

    #[test]
    fn zrange_takes_rev_and_withscores_only() {
        assert_eq!(ok("ZRANGE z 0 9 REV WITHSCORES").shape, Shape::MemberScores);
        assert_eq!(ok("zrange z 0 9").shape, Shape::Values);
        for line in [
            "ZRANGE z 0 9 BYSCORE",
            "ZRANGE z 0 9 LIMIT 0 1",
            "ZRANGE z 0 9 WITHSCORES WITHSCORES",
        ] {
            assert!(
                matches!(refused(line), Violation::UnknownOption { .. }),
                "{line}"
            );
        }
    }

    #[test]
    fn scan_takes_match_count_and_type() {
        assert_eq!(
            ok("SCAN 0 MATCH user:* COUNT 100 TYPE hash").shape,
            Shape::ScanKeys
        );
        assert_eq!(ok("HSCAN h 0").shape, Shape::ScanFieldValues);
        assert_eq!(ok("HSCAN h 0 NOVALUES").shape, Shape::ScanMembers);
        assert_eq!(ok("SSCAN s 0 MATCH a*").shape, Shape::ScanMembers);
        assert_eq!(ok("ZSCAN z 0 COUNT 10").shape, Shape::ScanMemberScores);
    }

    #[test]
    fn scan_refuses_what_it_does_not_know() {
        assert!(matches!(refused("SCAN abc"), Violation::WrongArguments(_)));
        assert!(matches!(
            refused("SCAN 0 MATCH"),
            Violation::WrongArguments(_)
        ));
        assert!(matches!(
            refused("SCAN 0 COUNT x"),
            Violation::WrongArguments(_)
        ));
        assert!(matches!(
            refused("SSCAN s 0 TYPE set"),
            Violation::UnknownOption { .. }
        ));
        assert!(matches!(
            refused("SCAN 0 NOVALUES"),
            Violation::UnknownOption { .. }
        ));
        assert_eq!(
            refused("SCAN 0 COUNT 5000"),
            Violation::CountTooLarge {
                command: "SCAN".into(),
                max: MAX_SCAN_COUNT
            }
        );
    }

    #[test]
    fn a_stream_range_must_be_counted() {
        assert_eq!(ok("XRANGE s - + COUNT 10").shape, Shape::Entries);
        assert_eq!(
            refused("XRANGE s - +"),
            Violation::CountRequired("XRANGE".into())
        );
        assert!(matches!(
            refused("XREVRANGE s + - COUNT 999999"),
            Violation::CountTooLarge { .. }
        ));
        assert!(matches!(
            refused("XRANGE s - + LIMIT 10"),
            Violation::UnknownOption { .. }
        ));
    }

    #[test]
    fn only_the_reading_subcommands_are_allowed() {
        assert_eq!(ok("OBJECT ENCODING k").shape, Shape::Value);
        assert_eq!(ok("MEMORY USAGE k SAMPLES 5").shape, Shape::Value);
        assert_eq!(ok("XINFO STREAM s").shape, Shape::FieldValues);
        assert!(matches!(
            refused("OBJECT HELP x"),
            Violation::UnknownOption { .. }
        ));
        assert!(matches!(refused("MEMORY PURGE"), Violation::NotAllowed(_)));
        assert!(matches!(
            refused("XINFO STREAM s FULL"),
            Violation::UnknownOption { .. }
        ));
        assert!(matches!(
            refused("XINFO GROUPS s"),
            Violation::NotAllowed(_)
        ));
    }

    #[test]
    fn info_takes_section_names_only() {
        assert_eq!(ok("INFO keyspace server").shape, Shape::Info);
        assert!(matches!(
            refused("INFO a;b"),
            Violation::UnknownOption { .. }
        ));
    }

    #[test]
    fn multi_key_reads_are_bounded() {
        let many = format!("MGET {}", vec!["k"; MAX_RESULT_ROWS + 1].join(" "));
        assert!(matches!(refused(&many), Violation::WrongArguments(_)));
        assert_eq!(ok("MGET a b").shape, Shape::Values);
    }
}
