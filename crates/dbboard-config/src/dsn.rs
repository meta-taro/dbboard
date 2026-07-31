//! Splitting a stored connection URL into the parts an edit form shows, and
//! putting a password back into one.
//!
//! A URL-bearing connection (Postgres, `MySQL`, Neon, Supabase, Aurora DSQL)
//! stores its whole DSN — password included — as a single keyring secret. The
//! add form asks for host/port/user/password/database separately (ADR-0073),
//! so the edit form has to as well, or the same connection is two different
//! products depending on which button opened it.
//!
//! That means reading the stored URL back apart. [`DsnParts`] is deliberately
//! the *non-secret* projection: it has no password field at all, so a prefill
//! payload cannot leak one by omission-turned-oversight. The password is
//! reachable only through [`with_password`], which never returns it on its own
//! and is used to re-attach the stored password to a URL the user rebuilt
//! without typing it.

use url::Url;

/// The non-secret half of a DSN, as the edit form displays it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DsnParts {
    pub host: String,
    /// `None` when the URL omits it, meaning the engine default.
    pub port: Option<u16>,
    pub user: String,
    pub database: String,
    /// The query string minus its leading `?`, empty when there is none.
    /// Carries `ssl-mode`/`sslmode` and anything else the user appended, so a
    /// round-trip through the form does not silently drop it.
    pub query: String,
}

/// Split `url` into its non-secret parts, or `None` if it does not parse.
///
/// Percent-encoded octets are decoded, because the form shows the value the
/// user typed rather than the wire form: a password-free `user` of `a%20b` is
/// displayed as `a b` and re-encoded on the way back out.
#[must_use]
pub fn parse_dsn(url: &str) -> Option<DsnParts> {
    let parsed = Url::parse(url.trim()).ok()?;
    let host = parsed.host_str()?.to_string();
    // `Url` keeps IPv6 hosts bracketed; the form wants the bare address so it
    // round-trips through an input box the same way it was typed.
    let host = host
        .strip_prefix('[')
        .and_then(|h| h.strip_suffix(']'))
        .map_or(host.clone(), ToString::to_string);
    Some(DsnParts {
        host,
        port: parsed.port(),
        user: decode(parsed.username()),
        database: decode(parsed.path().trim_start_matches('/')),
        query: parsed.query().unwrap_or_default().to_string(),
    })
}

/// `target` with `source`'s password grafted on.
///
/// This is the "leave the password blank to keep it" path: the form rebuilds
/// the URL from the parts the user can see, and the stored password — which
/// was never sent to the UI — is re-attached here. Returns `None` if either
/// URL fails to parse, and `target` unchanged when `source` has no password.
///
/// An unparseable `source` fails rather than falling through to "no password":
/// that would save a working connection back without its credential and break
/// it silently, which is the one outcome worse than refusing the edit.
#[must_use]
pub fn with_password(target: &str, source: &str) -> Option<String> {
    let mut parsed = Url::parse(target.trim()).ok()?;
    let stored = Url::parse(source.trim()).ok()?;
    let Some(password) = stored.password() else {
        return Some(target.trim().to_string());
    };
    parsed.set_password(Some(&decode(password))).ok()?;
    Some(parsed.to_string())
}

/// Percent-decode a URL component, falling back to the raw text when it is not
/// valid UTF-8 after decoding — a value we cannot decode is still better shown
/// as typed than dropped.
fn decode(raw: &str) -> String {
    percent_decode(raw).unwrap_or_else(|| raw.to_string())
}

