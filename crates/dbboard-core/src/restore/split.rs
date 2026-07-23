//! Split a `.sql` script into individual statements (ADR-0051, slice 1).
//!
//! This is the *raw* splitter — Layer 1 of the two-layer restore pipeline.
//! It scans the script's lexical structure (string literals, quoted
//! identifiers, dollar-quoted bodies, and comments) so a `;` that lives
//! inside any of those does not split a statement. It classifies nothing:
//! whether a resulting statement is a `CREATE`, an `INSERT`, or garbage is
//! Layer 2's job (the sqlparser-based classifier in a later slice).
//!
//! Being lexical rather than grammatical, it is robust to *any* `.sql` we
//! might be handed — dbboard's own dumps, `pg_dump` output (dollar-quoted
//! function bodies, `E'…'` escape strings), and `sqlite3 .dump` output
//! (backtick identifiers) all split correctly. It never parses, so it
//! cannot reject or rewrite; it only finds statement boundaries.
//!
//! The one lexical subtlety worth stating: a backslash is an escape
//! character *only* inside a Postgres `E'…'` escape string. In a standard
//! string literal (`standard_conforming_strings`, the `pg_dump` default
//! since PostgreSQL 9.1, and SQLite always) a backslash is an ordinary
//! character and the only in-string quote escape is a doubled `''`.
//! Honouring backslash everywhere would mis-scan `'a\'` — a complete
//! two-character string — as an unterminated one.

/// Split `sql` into its constituent statements, dropping the `;`
/// delimiters and any segment that is only whitespace and comments.
///
/// Each returned string is one statement's source text, trimmed of
/// surrounding whitespace but otherwise verbatim (interior comments are
/// preserved — every supported engine tolerates them). A trailing
/// statement with no terminating `;` is still returned. Unterminated
/// strings, identifiers, and dollar-quotes consume to end-of-input rather
/// than erroring — this layer reports no diagnostics.
#[must_use]
pub fn split_statements(sql: &str) -> Vec<String> {
    let chars: Vec<char> = sql.chars().collect();
    let n = chars.len();
    let mut statements = Vec::new();
    let mut start = 0;
    let mut has_content = false;
    let mut i = 0;

    while i < n {
        let c = chars[i];
        match c {
            // Line comment `-- … ⏎`. A lone `-` (subtraction, negative
            // literal) falls through to the content arm.
            '-' if i + 1 < n && chars[i + 1] == '-' => {
                i += 2;
                while i < n && chars[i] != '\n' {
                    i += 1;
                }
            }
            // Block comment `/* … */`, nesting-aware for Postgres.
            '/' if i + 1 < n && chars[i + 1] == '*' => {
                i = skip_block_comment(&chars, i);
            }
            '\'' => {
                i = skip_single_quoted(&chars, i, is_escape_string_opener(&chars, i));
                has_content = true;
            }
            '"' => {
                i = skip_delimited(&chars, i, '"');
                has_content = true;
            }
            '`' => {
                i = skip_delimited(&chars, i, '`');
                has_content = true;
            }
            '$' => {
                has_content = true;
                i = match try_open_dollar_quote(&chars, i) {
                    Some((body_start, delim)) => find_dollar_close(&chars, body_start, &delim),
                    None => i + 1,
                };
            }
            ';' => {
                if has_content {
                    statements.push(collect_trimmed(&chars, start, i));
                }
                i += 1;
                start = i;
                has_content = false;
            }
            other => {
                if !other.is_whitespace() {
                    has_content = true;
                }
                i += 1;
            }
        }
    }

    if has_content {
        statements.push(collect_trimmed(&chars, start, n));
    }
    statements
}

/// Is the `'` at `quote` the opening quote of a Postgres `E'…'` escape
/// string? True when the immediately preceding character is a word-boundary
/// `E`/`e` — i.e. an `E` that is not the tail of a longer identifier.
fn is_escape_string_opener(chars: &[char], quote: usize) -> bool {
    if quote == 0 {
        return false;
    }
    let prev = chars[quote - 1];
    if prev != 'e' && prev != 'E' {
        return false;
    }
    // The `E` must stand alone, else it is the last letter of an identifier
    // (`the'…'` is not valid SQL, but `type'…'` must not be read as `e'…'`).
    quote < 2 || !is_ident_char(chars[quote - 2])
}

fn is_ident_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Advance past a single-quoted string. `escapes` enables backslash escapes
/// (Postgres `E'…'` only); the doubled-quote escape `''` always applies.
/// Starts at the opening quote, returns the index just past the closing one.
fn skip_single_quoted(chars: &[char], open: usize, escapes: bool) -> usize {
    let n = chars.len();
    let mut i = open + 1;
    while i < n {
        let c = chars[i];
        if escapes && c == '\\' && i + 1 < n {
            i += 2;
            continue;
        }
        if c == '\'' {
            if i + 1 < n && chars[i + 1] == '\'' {
                i += 2;
                continue;
            }
            return i + 1;
        }
        i += 1;
    }
    n
}

