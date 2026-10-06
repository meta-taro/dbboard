//! Pointing a resolved connection at another database (ADR-0162).
//!
//! A Postgres connection is bound to one database. Showing a server's other
//! databases means opening each with the same credentials, which is a
//! different database name in the same URL. MySQL takes the same rewrite: there
//! it changes the default database, so unqualified SQL resolves against the one
//! the person picked.
//!
//! Kept apart from `config.rs`, which resolves *which* connection, because this
//! answers a different question: given one, *which database inside it*.

use dbboard_core::{DbError, DbResult};

use crate::BackendConfig;

/// `config` with its database replaced by `database`.
///
/// # Errors
///
/// [`DbError::Capability`] for engines that hold one database per connection
/// (libSQL, D1, Aurora DSQL, Firestore, `MongoDB` for now), and
/// [`DbError::Connection`] when the stored URL cannot be parsed.
pub fn scoped_to_database(config: BackendConfig, database: &str) -> DbResult<BackendConfig> {
    // Blank cannot mean "the default": a caller that asks for a database and
    // gets whichever one the server falls back to would show the wrong tables.
    if database.trim().is_empty() {
        return Err(DbError::Connection("a database name is required".into()));
    }
    Ok(match config {
        BackendConfig::Postgres { url, ssh } => BackendConfig::Postgres {
            url: with_database(&url, database)?,
            ssh,
        },
        BackendConfig::Neon { url, ssh } => BackendConfig::Neon {
            url: with_database(&url, database)?,
            ssh,
        },
        BackendConfig::Supabase { url, ssh } => BackendConfig::Supabase {
            url: with_database(&url, database)?,
            ssh,
        },
        BackendConfig::MySql { url, ssh } => BackendConfig::MySql {
            url: with_database(&url, database)?,
            ssh,
        },
        _ => {
            return Err(DbError::Capability(
                "this connection holds a single database".into(),
            ))
        }
    })
}

/// `url` with its path replaced by `database` as one percent-encoded segment.
/// The error never carries the URL: it embeds the password.
fn with_database(url: &str, database: &str) -> DbResult<String> {
    let mut parsed = url::Url::parse(url)
        .map_err(|_| DbError::Connection("the stored connection URL is not valid".into()))?;
    parsed
        .path_segments_mut()
        .map_err(|()| DbError::Connection("the stored connection URL has no host".into()))?
        .clear()
        .push(database);
    Ok(parsed.into())
}

#[cfg(test)]
mod tests {
    use super::scoped_to_database;
    use crate::BackendConfig;
    use dbboard_core::DbError;

    fn url_of(config: &BackendConfig) -> &str {
        match config {
            BackendConfig::Postgres { url, .. }
            | BackendConfig::Neon { url, .. }
            | BackendConfig::Supabase { url, .. }
            | BackendConfig::MySql { url, .. } => url,
            _ => panic!("not a URL-shaped backend"),
        }
    }

    #[test]
    fn a_postgres_url_gets_the_new_database_and_keeps_everything_else() {
        let config = BackendConfig::Postgres {
            url: "postgres://app:s%40cret@db.internal:5432/shop?sslmode=require".into(),
            ssh: None,
        };
        let scoped = scoped_to_database(config, "crm").expect("scoped");
        assert_eq!(
            url_of(&scoped),
            "postgres://app:s%40cret@db.internal:5432/crm?sslmode=require"
        );
    }

    /// The case this exists for: a connection saved with no database at all.
    #[test]
    fn a_url_with_no_database_gains_one() {
        let config = BackendConfig::Neon {
            url: "postgres://app@ep-x.neon.tech".into(),
            ssh: None,
        };
        let scoped = scoped_to_database(config, "analytics").expect("scoped");
        assert_eq!(url_of(&scoped), "postgres://app@ep-x.neon.tech/analytics");
    }

    /// A database name is an identifier, not a path. `a/b` or a space must
    /// reach the server as the name it is, not as two path segments.
    #[test]
    fn the_name_is_percent_encoded() {
        let config = BackendConfig::Supabase {
            url: "postgres://app@h:5432/postgres".into(),
            ssh: None,
        };
        let scoped = scoped_to_database(config, "my db/2").expect("scoped");
        assert_eq!(url_of(&scoped), "postgres://app@h:5432/my%20db%2F2");
    }

    #[test]
    fn mysql_changes_its_default_database() {
        let config = BackendConfig::MySql {
            url: "mysql://app@h:3306?ssl-mode=disabled".into(),
            ssh: None,
        };
        let scoped = scoped_to_database(config, "shop").expect("scoped");
        assert_eq!(url_of(&scoped), "mysql://app@h:3306/shop?ssl-mode=disabled");
    }

    #[test]
    fn a_single_database_engine_refuses() {
        let err = scoped_to_database(BackendConfig::turso(":memory:"), "other")
            .expect_err("libSQL holds one database");
        assert!(matches!(err, DbError::Capability(_)), "{err:?}");
    }

    #[test]
    fn an_empty_name_is_refused_rather_than_meaning_the_default() {
        let config = BackendConfig::Postgres {
            url: "postgres://app@h/shop".into(),
            ssh: None,
        };
        let err = scoped_to_database(config, "  ").expect_err("blank");
        assert!(matches!(err, DbError::Connection(_)), "{err:?}");
    }
}
