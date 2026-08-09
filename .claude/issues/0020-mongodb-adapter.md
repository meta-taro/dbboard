# 0020: MongoDB adapter (`dbboard-mongodb`)

- **Status**: open — slices 1 (the classifier) and 2 (the driver and the
  adapter) are done; the wiring into `dbboard-connect`, the desktop client and
  the MCP server is next
- **Opened**: 2026-08-05
- **Owner**: unassigned
- **Related ADRs**: ADR-0091 (document stores join through the same trait),
  ADR-0046 (read-only classifier), ADR-0087 (MCP write policy),
  ADR-0095 (the command allowlist), ADR-0096 (the driver and the double parse)
- **Blocked by**: 0018 (nested `Value`)

## Goal

A `DatabaseAdapter` implementation for MongoDB, reachable from the desktop
client and — only after its read-only classifier is proven — from the MCP
server.

## The part that is actually hard

`read_only.rs` cannot help here. It is `sqlparser`-based by design and says so
in its first line, and a classifier that cannot parse its input must fail
closed — which would mean refusing every Mongo query.

So MongoDB needs its own classifier, and unlike Firestore (issue 0019) it
cannot lean on the transport: `runCommand` accepts any command verb, so
"which endpoint" carries no information. The rule is an **explicit allowlist of
read commands** — `find`, `aggregate`, `count`, `distinct`, and whatever else
survives review — with everything unlisted refused. Fail closed, exactly as D1
does.

Two traps that make this more than a verb check:

- **`$out` and `$merge` are aggregation stages that write.** An `aggregate`
  command is on the read list and can still mutate the database. The pipeline
  has to be walked, not just the verb read — the same reason `read_only.rs`
  refuses to do `starts_with("SELECT")` matching, where
  `WITH x AS (DELETE … RETURNING *) SELECT * FROM x` is a write that starts
  with `WITH`.
- **`$where` and `$function` run JavaScript server-side.** Whether those belong
  on a read-only surface at all is a decision to make deliberately and write
  down, not to arrive at by omission.

This is why MongoDB is sequenced after Firestore even though it is the better
known database: the classifier is the expensive, safety-critical piece, and the
MCP write gate (ADR-0087) would be built on top of it.

## Shape

- **Query text** is a command document as JSON — the adapter's own native form
  (ADR-0091). No translation layer.
- **`list_tables`** returns collections.
- **`describe_table`** samples a bounded number of documents and reports the
  field union with sample size and per-field frequency (ADR-0091 §4).
- **Connection string** handling matches the existing adapters: never into a
  tracked file, `DBBOARD_*` env vars documented for the agent case.
- **`capabilities()`** declares honestly what is absent — no foreign keys, no
  DDL reconstruction.

## Slice 1: the classifier — done (ADR-0095)

`crates/dbboard-mongodb/src/read_only.rs`, pure and I/O-free, 18 tests. The
crate leads with this rather than with a driver, so the safety-critical piece
is reviewable on its own terms before anything can connect.

Two decisions that were not obvious going in:

- **The options are allowlisted too, not just the verbs.** A denylist of write
  verbs has to be complete to be sound, and MongoDB adds commands. With an
  option allowlist, `{"find": …, "insert": …, "documents": …}` is refused
  because `insert` is not a `find` option — nobody has to remember that it
  writes.
- **The parse keeps field order.** MongoDB reads the command name from the
  document's *first* field, and `serde_json::Value` sorts its map, so a
  classifier built on `Value` would read `find` out of
  `{"filter": …, "find": …}`. `CommandDoc` is a `Vec<(String, Value)>` with a
  local `Deserialize` impl, rather than the `preserve_order` feature, whose
  unification would reorder maps for every crate in the build.

Server-side JavaScript (`$where`, `$function`, `$accumulator`) is refused —
recorded in ADR-0095 §5 so that re-allowing it is a decision someone makes
rather than a gap someone finds.

## Slice 2: the driver and the adapter — done (ADR-0096)

The official `mongodb` crate at 3.8, default features plus `redact-errors`;
`MongoAdapter` with `ping`, `list_tables`, `query`, `query_read_only` and
`describe_table`; and three pure modules beside it — `document.rs` (BSON to
`Value`), `command.rs` (approved text to the wire document) and `sample.rs`
(bounded-sample schema inference). 67 unit tests, none of which needs a
server, plus 10 `#[ignore]`d end-to-end tests in `tests/live_mongodb.rs` that
were run green against `mongo:8` in Docker.

The two open questions above are now settled:

- **The official driver.** MongoDB's wire protocol is binary, with SCRAM,
  server discovery and pooling behind it — a much larger surface to
  reimplement than to depend on. Its default TLS is `rustls/ring`, which is
  what ADR-0034 asks for. The one gap, recorded rather than papered over, is
  that the driver offers no native-roots option (ADR-0096).
- **Transactions are declared absent.** This adapter cannot write at all, so
  there is nothing for `execute_in_transaction` to wrap.

One thing worth writing down because it was not obvious: the command text is
parsed *twice*, once as `serde_json` for the classifier and once as
`bson::Document` for the wire. `serde_json`'s map is sorted, so a two-key
`sort` would silently become a different sort, and `{"$oid": …}` would go out
as a subdocument and match nothing. Both parsers keep the last of a duplicated
key and the classifier walks every duplicate, so the classifier still sees a
superset of what the server is asked to run (ADR-0096 §3).

## Completion criteria

- [x] The read-only classifier is unit-tested against adversarial input to the
      same bar as `read_only.rs`, including `$out` / `$merge` inside an
      otherwise-read `aggregate`, and refuses anything it cannot prove read-only
- [x] `ping`, `list_tables`, `query`, `describe_table` implemented, with tests
      that need no live cluster
- [x] Nested documents round-trip through the `Value` variant from issue 0018
- [x] `capabilities()` matches what is actually implemented
- [x] ADR entry for any non-trivial crate added (ADR-0096)
- [ ] Exposed to the MCP server only after the classifier tests are green

## Verification

```sh
cargo test -p dbboard-mongodb --all-features
cargo clippy --all-targets --all-features -- -D warnings
```

End-to-end against a real server, which CI has none of:

```sh
docker run -d --rm --name dbboard-mongo-test -p 27117:27017 mongo:8
DBBOARD_TEST_MONGODB_URI=mongodb://127.0.0.1:27117/dbboard_test   cargo test -p dbboard-mongodb --test live_mongodb -- --ignored
docker stop dbboard-mongo-test
```