/// Advance past a `delim`-quoted identifier (`"…"` or `` `…` ``) whose only
/// escape is the doubled delimiter. Starts at the opener, returns just past
/// the closer.
fn skip_delimited(chars: &[char], open: usize, delim: char) -> usize {
    let n = chars.len();
    let mut i = open + 1;
    while i < n {
        if chars[i] == delim {
            if i + 1 < n && chars[i + 1] == delim {
                i += 2;
                continue;
            }
            return i + 1;
        }
        i += 1;
    }
    n
}

/// Advance past a `/* … */` block comment, honouring Postgres nesting.
/// Starts at the `/`, returns just past the outermost `*/`.
fn skip_block_comment(chars: &[char], open: usize) -> usize {
    let n = chars.len();
    let mut depth = 1;
    let mut i = open + 2;
    while i < n {
        if i + 1 < n && chars[i] == '/' && chars[i + 1] == '*' {
            depth += 1;
            i += 2;
        } else if i + 1 < n && chars[i] == '*' && chars[i + 1] == '/' {
            depth -= 1;
            i += 2;
            if depth == 0 {
                return i;
            }
        } else {
            i += 1;
        }
    }
    n
}

/// If a dollar-quote opens at `dollar` (`$tag$` with an identifier-shaped or
/// empty tag), return the body-start index and the full `$tag$` delimiter.
/// A bare `$1` parameter placeholder — `$` not closed by a second `$` after
/// a valid tag — returns `None`.
fn try_open_dollar_quote(chars: &[char], dollar: usize) -> Option<(usize, String)> {
    let n = chars.len();
    let mut j = dollar + 1;
    // A non-empty tag must be identifier-shaped: it cannot start with a
    // digit (that would be a `$1` placeholder, not a dollar-quote tag).
    if j < n && (chars[j].is_alphabetic() || chars[j] == '_') {
        j += 1;
        while j < n && is_ident_char(chars[j]) {
            j += 1;
        }
    }
    if j < n && chars[j] == '$' {
        let delim: String = chars[dollar..=j].iter().collect();
        Some((j + 1, delim))
    } else {
        None
    }
}

/// Find the closing dollar-quote `delim` at or after `from`, returning the
/// index just past it (or end-of-input if unterminated).
fn find_dollar_close(chars: &[char], from: usize, delim: &str) -> usize {
    let n = chars.len();
    let d: Vec<char> = delim.chars().collect();
    let mut i = from;
    while i + d.len() <= n {
        if chars[i..i + d.len()] == d[..] {
            return i + d.len();
        }
        i += 1;
    }
    n
}

