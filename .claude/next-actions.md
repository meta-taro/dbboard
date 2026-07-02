# 次のアクション案内 (user 側ボール常設ファイル)

このファイルは「セッションが切れても、開いた瞬間に user 側のボールがわかる」状態を維持するためのもの。
セッション終了時 / 状況が変わった時に必ず更新する。**日本語可・内部用。**

---

## 最終更新

- 日付: 2026-07-02
- develop tip: `5cc01e3` (PR #48 = post-PR47 chore doc-sync merged)
- 作業ブランチ: `feature/ddl-extraction` (未 push、1 commit =
  `00ac1b8` ADR-0028 draft + tracker issue 0011)
- 直近ハイライト: **ADR-0028 (Phase 4 Stage 2 Group D-1 = full DDL
  extraction via `DatabaseAdapter::describe_table`) draft を
  `docs/decisions.md` 末尾に追記 + `.claude/issues/0011-ddl-extraction.md`
  トラッカ新設**。Group D は 2 本の独立 ADR に分割 = D-1 (ADR-0028 = DB
  adapter 側、今 draft) と D-2 (ADR-0029 = AI provider 側の
  function-calling / tool-use、D-1 完了後着手)。10 Decision + 4 slice
  plan (a: core 拡張 / b: 3 adapter 実装 / c: dbboard-ai + UI plumbing /
  d: docs sweep)。3 論点 (method 名 / v1 scope narrow / UI 部分失敗
  挙動) を maintainer review 待ち = **本セッション末時点で slice
  未着手**。

## モード

**in-use / continuous-improvement (menu-not-sequence)** — 2026-06-24 以降。
ロードマップ順ではなく実利用の摩擦報告を優先。
Group A / B / C 3 グループ完了。Group D は 2 分割で D-1 が今 draft、
D-2 は D-1 の primitive を tool として expose する構造なので D-1
完了後に着手予定。

---

## user 側のボール (= 次に着手する時の選択肢)

### **★ 最優先: ADR-0028 draft の review + 3 論点への回答**

- **何**: `feature/ddl-extraction` の `00ac1b8` に含まれる ADR-0028
  draft を読み、3 論点への OK/NG を返す。
  1. Method 名 = `describe_table(&TableInfo)` (single-table primitive)
     vs `dump_schema` (whole-DB, ADR-0023 §7 queued name) — draft 側は
     前者採用 (function-calling 用途 + 大規模 schema の効率)。
  2. v1 scope = columns + composite PK のみ、indexes / FK は将来 ADR。
     narrow first pattern (ADR-0026/0027 継承)。FK を初回から含める
     案あり得るが、hallucination pattern 実データを見てから追加方針。
  3. UI 部分失敗挙動 = M 個中 N 個 describe 失敗時 = warning banner
     "N tables could not be described" + 残りで Suggest 続行、全失敗
     のみ block。cancel token 中断は open question として明記済。
- **確認方法**: `docs/decisions.md` の ADR-0028 セクション (末尾) +
  `.claude/issues/0011-ddl-extraction.md`。
- **合意が取れたら**: 私が slice (a) = `dbboard-core` 拡張から順に
  積み上げる (ADR-0026/0027 の 4-slice-single-branch pattern)。
  push + PR create は全 slice 完了後 = user 側のボールに戻る。
- **論点への修正が入る場合**: ADR-0028 draft を amend (別 commit)。
  slice 着手前に確定させたい。

### 選択肢 1: ADR-0028 slice (a) 着手 (review 通過後)

- Slice (a) スコープ:
  - `crates/dbboard-core/src/schema.rs` に `TableSchema` struct 追加
    (fields: `table: TableInfo`, `columns: Vec<ColumnInfo>`,
    `primary_key: Vec<String>`)。
  - `ColumnInfo` に `ordinal: u32` + `default_value: Option<String>` を
    additive で追加。
  - `crates/dbboard-core/src/adapter.rs` の `DatabaseAdapter` trait に
    `async fn describe_table(&self, table: &TableInfo) ->
    DbResult<TableSchema>` を **default impl 付き** で追加、default は
    `DbError::Capability("describe_table not supported by this
    adapter")` を返す。
  - `Capabilities::has_describe_table: bool` を additive で追加、default
    `false`、JSON round-trip test を追加。
  - unit test: capability flag round-trip / default trait impl の
    Capability error / TableSchema 構築 round-trip / Postgres / Turso / D1
    が既存の default impl を継承していることの smoke test。
  - 検証: fmt / clippy -D warnings / check / test / release build / release test。
- adapter 実装は slice (a) には入らず、slice (b) で 3 adapter まとめて。

### 選択肢 2: 現状 friction 報告

- ADR-0028 の 3 論点を横に置き、実利用で困っていることがあれば
  優先。例:
  > 「AI history の record を history panel に描画してほしい」
    (ADR-0027 out-of-scope で意図的に deferred = rich viewer は
    次 PR で拾える)
  > 「Suggest が column 名を hallucinate する件、v1 scope に FK 入れて
    もっと精度上げてほしい」→ ADR-0028 論点 2 の修正扱い
  > 「Include column details toggle は default ON でよくない？」
    → ADR-0028 Decision 9 の修正扱い
- friction 由来の修正は ADR-0028 amend か、まったく別の feat PR に
  なる (どちらかは内容次第)。

### 選択肢 3: ADR-0028 を一旦棚上げ、別作業

- `feature/ddl-extraction` を `git stash` 相当で寝かせて別 feat/chore
  を並行できる (ローカル branch なので origin へ影響なし)。
- 復帰時は本ファイルと `docs/decisions.md` の ADR-0028 status
  (Proposed) を read するだけで context 復旧可能。

### 選択肢 4: web 側状態の確認

- brief 0008 (`0008-web-history-v2-mirror.md`) は Anchors 埋め済
  (desktop 側 merge commit `768e009` 反映済)。web 側で v:2 schema mirror
  + v:1 back-compat test + `describe.skip` flip (v:1 fixture 分) が
  pending。desktop 側 v:2 fixture handoff は brief §Handoff §3 の通り、
  今後の follow-up で提供予定 (ADR-0028 とは独立、任意タイミング)。
- ADR-0028 は **HTTP contract 変更なし + `history.jsonl` schema 変更
  なし** = web 側に新規ミラー作業は発生しない。brief も不要。
  ADR-0029 (function-calling) も同じ posture (in-process のみ) の見込み
  だが確定は D-2 の ADR 起票時。
- 完了通知が来たら memory ([[dbboard-web-state]]) を更新する。

---

## web 側 (情報のみ・ボールは web 側)

- brief 0008 = v:2 schema mirror が web 側 pending (前セッション末で
  Anchors 埋め済)。desktop 側 v:2 fixture handoff は follow-up。
- ADR-0028 (D-1) / ADR-0029 (D-2) はいずれも **in-process only、web 側
  ミラー不要**。ADR-0025 (Group A) / ADR-0026 (Group B) と同じ posture。
- 上記以外の coordination は現時点で pending なし。

---

## 私単独で進められる作業がない理由 (確認用)

1. ADR-0028 draft の 3 論点は maintainer の設計判断領域 = review
   通過が slice (a) 着手の前提。
2. 論点 (2) の scope 判断 (FK 含める / 含めない) は実利用の hallucination
   pattern 情報が maintainer 側にある。
3. 論点 (3) の UI 挙動 (部分失敗時の block vs 続行) は UX judgment、
   私が単独で決めるべきでない。
4. push は user (CLAUDE.md mandate)。全 slice 完了後の push + PR
   create も同様。

→ 3 論点への OK/NG 返答 → slice 着手が user 側のボール。棚上げして
   別作業を優先するのも menu-not-sequence の範囲で OK。

---

## このファイルのメンテ規約

- セッション終了時、状況が動いた時は **必ず最新化**。
- 「最終更新」「develop tip」「test count」「選択肢」の 4 ブロックは
  毎回見直す。
- 大きな状態は memory ([[project-status-in-use]] など) に書き、ここは
  「user が次の一言を選ぶための短い案内」に留める。