fn percent_decode(raw: &str) -> Option<String> {
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok()?;
            let byte = u8::from_str_radix(hex, 16).ok()?;
            out.push(byte);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_a_mysql_url_into_its_parts() {
        let parts = parse_dsn("mysql://app:secret@db.internal:3306/shop").expect("parses");
        assert_eq!(parts.host, "db.internal");
        assert_eq!(parts.port, Some(3306));
        assert_eq!(parts.user, "app");
        assert_eq!(parts.database, "shop");
        assert_eq!(parts.query, "");
    }

    #[test]
    fn splits_a_postgres_url_into_its_parts() {
        let parts = parse_dsn("postgres://app:secret@db.example.com:5432/analytics").expect("ok");
        assert_eq!(parts.host, "db.example.com");
        assert_eq!(parts.port, Some(5432));
        assert_eq!(parts.user, "app");
        assert_eq!(parts.database, "analytics");
    }

    // The whole point of the type: a prefill payload built from it cannot leak
    // the password, because there is nowhere to put one.
    #[test]
    fn parts_never_carry_the_password() {
        let parts = parse_dsn("mysql://app:hunter2@db:3306/shop").expect("parses");
        let rendered = format!("{parts:?}");
        assert!(
            !rendered.contains("hunter2"),
            "password leaked into the parts: {rendered}"
        );
    }

    #[test]
    fn an_omitted_port_means_the_engine_default() {
        let parts = parse_dsn("mysql://app@db.internal/shop").expect("parses");
        assert_eq!(parts.port, None);
    }

    #[test]
    fn a_url_without_a_password_still_splits() {
        let parts = parse_dsn("mysql://app@db.internal:3306/shop").expect("parses");
        assert_eq!(parts.user, "app");
        assert_eq!(parts.database, "shop");
    }

    #[test]
    fn percent_encoded_user_and_database_come_back_decoded() {
        let parts = parse_dsn("mysql://a%20b@db:3306/my%20db").expect("parses");
        assert_eq!(parts.user, "a b");
        assert_eq!(parts.database, "my db");
    }

    // sqlx is handed the query verbatim, so `?ssl-mode=disabled` is load-bearing
    // (ADR-0078). Dropping it on a round-trip would re-enable a TLS requirement
    // the user turned off, and the connection would start failing again.
    #[test]
    fn the_query_string_survives_the_split() {
        let parts = parse_dsn("mysql://app:s@db:3306/shop?ssl-mode=disabled").expect("parses");
        assert_eq!(parts.query, "ssl-mode=disabled");
        assert_eq!(parts.database, "shop");
    }

    #[test]
    fn an_ipv6_host_comes_back_without_its_brackets() {
        let parts = parse_dsn("mysql://app@[::1]:3306/shop").expect("parses");
        assert_eq!(parts.host, "::1");
    }

    #[test]
    fn an_empty_database_is_empty_not_a_slash() {
        let parts = parse_dsn("mysql://app@db:3306/").expect("parses");
        assert_eq!(parts.database, "");
    }

    #[test]
    fn garbage_does_not_parse() {
        assert!(parse_dsn("not a url").is_none());
        assert!(parse_dsn("").is_none());
    }

    #[test]
    fn a_url_with_no_host_does_not_parse() {
        assert!(parse_dsn("mysql:///shop").is_none());
    }

    #[test]
    fn grafts_the_stored_password_onto_a_rebuilt_url() {
        let stored = "mysql://app:hunter2@old-host:3306/shop";
        let rebuilt = "mysql://app@new-host:3307/other";
        let merged = with_password(rebuilt, stored).expect("merges");
        assert_eq!(merged, "mysql://app:hunter2@new-host:3307/other");
    }

    #[test]
    fn keeps_the_rebuilt_query_when_grafting() {
        let stored = "mysql://app:hunter2@db:3306/shop";
        let rebuilt = "mysql://app@db:3306/shop?ssl-mode=disabled";
        let merged = with_password(rebuilt, stored).expect("merges");
        assert_eq!(merged, "mysql://app:hunter2@db:3306/shop?ssl-mode=disabled");
    }

    // An account with no password is legal; grafting must not invent an empty
    // `:` section, which would change the URL sqlx parses.
    #[test]
    fn a_stored_url_without_a_password_leaves_the_target_alone() {
        let merged =
            with_password("mysql://app@db:3306/shop", "mysql://app@db:3306/shop").expect("merges");
        assert_eq!(merged, "mysql://app@db:3306/shop");
    }

    #[test]
    fn a_password_needing_encoding_is_re_encoded_on_the_way_in() {
        let stored = "mysql://app:p%40ss%2Fword@db:3306/shop";
        let merged = with_password("mysql://app@db:3306/shop", stored).expect("merges");
        // Round-trips: the merged URL must parse back to the same password.
        let read_back = Url::parse(&merged).expect("parses").password().map(decode);
        assert_eq!(read_back.as_deref(), Some("p@ss/word"));
    }

    #[test]
    fn grafting_onto_garbage_fails_rather_than_guessing() {
        assert!(with_password("not a url", "mysql://app:s@db:3306/shop").is_none());
        assert!(with_password("mysql://app@db:3306/shop", "not a url").is_none());
    }
}
