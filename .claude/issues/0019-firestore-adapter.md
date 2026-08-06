# 0019: Firestore adapter (`dbboard-firestore`)

- **Status**: open — the crate is done; wiring it into the client is not
- **Opened**: 2026-08-05
- **Owner**: unassigned
- **Related ADRs**: ADR-0091 (document stores join through the same trait),
  ADR-0093 (REST directly, `ring` for signing, `query_read_only` overridden),
  ADR-0046 (read-only MCP surface), ADR-0087 (MCP write policy)
- **Blocked by**: 0018 (nested `Value`)

## Goal

A `DatabaseAdapter` implementation for Cloud Firestore, reachable from the
desktop client and — once its read-only story is proven — from the MCP server.

## Why this one first, ahead of MongoDB

Firestore's REST API splits reads from writes at the *endpoint*: `:runQuery`
and `:batchGet` read, `:commit` writes. So read-only enforcement is "which
endpoints this connection may call", not "parse this string and decide whether
it mutates". There is nothing to classify and therefore nothing to get wrong in
a classifier — which is the expensive, safety-critical part of the MongoDB work
(issue 0020).

That makes Firestore the cheaper way to find out whether the rest of the design
in ADR-0091 actually holds: native JSON query text through `query`, nested
`Value` for results, sampled schema for a schemaless collection.

## Shape

- **Query text** is a Firestore `StructuredQuery` as JSON. The `query` parameter
  is the adapter's own native form (ADR-0091) — no translation layer, and no
  invented query language.
- **`list_tables`** returns collections.
- **`describe_table`** samples a bounded number of documents and reports the
  field union with the sample size and per-field frequency, rendered so it is
  visibly an inference (ADR-0091 §4).
- **Read-only** is enforced by endpoint. A read-only connection never holds a
  code path to `:commit`.
- **Credentials**: service-account JSON. It has to go through the same handling
  as every other secret — never into a tracked file, and `DBBOARD_*` env vars
  documented for the agent case (ADR-0090 §4).
- **`capabilities()`** declares honestly what is absent. There are no foreign
  keys, no DDL to reconstruct, and no multi-statement transaction in the SQL
  sense; the trait already returns a capability error for each rather than
  faking one.

## Open questions — settled (ADR-0093)

- **Which crate.** None: the three endpoints needed are a documented, stable
  REST surface, and `reqwest`/`serde_json` are already in the tree. A client
  crate that exposes writes would also defeat the read-only story below.
  Signing the service-account assertion uses `ring`, already present via
  rustls-ring; `rsa`/`rand` are dev-dependencies for generating throwaway test
  keys.
- **Emulator support.** Every HTTP path is covered by `wiremock`, and the
  emulator's fixed `Bearer owner` credential is a first-class variant. No test
  touches a cloud project, and no key is committed.

## Completion criteria

- [x] `ping`, `list_tables`, `query`, `describe_table` implemented against the
      emulator, with unit tests that need no cloud project
- [x] A read-only connection has no reachable write path, proven by a test, not
      by inspection
- [x] Nested documents round-trip through the `Value` variant from issue 0018
- [x] `capabilities()` matches what is actually implemented
- [x] ADR entry for any non-trivial crate added
- [ ] Exposed to the MCP server only after the read-only guarantee is tested

## Remaining: reachable from the client

The crate is complete and green; nothing outside it knows Firestore exists.
The next slice is the wiring, kept separate because it touches secret storage
and the UI rather than the adapter:

- `BackendConfig::Firestore` + a `connect_adapter` arm in `dbboard-connect`.
- Service-account JSON through the same keychain path every other secret takes
  — never a tracked file. `DBBOARD_*` env vars for the agent case (ADR-0090 §4).
- Connection add **and** edit forms in the desktop client, changed together.
- `dbboard-mcp` registration, which the read-only test above now unblocks.

## Verification

```sh
cargo test -p dbboard-firestore --all-features
cargo clippy --all-targets --all-features -- -D warnings
```
