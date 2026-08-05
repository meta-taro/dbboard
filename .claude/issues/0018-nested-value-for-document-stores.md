# 0018: `Value` cannot hold a document, so no document store can return a row

- **Status**: open
- **Opened**: 2026-08-05
- **Owner**: unassigned
- **Related ADRs**: ADR-0091 (document stores join through the same trait),
  ADR-0009 (domain types cross the loopback HTTP wire)
- **Blocks**: 0019 (Firestore adapter), 0020 (MongoDB adapter)

## Problem

`dbboard_core::Value` is `Null | Integer | Real | Text | Blob`
(`crates/dbboard-core/src/value.rs:17`). A query result is rows of those under
named columns. That is exactly the shape a SQL result set has, and it is why the
seven existing adapters fit it without friction.

A MongoDB or Firestore document is a tree. There is no variant it can go into.
Flattening one into `Text` holding a JSON string would technically compile and
would push the parsing into the frontend, where each cell would have to guess
whether a string is a string or a serialised document — the same category of
mistake the `$blob` tag exists to avoid.

This is the shared prerequisite for both document adapters, which is why it is
its own issue: it changes a type that every adapter constructs and the frontend
renders, so it should land alone, where a regression is attributable to one
change.

## What has to change

- A nested variant on `Value`, carrying a parsed JSON tree. This makes
  `serde_json` a real dependency of `dbboard-core` rather than the dev-only one
  it is today. That is consistent with the existing carve-outs in
  `crates/dbboard-core/Cargo.toml`: parsing is a pure data transformation, so
  the crate's no-I/O rule still holds — the same argument already written down
  for `serde` and `sqlparser`.
- Hand-written `Serialize` / `Deserialize` arms alongside the existing ones.
  `Value` deliberately does *not* use serde's externally-tagged default so the
  wire form reads like ordinary JSON; the new variant follows the `$blob`
  precedent and rides inside a tagged object.
- `docs/api-contract.md` updated **before** the adapter work starts. The
  contract is shared with `dbboard-web`, and that repo cannot be edited from
  here — publishing the tag first is what lets the sibling implement against it
  instead of after it.
- Every adapter's row construction and the frontend's cell rendering reviewed
  for the new variant. Nothing existing should start producing it; this change
  is additive.

## Completion criteria

- [ ] `Value` round-trips a nested document through serialize → deserialize with
      the tag documented in `docs/api-contract.md`
- [ ] The tag is unambiguous against a natural string value, the way `$blob` is
- [ ] No existing adapter's output changes (regression tests unchanged and green)
- [ ] The frontend renders the new variant readably rather than as `[object Object]`
- [ ] `docs/api-contract.md` states the tag, and `.claude/next-actions.md`
      carries the "tell the web repo" hand-off explicitly (silence there once
      blocked the sibling for three weeks)

## Verification

```sh
cargo test -p dbboard-core --all-features
cargo clippy --all-targets --all-features -- -D warnings
```
