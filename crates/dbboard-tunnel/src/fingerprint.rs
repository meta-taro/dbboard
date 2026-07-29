//! Pure host-key-fingerprint helpers.
//!
//! OpenSSH renders a key fingerprint as `SHA256:<base64>` (Display of
//! `ssh_key::Fingerprint`). Users pin it by copy-pasting that string, so the
//! comparison must tolerate a present-or-absent `SHA256:` prefix and
//! surrounding whitespace, while staying byte-exact on the base64 body (which
//! is case-sensitive).

/// Strip an optional `SHA256:` prefix and surrounding whitespace, leaving the
/// bare base64 fingerprint body.
#[must_use]
pub fn normalize_fingerprint(fp: &str) -> String {
    let trimmed = fp.trim();
    trimmed
        .strip_prefix("SHA256:")
        .unwrap_or(trimmed)
        .trim()
        .to_string()
}

/// True when two fingerprints refer to the same key, ignoring the optional
/// `SHA256:` prefix and whitespace but comparing the base64 body exactly.
#[must_use]
pub fn fingerprint_matches(expected: &str, actual: &str) -> bool {
    let expected = normalize_fingerprint(expected);
    // An empty expectation can never match — otherwise a blank pin would
    // silently accept every host, defeating the whole policy.
    !expected.is_empty() && expected == normalize_fingerprint(actual)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_sha256_prefix_and_whitespace() {
        assert_eq!(normalize_fingerprint("SHA256:abc123"), "abc123");
        assert_eq!(normalize_fingerprint("  SHA256:abc123  "), "abc123");
        assert_eq!(normalize_fingerprint("abc123"), "abc123");
    }

    #[test]
    fn matches_with_or_without_prefix() {
        assert!(fingerprint_matches("SHA256:abc123", "abc123"));
        assert!(fingerprint_matches("abc123", "SHA256:abc123"));
        assert!(fingerprint_matches("  SHA256:abc123 ", "SHA256:abc123"));
    }

    #[test]
    fn body_is_case_sensitive() {
        // base64 is case-sensitive: 'A' and 'a' are different bytes.
        assert!(!fingerprint_matches("SHA256:ABC", "SHA256:abc"));
    }

    #[test]
    fn different_keys_do_not_match() {
        assert!(!fingerprint_matches("SHA256:aaa", "SHA256:bbb"));
    }

    #[test]
    fn empty_expectation_never_matches() {
        // A blank pin must fail closed, not accept everything.
        assert!(!fingerprint_matches("", ""));
        assert!(!fingerprint_matches("   ", "SHA256:abc"));
        assert!(!fingerprint_matches("SHA256:", "SHA256:abc"));
    }
}
