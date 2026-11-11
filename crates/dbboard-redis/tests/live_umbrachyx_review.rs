//! Live test of the review queue against a real Umbrachyx server
//! (ADR-0176).
//!
//! Self-skips unless `DBBOARD_UMBRACHYX_REVIEW_URL` is set. The URL carries a
//! person's approve token as its password, and the server must have been
//! started with `--public-key`. It needs one waiting change that the token
//! cannot approve, on the branch named in `DBBOARD_UMBRACHYX_REVIEW_BRANCH`.
//! The test never approves anything, so it can be run twice.
//!
//! ```sh
//! umbrachyx keygen root.key > pub.txt
//! umbrachyx commit main player/7/gems=30 --who game --why start
//! umbrachyx branch agent-b --from main
//! umbrachyx commit agent-b pos/7=c3 --who agent:mover --why move
//! umbrachyx token issue --key root.key --subject person:lead --prefix player \
//!   --columns value --operations read,approve --purpose review --expires-in 8h > lead.tok
//! umbrachyx serve agent-b --port 16400 --public-key "$(cat pub.txt)" &
//! DBBOARD_UMBRACHYX_REVIEW_URL="redis://:$(cat lead.tok)@127.0.0.1:16400" \
//! DBBOARD_UMBRACHYX_REVIEW_BRANCH=agent-b \
//!   cargo test -p dbboard-redis --test live_umbrachyx_review
//! ```

use dbboard_core::{DatabaseAdapter, ReviewError, ReviewRefusal, TableInfo};
use dbboard_redis::{RedisAdapter, RedisConfig};

#[tokio::test]
async fn a_waiting_change_is_listed_diffed_and_guarded() {
    let (Some(url), Some(branch)) = (
        std::env::var("DBBOARD_UMBRACHYX_REVIEW_URL")
            .ok()
            .filter(|v| !v.is_empty()),
        std::env::var("DBBOARD_UMBRACHYX_REVIEW_BRANCH")
            .ok()
            .filter(|v| !v.is_empty()),
    ) else {
        eprintln!("skipping: DBBOARD_UMBRACHYX_REVIEW_URL / _BRANCH not set");
        return;
    };
    let adapter = RedisAdapter::connect(RedisConfig {
        url,
        cluster: false,
        route: None,
    })
    .await
    .expect("connect");
    assert!(
        adapter.capabilities().has_review,
        "no review queue detected"
    );
    let review = adapter.review().expect("a review queue");

    let changes = review.pending().await.expect("UMBRA.PENDING");
    let change = changes
        .iter()
        .find(|c| c.branch == branch)
        .expect("the change set up for this test is waiting");
    assert!(!change.tables.is_empty());

    // The branch's own change, from where it left main.
    let history = adapter.history().expect("history");
    let from = change.base.clone().unwrap_or_else(|| "-".into());
    for t in &change.tables {
        let diff = history
            .diff(
                &TableInfo::unqualified(format!("{}/*", t.table)),
                &from,
                &change.head,
            )
            .await
            .expect("UMBRA.DIFF");
        assert_eq!(diff.changes.len() as u64, t.changed);
    }

    // The token can approve `player` only; this change touches more.
    match review
        .approve(&change.branch, &change.head, "live test")
        .await
    {
        Err(ReviewError::Refused(ReviewRefusal::NoPermission { .. })) => {}
        other => panic!("expected NOPERM, got {other:?}"),
    }
}
