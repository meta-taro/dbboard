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

/// The database `config` was saved with, decoded, or `None` when it names none
/// or the engine holds a single database (ADR-0162). The UI reads this to keep
/// a connection saved with a database looking exactly as it did, and to offer
/// every database only for one saved without.
#[must_use]
pub fn configured_database(config: &BackendConfig) -> Option<String> {
    let (BackendConfig::Postgres { url, .. }
    | BackendConfig::Neon { url, .. }
    | BackendConfig::Supabase { url, .. }
    | BackendConfig::MySql { url, .. }) = config
    else {
        return None;
    };
    // The same reading the edit form uses, so the two never disagree about
    // what a connection was saved with.
    dbboard_config::parse_dsn(url)
        .map(|parts| parts.database)
        .filter(|db| !db.is_empty())
}

/// Where a Postgres-family connection saved without a database opens.
const POSTGRES_MAINTENANCE_DATABASE: &str = "postgres";

/// `config`, made connectable when it names no database (ADR-0162).
///
/// A Postgres server given no database falls back to one named after the
/// user, which rarely exists. The `postgres` maintenance database does on a
/// self-hosted server, Neon and Supabase, so a connection saved without a
/// database opens there and lists the rest. A saved database is never
/// overridden, and every other engine passes through unchanged.
///
/// # Errors
///
/// Only what [`scoped_to_database`] returns for an unparseable URL.
pub fn with_listing_default(config: BackendConfig) -> DbResult<BackendConfig> {
    let postgres_family = matches!(
        config,
        BackendConfig::Postgres { .. }
            | BackendConfig::Neon { .. }
            | BackendConfig::Supabase { .. }
    );
    if postgres_family && configured_database(&config).is_none() {
        scoped_to_database(config, POSTGRES_MAINTENANCE_DATABASE)
    } else {
        Ok(config)
    }
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
    use super::{configured_database, scoped_to_database, with_listing_default};
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

    /// What the connection was saved with, so the UI can tell "show this one
    /// database, as before" from "show every database it can reach".
    #[test]
    fn the_saved_database_is_read_back_decoded() {
        let named = BackendConfig::Postgres {
            url: "postgres://app@h:5432/my%20db?sslmode=require".into(),
            ssh: None,
        };
        assert_eq!(configured_database(&named).as_deref(), Some("my db"));

        let blank = BackendConfig::MySql {
            url: "mysql://app@h:3306?ssl-mode=disabled".into(),
            ssh: None,
        };
        assert_eq!(configured_database(&blank), None);

        let trailing_slash = BackendConfig::Neon {
            url: "postgres://app@h/".into(),
            ssh: None,
        };
        assert_eq!(configured_database(&trailing_slash), None);
    }

    /// An engine with one database per connection has nothing to choose
    /// between, so there is no "saved database" to report.
    #[test]
    fn a_single_database_engine_reports_none() {
        assert_eq!(configured_database(&BackendConfig::turso(":memory:")), None);
    }

    /// A Postgres server falls back to a database named after the user when
    /// the URL names none, and that one rarely exists. `postgres` does, on a
    /// self-hosted server, Neon and Supabase alike, so a connection saved
    /// without a database opens there to list the rest.
    #[test]
    fn a_blank_postgres_connection_opens_the_maintenance_database() {
        let blank = BackendConfig::Neon {
            url: "postgres://app@ep-x.neon.tech?sslmode=require".into(),
            ssh: None,
        };
        let opened = with_listing_default(blank).expect("defaulted");
        assert_eq!(
            url_of(&opened),
            "postgres://app@ep-x.neon.tech/postgres?sslmode=require"
        );
    }

    /// A saved database is never overridden, and MySQL needs no default: one
    /// MySQL connection sees every database without being "in" any.
    #[test]
    fn a_named_database_and_mysql_are_left_alone() {
        let named = BackendConfig::Postgres {
            url: "postgres://app@h/shop".into(),
            ssh: None,
        };
        assert_eq!(
            url_of(&with_listing_default(named).expect("ok")),
            "postgres://app@h/shop"
        );
        let mysql = BackendConfig::MySql {
            url: "mysql://app@h:3306".into(),
            ssh: None,
        };
        assert_eq!(
            url_of(&with_listing_default(mysql).expect("ok")),
            "mysql://app@h:3306"
        );
    }
}
