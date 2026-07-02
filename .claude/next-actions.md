# 次のアクション案内 (user 側ボール常設ファイル)

このファイルは「セッションが切れても、開いた瞬間に user 側のボールがわかる」状態を維持するためのもの。
セッション終了時 / 状況が変わった時に必ず更新する。**日本語可・内部用。**

---

## 最終更新

- 日付: 2026-07-02
- develop tip: `5cc01e3` (PR #48 = post-PR47 chore doc-sync merged)
- 作業ブランチ: `feature/ddl-extraction` (**未 push、全 slice 完了**)
- 直近ハイライト: **ADR-0028 (Phase 4 Stage 2 Group D-1 = full DDL
  extraction) 全 4 slice 完了、ADR Accepted (2026-07-02)**。
  - slice (a) `a42a27c` + review-fix `bba4072` = core 拡張
    (`describe_table` trait method / `TableSchema` /
    `ColumnInfo.ordinal`+`default_value` / `has_describe_table`)
  - slice (b) `b509a36` = turso / d1 / postgres の 3 adapter 実装
  - slice (c) `dfdaaca` = `SuggestRequest.full_schema` + Anthropic
    prompt rendering + worker `PrefetchSchema` fan-out (Semaphore 8) +
    `AiPanel`「Include column details」checkbox + 部分失敗 warning
    banner + 11 locale i18n。計画からの逸脱 1 点: `apps/dbboard` に
    `DesktopSchemaSource` (新 narrow trait `SchemaSource` の impl) を
    配線 (ADR status block に記録済)。
  - slice (d) = docs sweep (ADR Accepted 化 / README AI 節 /
    issue 0011 close / 本ファイル + project-status)
- 検証: fmt / clippy -D warnings / check / test 全グリーン
  (pre-commit hook でも再検証済)。

## モード

**in-use / continuous-improvement (menu-not-sequence)** — 2026-06-24 以降。
ロードマップ順ではなく実利用の摩擦報告を優先。
Group A / B / C / **D-1** 完了。残りは D-2 (ADR-0029 =
function-calling / tool-use、`describe_table` を最初の tool として
expose) のみ。

---

## user 側のボール (= 次に着手する時の選択肢)

### **★ 最優先: `feature/ddl-extraction` の push + PR 作成**

- **何**: 全 slice 完了済みのブランチを push し、develop に対して
  feat PR を 1 本立てる (PR #45/#47 パターン)。
  ```
  git push -u origin feature/ddl-extraction
  ```
  PR 本文は commit 5 本 (`00ac1b8` ADR draft → `a42a27c` → `bba4072`
  → `b509a36` → `dfdaaca` → docs sweep commit) の積み上げを要約。
  希望があれば私が PR 本文ドラフトを用意する。
- **動作確認の観点** (Pre-Push Checklist):
  1. AI provider 設定済みの状態で Suggest モードを開くと
     「Include column details」checkbox が出る (describe 可能な
     adapter = 3 種すべて)。
  2. checkbox ON で Send → 「Fetching table schemas…」表示 →
     Suggest が実 column 名を使った SQL を返す。
  3. 存在しない/権限のない table が混じると黄色 warning
     「Could not describe N table(s)…」が出るが Suggest は続行。
  4. token meter が names-only 時より増える (= full schema が
     prompt に載っている証拠)。
- **merge 後**: 私が `chore/post-prNN-doc-sync` を作る
  (roadmap Group D-1 tick + project-status + memory 更新。
  README/ADR は feat PR 側に同梱済 = PR #47/#48 doc-split パターン)。

### 選択肢 1: ADR-0029 (Group D-2 = function-calling) draft 着手

- D-1 の `describe_table` primitive が landed = D-2 の前提が揃った。
- 内容: AI provider 側に tool-use surface を追加し、`describe_table`
  を最初の callable tool として expose。Anthropic Messages API の
  `tools` パラメータ + `tool_use`/`tool_result` content block 対応。
- in-process only の見込み (HTTP contract 変更なし) だが、確定は
  ADR 起票時に判断。
- feat PR (D-1) の merge を待たずに draft だけ先行することも可能。

### 選択肢 2: 現状 friction 報告

- 実利用で困っていることがあれば優先。例:
  > 「AI history record を history panel に描画してほしい」
    (ADR-0027 で意図的に deferred = rich viewer)
  > 「Include column details を session 跨ぎで記憶してほしい」
    → ADR-0028 out-of-scope 明記済、`ui-preferences.toml` 系の
    小 ADR で拾える
  > 「大規模 schema で prompt が重い」→ ADR-0028 open question
    (prompt-size cap) の再訪 = 実測 friction が来たら着手の約束
- schema browser UI (tables → columns tree) も `describe_table` の
  自然な consumer として今なら安く作れる。

### 選択肢 3: web 側状態の確認

- brief 0008 (`0008-web-history-v2-mirror.md`) = v:2 schema mirror が
  web 側 pending。desktop 側 v:2 fixture handoff は follow-up のまま。
- ADR-0028 は **HTTP contract 変更なし + `history.jsonl` 変更なし** =
  web ミラー不要、brief 不要 (ADR 冒頭に明記済)。
- 完了通知が来たら memory ([[dbboard-web-state]]) を更新する。

---

## web 側 (情報のみ・ボールは web 側)

- brief 0008 = v:2 schema mirror が web 側 pending。
- ADR-0028 (D-1) は in-process only、web ミラー不要 (確定)。
  ADR-0029 (D-2) も同 posture の見込み、確定は起票時。
- 上記以外の coordination は現時点で pending なし。

---

## 私単独で進められる作業がない理由 (確認用)

1. push + PR create は user (CLAUDE.md mandate)。全 slice 完了済み
   なので、これが唯一のブロッカー。
2. ADR-0029 draft は着手可能だが、D-1 の PR review で設計 feedback が
   入る可能性を考えると merge 待ちが安全 (先行 draft は user 判断)。
3. friction 報告は実利用者 = maintainer からしか来ない。

→ **push + PR 作成が user 側のボール**。並行で ADR-0029 draft 指示 /
   friction 報告も menu-not-sequence の範囲で OK。

---

## このファイルのメンテ規約

- セッション終了時、状況が動いた時は **必ず最新化**。
- 「最終更新」「develop tip」「test count」「選択肢」の 4 ブロックは
  毎回見直す。
- 大きな状態は memory ([[project-status-in-use]] など) に書き、ここは
  「user が次の一言を選ぶための短い案内」に留める。
