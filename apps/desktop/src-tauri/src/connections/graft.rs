//! Keeping the stored password when the edit form cannot send it back
//! (ADR-0080).
//!
//! Its own file because it is the one place in the write path that reads a
//! secret rather than writing one, and the rule for when it must *not* run is
//! easier to keep straight when it is not surrounded by the DTOs it rewrites.

use super::input::KindEditInput;

/// Rewrite a URL-bearing kind's DSN through `graft` (ADR-0080).
///
/// The edit form composes its DSN from the parts it was shown, and those never
/// included the password — so when the user leaves the password box blank,
/// meaning "keep the stored one", the composed URL is missing a credential the
/// connection needs. `graft` puts it back, inside the process that already
/// holds it.
///
/// Kinds with no DSN, and a blank URL (the URL-mode "keep the whole secret"
/// signal), pass through untouched. MongoDB is among them despite carrying a
/// URI: its edit form shows the URI whole rather than in parts, so there is no
/// password to put back — grafting one would rewrite what the user just typed.
pub(crate) fn graft_url<F>(kind: KindEditInput, graft: F) -> Result<KindEditInput, String>
where
    F: Fn(&str) -> Result<String, String>,
{
    let apply = |url: Option<String>| -> Result<Option<String>, String> {
        match url {
            Some(u) if !u.trim().is_empty() => graft(&u).map(Some),
            other => Ok(other),
        }
    };
    Ok(match kind {
        KindEditInput::Postgres { url } => KindEditInput::Postgres { url: apply(url)? },
        KindEditInput::MySql { url } => KindEditInput::MySql { url: apply(url)? },
        KindEditInput::Neon { url } => KindEditInput::Neon { url: apply(url)? },
        KindEditInput::Supabase { url } => KindEditInput::Supabase { url: apply(url)? },
        KindEditInput::AuroraDsql { url } => KindEditInput::AuroraDsql { url: apply(url)? },
        other => other,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn graft_url_rewrites_every_url_bearing_kind() {
        let graft = |url: &str| Ok(format!("{url}#grafted"));
        let cases = vec![
            KindEditInput::Postgres {
                url: Some("postgres://app@db:5432/x".to_string()),
            },
            KindEditInput::MySql {
                url: Some("mysql://app@db:3306/x".to_string()),
            },
            KindEditInput::Neon {
                url: Some("postgres://app@db:5432/x".to_string()),
            },
            KindEditInput::Supabase {
                url: Some("postgres://app@db:5432/x".to_string()),
            },
            KindEditInput::AuroraDsql {
                url: Some("postgres://app@db:5432/x".to_string()),
            },
        ];
        for kind in cases {
            let out = graft_url(kind, graft).expect("graft");
            let url = match out {
                KindEditInput::Postgres { url }
                | KindEditInput::MySql { url }
                | KindEditInput::Neon { url }
                | KindEditInput::Supabase { url }
                | KindEditInput::AuroraDsql { url } => url,
                _ => panic!("kind changed under graft_url"),
            };
            assert!(url.expect("url").ends_with("#grafted"));
        }
    }

    // A blank URL is URL-mode's own "keep the whole stored secret" signal;
    // grafting a password into nothing would compose a bogus DSN.
    #[test]
    fn graft_url_leaves_a_blank_url_alone() {
        let boom = |_: &str| -> Result<String, String> { panic!("must not graft a blank url") };
        for url in [None, Some(String::new()), Some("  ".to_string())] {
            let out = graft_url(KindEditInput::MySql { url }, boom).expect("graft");
            match out {
                KindEditInput::MySql { url } => assert!(url.unwrap_or_default().trim().is_empty()),
                _ => panic!("kind changed"),
            }
        }
    }

    #[test]
    fn graft_url_ignores_kinds_that_store_no_dsn() {
        let boom = |_: &str| -> Result<String, String> { panic!("must not graft a non-dsn kind") };
        let out = graft_url(
            KindEditInput::Turso {
                path: "./a.db".to_string(),
            },
            boom,
        )
        .expect("graft");
        assert!(matches!(out, KindEditInput::Turso { .. }));
    }

    // The stored DSN being unreadable must surface, not be swallowed into a
    // save that drops the credential.
    #[test]
    fn graft_url_propagates_a_failure() {
        let fail = |_: &str| -> Result<String, String> { Err("keychain gone".to_string()) };
        // Matched rather than `expect_err`, which would need `Debug` on
        // `KindEditInput` — a type that carries a DSN, password and all.
        match graft_url(
            KindEditInput::MySql {
                url: Some("mysql://app@db:3306/x".to_string()),
            },
            fail,
        ) {
            Err(err) => assert_eq!(err, "keychain gone"),
            Ok(_) => panic!("a failed graft must not save"),
        }
    }

    #[test]
    fn graft_url_leaves_a_mongodb_uri_alone() {
        // `graft` exists for the DSN-parts form, which is pg-wire shaped. A
        // Mongo URI is edited whole, so grafting a stored password into it
        // would rewrite a URI the user just typed in full.
        let out = graft_url(
            KindEditInput::MongoDb {
                uri: Some("mongodb://app@127.0.0.1:27117".to_string()),
                database: None,
            },
            |_| panic!("graft must not be called for MongoDB"),
        )
        .expect("graft_url");
        assert!(matches!(
            out,
            KindEditInput::MongoDb { uri: Some(u), .. } if u == "mongodb://app@127.0.0.1:27117"
        ));
    }
}
