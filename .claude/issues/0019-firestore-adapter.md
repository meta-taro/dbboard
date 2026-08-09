# 0019: Firestore adapter (`dbboard-firestore`)

- **Status**: closed — the crate is done, reachable from both the client and the
  MCP server, and verified end-to-end against the local Firestore emulator
- **Opened**: 2026-08-05
- **Owner**: unassigned
- **Related ADRs**: ADR-0091 (document stores join through the same trait),
  ADR-0093 (REST directly, `ring` for signing, `query_read_only` overridden),
  ADR-0094 (optional credential, and a browse that is not SQL),
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
- [x] Exposed to the MCP server only after the read-only guarantee is tested

## Slice 2: reachable from the client — done (ADR-0094)

- [x] `BackendConfig::Firestore` + a `connect_adapter` arm in `dbboard-connect`.
- [x] Service-account JSON through the same keychain path every other secret
      takes — never a tracked file. `DBBOARD_FIRESTORE_*` env vars for the agent
      case (ADR-0090 §4).
- [x] Connection add **and** edit forms in the desktop client, changed together.
      The emulator is a checkbox, because a blank credential is a *choice* here
      and not an unfinished form (ADR-0094 §1).
- [x] The sidebar generates a `StructuredQuery` rather than SQL, and offers no
      *Count rows* on a connection that cannot count (ADR-0094 §4).
- [x] `dbboard-mcp` registration, which the read-only test above now unblocks.

## Slice 3: reachable from an agent — done

There was no wiring to add: the MCP service opens connections through
`backend_config_for_entry` + `connect_adapter`, which slice 2 already taught
about Firestore, and every tool goes through the trait. What was missing was
*honesty*, which is the part an agent actually consumes.

- [x] `list_connections` names every kind `kind_label` can return, guarded by a
      test that reads the two source spans rather than a hand-kept list — it
      had already gone stale twice (MySQL, Aurora DSQL IAM) before Firestore
      made staleness expensive.
- [x] `run_read_query` states that a `firestore` connection takes a
      `StructuredQuery` rather than SQL, with a bounded example. Without this
      an agent sends `SELECT`, gets a parse error, and reads it as its own
      mistake.
- [x] `describe_table` says its Firestore result is inferred from a sample, not
      declared.

## Slice 4: verified against a real Firestore — done

`wiremock` proves the crate sends what we *believe* Firestore accepts. It cannot
prove Firestore accepts it. `crates/dbboard-connect/tests/firestore_emulator.rs`
closes that gap against the local emulator, and goes through `connect_adapter`
rather than the adapter directly, so it covers the same wiring the desktop
client and the MCP server both use.

- [x] `ping`, `list_tables`, `query`, `describe_table` answered by the emulator.
- [x] The browse query is asserted as the *exact* string `browseQuery` generates
      (ADR-0094 §4) — a change there that the emulator would reject fails here
      rather than in front of a user.
- [x] A nested map survives as an `address` column (the issue 0018 `Value::Json`
      variant), and no collection is reported with a schema namespace.
- [x] `execute` is refused rather than attempted.

The tests are `#[ignore]`d and gated on `DBBOARD_TEST_FIRESTORE_EMULATOR`, so
CI — which has no emulator — is unaffected.

## Remaining

Nothing blocking. Pointing the client at a real cloud project still needs a
service-account credential, but that path is the same one every other secret
takes and the emulator exercises everything above it.

## Verification

```sh
cargo test -p dbboard-firestore --all-features
cargo clippy --all-targets --all-features -- -D warnings
```

And, against the emulator. The port is not a CLI flag — it comes from a
`firebase.json` in whatever directory you start it from, so put one in a scratch
directory. 8385 rather than the default 8080 because this machine is shared and
8080 is usually already taken:

```json
{ "emulators": { "firestore": { "port": 8385, "host": "127.0.0.1" }, "ui": { "enabled": false } },
  "firestore": { "rules": "firestore.rules" } }
```

```sh
firebase emulators:start --only firestore --project demo-dbboard
DBBOARD_TEST_FIRESTORE_EMULATOR=http://127.0.0.1:8385/v1 \
  cargo test -p dbboard-connect --test firestore_emulator --all-features -- --ignored
```

The project id must stay `demo-`prefixed: that is the Firebase tooling's own
marker for "never contact production", and it is what stops a stray credential
from turning this into a live write.
