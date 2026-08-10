//! The only Firestore endpoints this adapter can address.
//!
//! ADR-0091 decided that read-only enforcement for Firestore is structural
//! rather than parsed: the REST API splits reads (`:runQuery`,
//! `:listCollectionIds`, `GET .../{collection}`) from writes (`:commit`,
//! `:batchWrite`, `:rollback`) at the endpoint, so "this connection may not
//! write" can be expressed as "this code cannot build a write URL".
//!
//! That is what this enum is. Every request the adapter makes goes through
//! [`ReadEndpoint::path`], and the enum has no write variant — so a write is
//! not a check that can be bypassed, it is a URL that does not exist here.
//! [`ReadEndpoint::is_read`] matches exhaustively with no catch-all arm, so
//! adding a variant that writes would fail to compile until someone answers
//! for it.

use reqwest::Method;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ReadEndpoint {
    /// `POST .../documents:runQuery` — run a `StructuredQuery`.
    RunQuery,
    /// `POST .../documents:listCollectionIds` — the collection ids under the
    /// documents root, which is what `list_tables` reports.
    ListCollectionIds,
    /// `GET .../documents/{collection}` — read documents from one collection
    /// without a query, used for schema sampling.
    ListDocuments { collection: String },
}

impl ReadEndpoint {
    /// The request path, relative to the API root, for a database whose
    /// documents root is `documents_root` (e.g.
    /// `projects/p/databases/(default)/documents`).
    pub(crate) fn path(&self, documents_root: &str) -> String {
        match self {
            Self::RunQuery => format!("{documents_root}:runQuery"),
            Self::ListCollectionIds => format!("{documents_root}:listCollectionIds"),
            Self::ListDocuments { collection } => format!("{documents_root}/{collection}"),
        }
    }

    pub(crate) fn method(&self) -> Method {
        match self {
            Self::RunQuery | Self::ListCollectionIds => Method::POST,
            Self::ListDocuments { .. } => Method::GET,
        }
    }

    /// Whether this endpoint only reads.
    ///
    /// Total over the enum with no catch-all arm: a new variant does not
    /// silently inherit `true`, it breaks the build here.
    pub(crate) fn is_read(&self) -> bool {
        match self {
            Self::RunQuery | Self::ListCollectionIds | Self::ListDocuments { .. } => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all() -> Vec<ReadEndpoint> {
        vec![
            ReadEndpoint::RunQuery,
            ReadEndpoint::ListCollectionIds,
            ReadEndpoint::ListDocuments {
                collection: "users".to_owned(),
            },
        ]
    }

    #[test]
    fn every_endpoint_this_adapter_can_reach_is_a_read() {
        for endpoint in all() {
            assert!(
                endpoint.is_read(),
                "{endpoint:?} is reachable but is not a read"
            );
        }
    }

    #[test]
    fn no_endpoint_resolves_to_a_firestore_write_verb() {
        // The mutating verbs of the Firestore REST surface. A write path could
        // not be added without naming one of these, and none is reachable.
        const WRITE_VERBS: [&str; 4] = [":commit", ":write", ":batchWrite", ":rollback"];
        for endpoint in all() {
            let path = endpoint.path("projects/p/databases/(default)/documents");
            for verb in WRITE_VERBS {
                assert!(
                    !path.contains(verb),
                    "{endpoint:?} produced a write path: {path}"
                );
            }
        }
    }

    #[test]
    fn run_query_targets_the_documents_root() {
        assert_eq!(
            ReadEndpoint::RunQuery.path("projects/p/databases/(default)/documents"),
            "projects/p/databases/(default)/documents:runQuery"
        );
    }

    #[test]
    fn list_collection_ids_targets_the_documents_root() {
        assert_eq!(
            ReadEndpoint::ListCollectionIds.path("projects/p/databases/(default)/documents"),
            "projects/p/databases/(default)/documents:listCollectionIds"
        );
    }

    #[test]
    fn list_documents_appends_the_collection_id() {
        assert_eq!(
            ReadEndpoint::ListDocuments {
                collection: "users".to_owned()
            }
            .path("projects/p/databases/(default)/documents"),
            "projects/p/databases/(default)/documents/users"
        );
    }

    #[test]
    fn http_method_matches_the_endpoint() {
        assert_eq!(ReadEndpoint::RunQuery.method(), Method::POST);
        assert_eq!(ReadEndpoint::ListCollectionIds.method(), Method::POST);
        assert_eq!(
            ReadEndpoint::ListDocuments {
                collection: "users".to_owned()
            }
            .method(),
            Method::GET
        );
    }
}
