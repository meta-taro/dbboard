# 0020: MongoDB adapter (`dbboard-mongodb`)

- **Status**: open
- **Opened**: 2026-08-05
- **Owner**: unassigned
- **Related ADRs**: ADR-0091 (document stores join through the same trait),
  ADR-0046 (read-only classifier), ADR-0087 (MCP write policy)
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

## Open questions to settle during implementation

- The official `mongodb` crate versus the REST-ish alternatives. A non-trivial
  crate choice gets its own ADR entry per CLAUDE.md.
- Whether transactions map onto `execute_in_transaction` usefully, or whether
  that capability is better declared absent than half-implemented.

## Completion criteria

- [ ] The read-only classifier is unit-tested against adversarial input to the
      same bar as `read_only.rs`, including `$out` / `$merge` inside an
      otherwise-read `aggregate`, and refuses anything it cannot prove read-only
- [ ] `ping`, `list_tables`, `query`, `describe_table` implemented, with tests
      that need no live cluster
- [ ] Nested documents round-trip through the `Value` variant from issue 0018
- [ ] `capabilities()` matches what is actually implemented
- [ ] ADR entry for any non-trivial crate added
- [ ] Exposed to the MCP server only after the classifier tests are green

## Verification

```sh
cargo test -p dbboard-mongodb --all-features
cargo clippy --all-targets --all-features -- -D warnings
```
