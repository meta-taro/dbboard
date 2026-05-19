//! Adapter-neutral representation of a single database cell.
//!
//! Each adapter (Turso, Neon, Supabase, ...) translates its native
//! driver value into this enum so the UI layer never sees adapter
//! types. Variants intentionally mirror SQLite storage classes plus
//! the bare minimum needed for PostgreSQL adapters to widen later.

use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Integer(i64),
    Real(f64),
    Text(String),
    Blob(Vec<u8>),
}

impl Value {
    #[must_use]
    pub fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Null => write!(f, "NULL"),
            Self::Integer(n) => write!(f, "{n}"),
            Self::Real(x) => write!(f, "{x}"),
            Self::Text(s) => write!(f, "{s}"),
            Self::Blob(b) => write!(f, "<blob: {} bytes>", b.len()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Value;

    #[test]
    fn null_is_null_other_variants_are_not() {
        assert!(Value::Null.is_null());
        assert!(!Value::Integer(0).is_null());
        assert!(!Value::Real(0.0).is_null());
        assert!(!Value::Text(String::new()).is_null());
        assert!(!Value::Blob(Vec::new()).is_null());
    }

    #[test]
    fn display_renders_null_as_keyword() {
        assert_eq!(Value::Null.to_string(), "NULL");
    }

    #[test]
    fn display_renders_text_without_quotes() {
        assert_eq!(Value::Text("hello".into()).to_string(), "hello");
    }

    #[test]
    fn display_renders_integer_in_decimal() {
        assert_eq!(Value::Integer(42).to_string(), "42");
        assert_eq!(Value::Integer(-7).to_string(), "-7");
    }

    #[test]
    fn display_renders_blob_as_byte_count_summary() {
        assert_eq!(Value::Blob(vec![0; 12]).to_string(), "<blob: 12 bytes>");
    }
}
