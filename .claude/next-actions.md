# 次のアクション案内 (user 側ボール常設ファイル)

このファイルは「セッションが切れても、開いた瞬間に user 側のボールがわかる」状態を維持するためのもの。
セッション終了時 / 状況が変わった時に必ず更新する。**日本語可・内部用。**

---

## 最終更新

- 日付: 2026-07-03
- develop tip: `6c34ee3` (**PR #49 = ADR-0028 full DDL extraction merged**)
- 作業ブランチ: `chore/post-pr49-doc-sync` (doc-sync chore、push + PR 待ち)
- 直近ハイライト: **ADR-0028 (Phase 4 Stage 2 Group D-1 = full DDL
  extraction) が PR #49 で develop に merge 済 (merge `6c34ee3`,
  2026-07-03)。** slice (a) `a42a27c`+`bba4072` / (b) `b509a36` /
  (c) `dfdaaca` / (d) `3c3e3d8`。ADR-0028 Accepted。
  - 計画からの逸脱 1 点: `apps/dbboard` に `DesktopSchemaSource`
    (新 narrow trait `SchemaSource` の impl) を配線 (ADR に記録済)。
  - HTTP contract / `history.jsonl` 不変 = web ミラー不要。
- post-merge doc-sync (`chore/post-pr49-doc-sync`): roadmap の
  Group D-1 tick + Stage 2 まとめ段落更新 + line 258 の
  「full DDL extraction deferred」を「shipped as D-1」に訂正 +
  project-status + next-actions + memory。**push + chore PR create が
  user 側のボール。**

## モード

**in-use / continuous-improvement (menu-not-sequence)** — 2026-06-24 以降。
ロードマップ順ではなく実利用の摩擦報告を優先。
Group A / B / C / **D-1 完了**。残りは D-2 (ADR-0029 =
function-calling / tool-use、`describe_table` を最初の tool として
expose) のみ。

---

## user 側のボール (= 次に着手する時の選択肢)

### **★ 最優先: `chore/post-pr49-doc-sync` の push + PR 作成**

- **何**: doc-sync chore ブランチを push し、develop に対して
  chore PR を 1 本立てる (PR #48 パターン)。
  ```
  git push -u origin chore/post-pr49-doc-sync
  ```
  中身は roadmap の Group D-1 tick + project-status/next-actions/memory
  整合のみ (コード変更なし)。希望があれば私が PR 本文ドラフトを用意する。
- **merge 後**: この chore が入れば ADR-0028 は完全クローズ。

### 選択肢 1: ADR-0029 (Group D-2 = function-calling) draft 着手

- D-1 の `describe_table` primitive が landed = D-2 の前提が揃った。
- 内容: AI provider 側に tool-use surface を追加し、`describe_table`
  を最初の callable tool として expose。Anthropic Messages API の
  `tools` パラメータ + `tool_use`/`tool_result` content block 対応。
- in-process only の見込み (HTTP contract 変更なし) だが、確定は
  ADR 起票時に判断。
- doc-sync chore の merge を待たずに draft だけ先行することも可能。

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
- ADR-0028 (D-1) は **HTTP contract 変更なし + `history.jsonl` 変更なし** =
  web ミラー不要、brief 不要。
- 完了通知が来たら memory ([[dbboard-web-state]]) を更新する。

---

## web 側 (情報のみ・ボールは web 側)

- brief 0008 = v:2 schema mirror が web 側 pending。
- ADR-0028 (D-1) は in-process only、web ミラー不要 (確定)。
  ADR-0029 (D-2) も同 posture の見込み、確定は起票時。
- 上記以外の coordination は現時点で pending なし。

---

## 私単独で進められる作業がない理由 (確認用)

1. push + PR create は user (CLAUDE.md mandate)。doc-sync chore は
   commit 済なので、これが唯一のブロッカー。
2. ADR-0029 draft は着手可能。doc-sync chore と独立なので並行可。
3. friction 報告は実利用者 = maintainer からしか来ない。

→ **doc-sync chore の push + PR 作成が user 側のボール**。並行で
   ADR-0029 draft 指示 / friction 報告も menu-not-sequence の範囲で OK。

---

## このファイルのメンテ規約

- セッション終了時、状況が動いた時は **必ず最新化**。
- 「最終更新」「develop tip」「test count」「選択肢」の 4 ブロックは
  毎回見直す。
- 大きな状態は memory ([[project-status-in-use]] など) に書き、ここは
  「user が次の一言を選ぶための短い案内」に留める。
