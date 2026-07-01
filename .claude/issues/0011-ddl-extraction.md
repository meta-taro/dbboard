# 0011: Full DDL extraction via `DatabaseAdapter::describe_table` (Phase 4 Stage 2 Group D-1)

- **Status**: open 2026-07-01
- **Phase**: 4 Stage 2 Group D-1 (the first of two Group D ADRs).
  Group A (issue 0008 / PR #43), Group B (issue 0009 / PR #45), Group C
  (issue 0010 / PR #47) all closed on `develop`.
- **Opened**: 2026-07-01
- **Tracks**: ADR-0028
- **Cross-repo brief**: none. `describe_table` is desktop-side
  `DatabaseAdapter` trait extension; no HTTP contract change, no
  `history.jsonl` schema bump. Web has its own connection-management
  story and would decide its own DDL-fetching shape independently
  (same posture as ADR-0025 / ADR-0026 = in-process only, no web
  mirror).
- **Depends on**: ADR-0023 (the `DatabaseAdapter` trait and
  `TableInfo` + `ColumnInfo` types this slice extends).
- **Prerequisite for**: ADR-0029 (function-calling / tool-use) —
  `describe_table` becomes the first exposed tool there.

## Context

ADR-0028 records the design. This issue is the implementation
tracker; anything that surfaces here as a Decision-level surprise
becomes a follow-up ADR, not a silent edit.

Three observations motivate this slice (full discussion in
ADR-0028 §Context):

1. Current `list_tables()` returns names only — AI Suggest
   hallucinates column names because the prompt has no column info.
2. `ColumnInfo` already exists in `dbboard-core::schema` and is
   unused. This ADR closes the loop with one required-with-default
   trait method + one new sibling struct (`TableSchema`).
3. Function-calling (ADR-0029) needs a real tool to expose;
   `describe_table` is the natural first tool for a DB-companion AI.

## Acceptance

### Slice (a) — `dbboard-core` trait + types

- [ ] New `TableSchema { table: TableInfo, columns: Vec<ColumnInfo>,
      primary_key: Vec<String> }` struct in
      `crates/dbboard-core/src/schema.rs`.
- [ ] `ColumnInfo` extended with `ordinal: u32` +
      `default_value: Option<String>` (additive).
- [ ] `DatabaseAdapter::describe_table(&self, table: &TableInfo) ->
      DbResult<TableSchema>` trait method with default impl returning
      `DbError::Capability("describe_table not supported by this
      adapter")`.
- [ ] `Capabilities::has_describe_table: bool` additive field,
      default `false`, round-trips through JSON.
- [ ] Unit tests: capability flag round-trip; default trait impl
      surfacing the `Capability` error; `TableSchema` construction
      round-trip.
- [ ] Existing adapters compile unchanged (they inherit the default
      impl and their `capabilities()` output does not gain the new
      flag).

### Slice (b) — per-adapter implementations

- [ ] `dbboard-postgres::PostgresAdapter::describe_table`:
      `information_schema.columns` (columns) + composite-PK query
      via `information_schema.table_constraints` +
      `key_column_usage`. `capabilities().has_describe_table = true`.
      Integration test against `postgres:16-alpine` via testcontainers
      (Docker-skip guard).
- [ ] `dbboard-turso::TursoAdapter::describe_table`: single
      `PRAGMA table_info('<name>')` call; composite PK materialised
      by collecting rows with `pk > 0` sorted by `pk`.
      `capabilities().has_describe_table = true`. Unit test against
      an in-memory libsql DB.
- [ ] `dbboard-d1::D1Adapter::describe_table`: same PRAGMA query as
      Turso, over the existing raw-HTTP envelope path.
      `capabilities().has_describe_table = true`. Test via the
      mocked-HTTP layer.
- [ ] Missing-table cases surface as the engine's native error
      (mapped to `DbError::Query`) — assertion in one adapter test.

### Slice (c) — `dbboard-ai` + `dbboard-ui`

- [ ] `dbboard-ai::SuggestRequest.full_schema:
      Option<Vec<TableSchema>>` additive field.
      `dbboard-anthropic::AnthropicProvider` renders `full_schema`
      into the prompt when present (existing `schema` path stays for
      names-only).
- [ ] `dbboard-ui::worker` gains `Command::PrefetchSchema { tables:
      Vec<TableInfo> }` + `Reply::SchemaPrefetched { schemas:
      Vec<TableSchema>, errors: Vec<(TableInfo, String)> }`.
      Fan-out capped at 8 concurrent `describe_table` calls via
      `tokio::sync::Semaphore`.
- [ ] `dbboard-ui::ai::AiPanel` gains an "Include column details"
      checkbox, gated on `has_describe_table`. When checked and
      Send is pressed, PrefetchSchema fires first, then Suggest with
      `full_schema` populated. Warning banner on partial failure
      ("N tables could not be described — Suggest will use partial
      schema").
- [ ] Session-local toggle state (not persisted).
- [ ] Unit tests: toggle on/off round-trip; PrefetchSchema
      dispatch; partial-failure banner render.

### Slice (d) — docs sweep

- [ ] ADR-0028 status Proposed (2026-07-01) → Accepted (date TBD),
      slice commit hashes embedded in ADR body (matching the
      ADR-0026 `fff669c` / ADR-0027 `34ad0eb` slice-d placeholder
      pattern).
- [ ] `docs/roadmap.md` Phase 4 Stage 2 Group D-1 entry ticked.
- [ ] `README.md` AI section gains one paragraph about the
      Include-column-details toggle (schema bytes go into the AI
      context window; cost implications).
- [ ] This issue closed with all boxes ticked.
- [ ] `.claude/project-status.md` slice landing record.
- [ ] `.claude/next-actions.md` regenerated for the post-Group-D-1
      state (Group D-2 = ADR-0029 becomes the standing next action).

## Explicit non-goals

- Indexes and foreign keys (deferred to a future ADR).
- View / function / stored-procedure DDL (existing optional trait
  accessors already exist and can grow their own describe methods
  later).
- Batch `describe_tables(&[TableInfo])` (fan-out from the UI is
  enough at expected caller sizes).
- Schema browser UI (natural follow-up but not gating).
- Persisting the toggle across sessions.
- `CREATE TABLE` text rendering.
- In-adapter caching.

## Risks

- **Prompt cost** for large schemas — toggle is off by default;
  Anthropic token meter (ADR-0026) surfaces the cost.
- **Fan-out load** on shared Postgres — semaphore-capped at 8.
- **Cross-adapter type drift** (`text` vs `TEXT` etc.) — raw
  `declared_type` retained; AI reads dialect from
  `SuggestRequest.dialect`.
- **Stale `TableInfo` between `list_tables` and `describe_table`** —
  engine error → `DbError::Query` → UI prompts refresh (Decision 6).

## Notes

- ADR-0023 §7 called the queued method `dump_schema`;
  ADR-0028 names it `describe_table` because dumping the whole DB
  is wasteful for large schemas and awkward for the
  function-calling use case (ADR-0029). `dump_schema` can be added
  as a batch helper in a future ADR if profiling shows the fan-out
  is the bottleneck.
- The four-slice single-branch pattern (ADR-0026 / ADR-0027
  precedent) is used here too. All four slices land on
  `feature/ddl-extraction` and ship in one feat PR + a small
  post-merge doc-sync chore PR (matching the PR #45/#46 and
  PR #47/#48 pattern).
