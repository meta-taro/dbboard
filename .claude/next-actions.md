# 次のアクション案内 (user 側ボール常設ファイル)

このファイルは「セッションが切れても、開いた瞬間に user 側のボールがわかる」状態を維持するためのもの。
セッション終了時 / 状況が変わった時に必ず更新する。**日本語可・内部用。**

---

## 最終更新

- 日付: 2026-07-10
- develop tip: `c343b8f` (**PR #50 = post-pr49 doc-sync merged →
  ADR-0028 完全クローズ**)
- 作業ブランチ: `feature/query-ux` (develop `c343b8f` から分岐、
  4 commit 積み上げ済、**push + feat PR create 待ち**)
- 直近ハイライト: **query-UX 摩擦バッチ完了 (2026-07-10)。** 実利用で
  出た 4 件の UI 摩擦を `feature/query-ux` に実装。全 commit で
  pre-commit フック完走、全テスト green。
  - `76f7520` run trigger (F5 / Ctrl+Enter / 右クリック)
  - `874ab8e` result grid 刷新 = egui_extras TableBuilder / sticky
    header / 縦罫線 / 仮想化 / 長文セル popup (**ADR-0030**)
  - `2a1d446` 裸 SELECT の auto-LIMIT 100 ガード (**ADR-0030**)
  - `8ccc1f6` structure タブ = describe_table 経由の列情報表示
    (**ADR-0031**)
  - 新規 i18n キーは全 11 locale 伝播済。HTTP contract 不変 =
    web ミラー不要。

## モード

**in-use / continuous-improvement (menu-not-sequence)** — 2026-06-24 以降。
ロードマップ順ではなく実利用の摩擦報告を優先。今回の query-UX バッチが
その典型 (roadmap 順ではなく実利用摩擦を直接処理)。Stage 2 Group
A/B/C/D-1 完了、残りは D-2 (ADR-0029 = function-calling) のみで
`feature/adr-0029-function-calling` に planning ball あり (別ストリーム)。

---

## user 側のボール (= 次に着手する時の選択肢)

### **★ 最優先: `feature/query-ux` の push + feat PR 作成**

- **何**: query-UX ブランチを push し、develop に対して feat PR を
  1 本立てる。
  ```
  git push -u origin feature/query-ux
  ```
  中身: 上記 4 commit (run trigger / grid 刷新 / auto-LIMIT /
  structure タブ) + ADR-0030 / ADR-0031 + 11 locale i18n。コード +
  ADR + user-facing docs を含む feat PR (doc-split パターンなら
  roadmap tick は post-merge chore に送る)。希望があれば PR 本文
  ドラフトを用意する。
- **push 前チェック (human)**: pre-push (`cargo build --release` /
  `cargo test --all-features --release`) は cargo-husky が自動実行。

### 選択肢 1: ADR-0029 (Group D-2 = function-calling) draft 着手

- `feature/adr-0029-function-calling` ブランチに draft ball あり。
- D-1 の `describe_table` primitive が landed = D-2 の前提が揃った。
- 内容: AI provider 側に tool-use surface を追加し、`describe_table`
  を最初の callable tool として expose。Anthropic Messages API の
  `tools` パラメータ + `tool_use`/`tool_result` content block 対応。
- in-process only の見込み (HTTP contract 変更なし)。

### 選択肢 2: 現状 friction 報告

- 実利用で困っていることがあれば優先。今回 4 件処理したので新規の
  摩擦があれば挙げてほしい。既知の deferred 候補:
  > 「AI history record を history panel に描画してほしい」
    (ADR-0027 で意図的に deferred = rich viewer)
  > 「Include column details / auto-LIMIT を session 跨ぎで記憶」
    → `ui-preferences.toml` 系の小 ADR で拾える
  > 「大規模 schema で prompt が重い」→ ADR-0028 open question
    (prompt-size cap) の再訪

### 選択肢 3: web 側状態の確認

- brief 0008 (`0008-web-history-v2-mirror.md`) = v:2 schema mirror が
  web 側 pending。desktop 側 v:2 fixture handoff は follow-up のまま。
- ADR-0030/0031 (query-UX) は **HTTP contract 変更なし + `history.jsonl`
  変更なし** = web ミラー不要、brief 不要。
- 完了通知が来たら memory ([[dbboard-web-state]]) を更新する。

---

## web 側 (情報のみ・ボールは web 側)

- brief 0008 = v:2 schema mirror が web 側 pending。
- ADR-0030/0031 (query-UX) は in-process only、web ミラー不要 (確定)。
  ADR-0029 (D-2) も同 posture の見込み、確定は起票時。
- 上記以外の coordination は現時点で pending なし。

---

## 私単独で進められる作業がない理由 (確認用)

1. push + PR create は user (CLAUDE.md mandate)。query-UX 4 commit は
   commit 済なので、これが唯一のブロッカー。
2. ADR-0029 draft は着手可能 (`feature/adr-0029-function-calling`)。
   query-UX の push と独立なので並行可。
3. friction 報告は実利用者 = maintainer からしか来ない。

→ **`feature/query-ux` の push + feat PR 作成が user 側のボール**。
   並行で ADR-0029 draft 指示 / 新規 friction 報告も
   menu-not-sequence の範囲で OK。

---

## このファイルのメンテ規約

- セッション終了時、状況が動いた時は **必ず最新化**。
- 「最終更新」「develop tip」「test count」「選択肢」の 4 ブロックは
  毎回見直す。
- 大きな状態は memory ([[project-status-in-use]] など) に書き、ここは
  「user が次の一言を選ぶための短い案内」に留める。