fn collect_trimmed(chars: &[char], start: usize, end: usize) -> String {
    chars[start..end]
        .iter()
        .collect::<String>()
        .trim()
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::split_statements;

    #[test]
    fn empty_input_yields_no_statements() {
        assert!(split_statements("").is_empty());
        assert!(split_statements("   \n\t ").is_empty());
    }

    #[test]
    fn splits_two_simple_statements() {
        let out = split_statements("SELECT 1; SELECT 2;");
        assert_eq!(out, vec!["SELECT 1", "SELECT 2"]);
    }

    #[test]
    fn a_trailing_statement_without_a_semicolon_is_returned() {
        let out = split_statements("SELECT 1;\nSELECT 2");
        assert_eq!(out, vec!["SELECT 1", "SELECT 2"]);
    }

    #[test]
    fn semicolon_inside_a_string_literal_does_not_split() {
        let out = split_statements("INSERT INTO t VALUES ('a; b'); SELECT 1");
        assert_eq!(out, vec!["INSERT INTO t VALUES ('a; b')", "SELECT 1"]);
    }

    #[test]
    fn a_doubled_quote_escapes_within_a_string() {
        let out = split_statements("INSERT INTO t VALUES ('O''Brien; Jr'); SELECT 1");
        assert_eq!(
            out,
            vec!["INSERT INTO t VALUES ('O''Brien; Jr')", "SELECT 1"]
        );
    }

    #[test]
    fn a_backslash_is_literal_in_a_standard_string() {
        // Standard string: `'a\'` is the complete two-char string `a\`, so
        // the following `;` splits. Honouring backslash would swallow it.
        let out = split_statements(r"SELECT 'a\'; SELECT 2");
        assert_eq!(out, vec![r"SELECT 'a\'", "SELECT 2"]);
    }

    #[test]
    fn a_backslash_escapes_only_in_an_escape_string() {
        // `E'\''` is a single-char string containing a quote; the `;`
        // after it splits. The backslash-escaped quote must not close it.
        let out = split_statements(r"SELECT E'\''; SELECT 2");
        assert_eq!(out, vec![r"SELECT E'\''", "SELECT 2"]);
    }

    #[test]
    fn a_trailing_e_of_an_identifier_does_not_start_an_escape_string() {
        // `type` ends in `e`, but `'…'` after it is a standard string:
        // the interior `\'` closes at the `'`, and the `;` splits.
        let out = split_statements(r"SELECT type '\'; SELECT 2");
        assert_eq!(out, vec![r"SELECT type '\'", "SELECT 2"]);
    }

    #[test]
    fn semicolon_inside_a_quoted_identifier_does_not_split() {
        let out = split_statements(r#"SELECT "a;b" FROM t; SELECT 1"#);
        assert_eq!(out, vec![r#"SELECT "a;b" FROM t"#, "SELECT 1"]);
    }

    #[test]
    fn semicolon_inside_a_backtick_identifier_does_not_split() {
        let out = split_statements("SELECT `a;b` FROM t; SELECT 1");
        assert_eq!(out, vec!["SELECT `a;b` FROM t", "SELECT 1"]);
    }

    #[test]
    fn semicolon_inside_a_dollar_quoted_body_does_not_split() {
        let src = "CREATE FUNCTION f() RETURNS int AS $$ BEGIN; RETURN 1; END; $$ LANGUAGE plpgsql; SELECT 1";
        let out = split_statements(src);
        assert_eq!(out.len(), 2);
        assert!(out[0].contains("BEGIN; RETURN 1; END;"));
        assert_eq!(out[1], "SELECT 1");
    }

    #[test]
    fn a_tagged_dollar_quote_matches_only_its_own_tag() {
        // A `$$` inside the `$body$…$body$` region must not close it.
        let src = "SELECT $body$ a $$ b ; c $body$; SELECT 1";
        let out = split_statements(src);
        assert_eq!(out.len(), 2);
        assert!(out[0].contains("$$ b ; c"));
        assert_eq!(out[1], "SELECT 1");
    }

    #[test]
    fn a_dollar_parameter_placeholder_is_not_a_dollar_quote() {
        // `$1`/`$2` are placeholders; the `;` still splits normally.
        let out = split_statements("SELECT $1 FROM t WHERE id = $2; SELECT 1");
        assert_eq!(out, vec!["SELECT $1 FROM t WHERE id = $2", "SELECT 1"]);
    }

    #[test]
    fn semicolon_inside_a_line_comment_does_not_split() {
        let out = split_statements("SELECT 1 -- a; b\n; SELECT 2");
        assert_eq!(out.len(), 2);
        assert!(out[0].starts_with("SELECT 1"));
        assert_eq!(out[1], "SELECT 2");
    }

    #[test]
    fn semicolon_inside_a_block_comment_does_not_split() {
        let out = split_statements("SELECT 1 /* a; b */; SELECT 2");
        assert_eq!(out.len(), 2);
        assert_eq!(out[1], "SELECT 2");
    }

    #[test]
    fn nested_block_comments_are_balanced() {
        let out = split_statements("SELECT 1 /* outer /* inner; */ still; */; SELECT 2");
        assert_eq!(out.len(), 2);
        assert_eq!(out[1], "SELECT 2");
    }

    #[test]
    fn comment_only_and_blank_segments_are_dropped() {
        let out = split_statements("-- a dump header\n\n; ; SELECT 1; -- trailing\n");
        assert_eq!(out, vec!["SELECT 1"]);
    }

    #[test]
    fn interior_comments_are_preserved_in_the_statement() {
        let out = split_statements("-- note\nSELECT 1");
        assert_eq!(out, vec!["-- note\nSELECT 1"]);
    }

    #[test]
    fn an_unterminated_string_consumes_to_end_without_panicking() {
        let out = split_statements("SELECT 'oops; no close");
        assert_eq!(out, vec!["SELECT 'oops; no close"]);
    }

    #[test]
    fn a_realistic_pg_dump_snippet_splits_into_its_statements() {
        let src = "\
-- dbboard logical dump (postgres)

CREATE TABLE public.users (
    id integer NOT NULL,
    note text
);

INSERT INTO public.users VALUES (1, 'has ; and '' quote');
INSERT INTO public.users VALUES (2, E'tab\\tafter; semicolon');

CREATE FUNCTION public.greet() RETURNS text AS $func$
BEGIN
    RETURN 'hi; there';
END;
$func$ LANGUAGE plpgsql;
";
        let out = split_statements(src);
        assert_eq!(out.len(), 4, "statements: {out:#?}");
        // The dump header comment leads the first statement (no `;` between
        // them), so it rides along — harmless to execute, and asserted here
        // as the documented leading-comment behaviour.
        assert!(out[0].contains("CREATE TABLE public.users"));
        assert!(out[1].contains("has ; and '' quote"));
        assert!(out[2].contains(r"tab\tafter; semicolon"));
        assert!(out[3].contains("$func$") && out[3].contains("RETURN 'hi; there';"));
    }
}
