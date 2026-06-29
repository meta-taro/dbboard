# 次のアクション案内 (user 側ボール常設ファイル)

このファイルは「セッションが切れても、開いた瞬間に user 側のボールがわかる」状態を維持するためのもの。
セッション終了時 / 状況が変わった時に必ず更新する。**日本語可・内部用。**

---

## 最終更新

- 日付: 2026-06-29
- develop tip: `8a08f67` (PR #44 merged = post-PR43 doc sync 着地)
- 作業ブランチ: `feature/ai-streaming-cancel-tokens` (Group B 着手中)
- 直近ハイライト: **ADR-0026 (Phase 4 Stage 2 Group B = streaming + cancel +
  token meter) ドラフト完了 / user レビュー待ち。** ADR 本文は
  `docs/decisions.md`、実装トラッカは `.claude/issues/0009-ai-streaming-cancel-tokens.md`。
  11 個の Decision を整理済み (additive trait 拡張・drop-the-stream cancel・
  cumulative token 表示・streaming opt-in toggle 他)。
- ワークスペース test count: 474 件 pass (Group A 完了時点、Group B はまだ
  test 書く前)
- ローカルブランチ状態: docs 3 ファイル変更済み (`docs/decisions.md` +
  `.claude/issues/0009-*.md` + 当ファイル)、commit 前

## モード

**in-use / continuous-improvement (menu-not-sequence)** — 2026-06-24 以降。
ロードマップ順ではなく実利用の摩擦報告を優先。
ただし現在は **Group B 着手中** (user が選択肢 1 を選んだため)、
ADR-0026 user 合意 → Slice a 実装に進む段階。

---

## user 側のボール (= 次に着手する時の選択肢)

### **★ 最優先: ADR-0026 をレビュー**

- **何**: `docs/decisions.md` 末尾の ADR-0026 (Phase 4 Stage 2 Group B =
  streaming + cancel + token meter) をレビューして OK / 修正指示を出す。
- **特に確認してほしい設計判断** (Decision 番号):
  - Decision 1: **additive trait 拡張** (`stream_explain` / `stream_suggest_sql`
    を別 method として追加、既存 `explain` / `suggest_sql` は無変更)
  - Decision 5: **cancel は drop-the-stream** (trait に
    `CancellationToken` 引数を取らない、`reqwest` の h2 close で代用)
  - Decision 7: **token meter は cumulative read** (delta を sum せず、
    Anthropic の `message_delta.usage.output_tokens` が cumulative なので
    上書き)
  - Decision 9: **streaming は UI の opt-in toggle** (デフォルトは atomic、
    `has_streaming` capability で gate)
  - Decision 11: **mid-stream provider swap は cancel しない** (ADR-0025
    の slot snapshot 挙動を継承、cancel は user 明示操作のみ)
- **キックオフの一言例**:
  > 「ADR-0026 OK、Slice a の RED tests から進めて」
  > 「Decision 9 を逆にしてほしい (streaming をデフォルトに)」
  > 「Decision 5 はやはり CancellationToken を渡したい」

### 選択肢 1: Slice a から実装着手 (ADR-0026 合意後)

- ADR-0026 OK 後、`dbboard-ai` trait 拡張 + `StreamEvent` / `StopReason` 型
  + default delegate impl を RED tests first で書く。
- 規模感: 小〜中。新しい trait method 2 個と型 2 個追加、テスト 10 件くらい。
- HTTP contract 無変更、cross-repo coordination 不要。

### 選択肢 2: 別 Group に切り替え (Group B を保留)

- Group C (history.jsonl + v:2 + web brief) や Group D (DDL + function-calling)
  に方針転換する場合。
- ADR-0026 と issue 0009 は draft のまま残しても良い (Status: Proposed)。
- branch `feature/ai-streaming-cancel-tokens` は破棄 or 保留。

### 選択肢 3: 実利用の摩擦報告

- Group A クローズ後、AI Settings UI を実利用していて気になった点があれば
  Group B 着手より優先する余地あり (menu-not-sequence の原則)。
- キックオフの一言例:
  > 「Group B 中断、AI Providers 画面の〜〜が辛い」

---

## web 側 (情報のみ・ボールは web 側)

- PR #29 で渡した fixture 受領後、web 側で `describe.skip` をフリップする
  作業が残っている。完了通知が来たらこちらの memory
  ([[dbboard-web-state]]) を更新する。
- ADR-0026 (Group B) は **web 側ミラー不要**。理由: ADR-0023 Decision 3 が
  AI を HTTP contract から外しているので、streaming も in-process のみ。
  PR #33 の `0007-web-ai-phase6-no-contract-mirror.md` で既に explicit-no-op
  brief 済み = 追加 brief 不要。
- 上記以外の coordination は現時点で pending なし。

---

## 私単独で進められる作業がない理由 (確認用)

1. ADR-0026 は Proposed 状態 = user の OK が出るまで実装着手しない方が筋。
2. 設計判断 (特に Decision 5 = cancel design、Decision 9 = streaming opt-in
   policy) は user の好みで分かれる可能性あり = 勝手に進めて後で書き直しは
   過剰。
3. Group A / 過去 ADR でも「ADR draft → user review → 合意 → 実装」の順を
   踏襲してきた (ADR-0023 / 0024 / 0025 すべて) = 同じパターンを維持。

→ ADR-0026 を読んで OK / 修正指示を出してもらえれば、Slice a から即着手可能。

---

## このファイルのメンテ規約

- セッション終了時、状況が動いた時は **必ず最新化**。
- 「最終更新」「develop tip」「test count」「選択肢」の 4 ブロックは
  毎回見直す。
- 大きな状態は memory ([[project-status-in-use]] など) に書き、ここは
  「user が次の一言を選ぶための短い案内」に留める。
