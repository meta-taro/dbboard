//! Splitting a command line into words, the way `redis-cli` does.
//!
//! `HGET "user:1" "display name"` is three words; a backslash escapes the
//! next character inside double quotes, and single quotes take everything
//! literally. Nothing here decides whether a command may run — that is
//! [`crate::read_only`]. This only turns the text a person typed into the
//! words the server would receive, so the classifier judges exactly what
//! would be sent.

use std::fmt;

/// Why a command line could not be split into words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SplitError {
    /// A quote was opened and never closed.
    UnterminatedQuote,
    /// A closing quote was followed directly by more text, as in `"a"b`.
    /// `redis-cli` refuses this too; accepting it would mean guessing.
    TextAfterQuote,
}

impl fmt::Display for SplitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnterminatedQuote => f.write_str("a quote is opened and never closed"),
            Self::TextAfterQuote => {
                f.write_str("a closing quote must be followed by a space or the end of the line")
            }
        }
    }
}

/// Split `line` into words.
///
/// # Errors
///
/// [`SplitError`] when a quote is left open or glued to the next word.
pub fn split(line: &str) -> Result<Vec<String>, SplitError> {
    let mut words = Vec::new();
    let mut chars = line.chars().peekable();
    loop {
        while chars.next_if(|c| c.is_whitespace()).is_some() {}
        let Some(&first) = chars.peek() else {
            return Ok(words);
        };
        let word = match first {
            '"' => {
                chars.next();
                double_quoted(&mut chars)?
            }
            '\'' => {
                chars.next();
                single_quoted(&mut chars)?
            }
            _ => bare(&mut chars),
        };
        if matches!(first, '"' | '\'') && chars.peek().is_some_and(|c| !c.is_whitespace()) {
            return Err(SplitError::TextAfterQuote);
        }
        words.push(word);
    }
}

type Chars<'a> = std::iter::Peekable<std::str::Chars<'a>>;

fn bare(chars: &mut Chars<'_>) -> String {
    let mut word = String::new();
    while let Some(c) = chars.next_if(|c| !c.is_whitespace()) {
        word.push(c);
    }
    word
}

fn double_quoted(chars: &mut Chars<'_>) -> Result<String, SplitError> {
    let mut word = String::new();
    loop {
        match chars.next() {
            None => return Err(SplitError::UnterminatedQuote),
            Some('"') => return Ok(word),
            Some('\\') => match chars.next() {
                None => return Err(SplitError::UnterminatedQuote),
                Some('n') => word.push('\n'),
                Some('r') => word.push('\r'),
                Some('t') => word.push('\t'),
                Some(other) => word.push(other),
            },
            Some(c) => word.push(c),
        }
    }
}

fn single_quoted(chars: &mut Chars<'_>) -> Result<String, SplitError> {
    let mut word = String::new();
    loop {
        match chars.next() {
            None => return Err(SplitError::UnterminatedQuote),
            Some('\'') => return Ok(word),
            Some(c) => word.push(c),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(line: &str) -> Vec<String> {
        split(line).expect("splits")
    }

    #[test]
    fn bare_words_split_on_any_whitespace() {
        assert_eq!(words("  GET   user:1\t"), ["GET", "user:1"]);
    }

    #[test]
    fn an_empty_line_has_no_words() {
        assert!(words("   ").is_empty());
    }

    #[test]
    fn double_quotes_keep_spaces_and_honour_escapes() {
        assert_eq!(
            words(r#"HGET "user:1" "display \"name\"\n""#),
            ["HGET", "user:1", "display \"name\"\n"]
        );
    }

    #[test]
    fn single_quotes_take_everything_literally() {
        assert_eq!(words(r"GET 'a\nb c'"), ["GET", r"a\nb c"]);
    }

    #[test]
    fn an_empty_quoted_word_is_still_a_word() {
        assert_eq!(words(r#"GET """#), ["GET", ""]);
    }

    #[test]
    fn an_open_quote_is_refused() {
        assert_eq!(split(r#"GET "user"#), Err(SplitError::UnterminatedQuote));
        assert_eq!(split("GET 'user"), Err(SplitError::UnterminatedQuote));
        assert_eq!(split(r#"GET "user\"#), Err(SplitError::UnterminatedQuote));
    }

    #[test]
    fn a_quote_glued_to_the_next_word_is_refused() {
        assert_eq!(split(r#"GET "a"b"#), Err(SplitError::TextAfterQuote));
    }

    #[test]
    fn a_quote_inside_a_bare_word_is_part_of_it() {
        // `redis-cli` reads `a"b` as one word; only a word that *starts* with a
        // quote is quoted.
        assert_eq!(words(r#"GET a"b"#), ["GET", r#"a"b"#]);
    }
}
