//! Domain-level errors that every adapter must map onto.
//!
//! Adapters convert driver-specific failures (libsql, sqlx, ...) into
//! one of these variants so the UI can render a stable taxonomy of
//! error categories regardless of which database is connected.

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("connection failed: {0}")]
    Connection(String),

    #[error("query failed: {0}")]
    Query(String),

    #[error("schema introspection failed: {0}")]
    Schema(String),

    #[error("type conversion failed: {0}")]
    TypeConversion(String),
}

pub type DbResult<T> = Result<T, DbError>;

#[cfg(test)]
mod tests {
    use super::DbError;

    #[test]
    fn connection_error_renders_category_and_message() {
        let e = DbError::Connection("host unreachable".into());
        assert_eq!(e.to_string(), "connection failed: host unreachable");
    }

    #[test]
    fn query_error_renders_category_and_message() {
        let e = DbError::Query("syntax near SELEC".into());
        assert_eq!(e.to_string(), "query failed: syntax near SELEC");
    }

    #[test]
    fn schema_error_renders_category_and_message() {
        let e = DbError::Schema("sqlite_master unreadable".into());
        assert_eq!(
            e.to_string(),
            "schema introspection failed: sqlite_master unreadable"
        );
    }

    #[test]
    fn type_conversion_error_renders_category_and_message() {
        let e = DbError::TypeConversion("BLOB into i64".into());
        assert_eq!(e.to_string(), "type conversion failed: BLOB into i64");
    }
}
