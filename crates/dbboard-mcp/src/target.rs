//! Which database inside a connection a call is about (ADR-0162).
//!
//! A Postgres connection is bound to one database, so browsing another one on
//! the same server needs a second adapter with the same credentials. Every
//! table-level call therefore names a connection *and*, optionally, a
//! database. `None` means what it always meant: the database the connection
//! was saved with.
//!
//! A `&str` converts into a `Target` with no database, which is why none of the
//! callers that predate this had to change.

/// A connection, and optionally a database within it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Target<'a> {
    pub connection_id: &'a str,
    /// `None`, or a blank name, means the connection's own database.
    pub database: Option<&'a str>,
}

impl<'a> Target<'a> {
    #[must_use]
    pub fn new(connection_id: &'a str, database: Option<&'a str>) -> Self {
        Self {
            connection_id,
            database,
        }
    }

    /// The database to switch to, or `None` for the connection's own. A blank
    /// name is treated as no name: the frontend sends `""` for "not chosen".
    #[must_use]
    pub fn database(&self) -> Option<&'a str> {
        self.database.filter(|d| !d.trim().is_empty())
    }

    /// The adapter-cache key. A plain connection keeps its id as the key, so
    /// existing entries and `invalidate(id)` behave as before. A database
    /// target appends a NUL and the name: NUL cannot occur in a TOML-stored
    /// id, so the two kinds of key never collide. The key is internal and
    /// never leaves the process.
    #[must_use]
    pub fn cache_key(&self) -> String {
        match self.database() {
            None => self.connection_id.to_string(),
            Some(db) => format!("{}\0{db}", self.connection_id),
        }
    }

    /// Whether `key` belongs to `connection_id`, with or without a database.
    /// Used to evict every pool of a connection when it is edited or reset.
    #[must_use]
    pub fn key_belongs_to(key: &str, connection_id: &str) -> bool {
        key == connection_id
            || key
                .strip_prefix(connection_id)
                .is_some_and(|rest| rest.starts_with('\0'))
    }
}

impl<'a> From<&'a str> for Target<'a> {
    fn from(connection_id: &'a str) -> Self {
        Self::new(connection_id, None)
    }
}

impl<'a> From<&'a String> for Target<'a> {
    fn from(connection_id: &'a String) -> Self {
        Self::new(connection_id, None)
    }
}

#[cfg(test)]
mod tests {
    use super::Target;

    #[test]
    fn a_bare_connection_keeps_its_id_as_the_cache_key() {
        assert_eq!(Target::from("pg").cache_key(), "pg");
        assert_eq!(Target::new("pg", None).cache_key(), "pg");
    }

    #[test]
    fn a_database_gets_its_own_cache_entry() {
        assert_eq!(Target::new("pg", Some("crm")).cache_key(), "pg\0crm");
    }

    #[test]
    fn a_blank_database_means_the_connections_own() {
        assert_eq!(Target::new("pg", Some("  ")).database(), None);
        assert_eq!(Target::new("pg", Some("")).cache_key(), "pg");
    }

    /// Evicting `pg` must take its database pools with it, and must not touch
    /// a different connection whose id merely starts with the same letters.
    #[test]
    fn eviction_matches_the_connection_and_its_databases_only() {
        assert!(Target::key_belongs_to("pg", "pg"));
        assert!(Target::key_belongs_to("pg\0crm", "pg"));
        assert!(!Target::key_belongs_to("pg-staging", "pg"));
        assert!(!Target::key_belongs_to("pg-staging\0crm", "pg"));
    }
}
