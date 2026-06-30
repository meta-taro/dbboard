# 次のアクション案内 (user 側ボール常設ファイル)

このファイルは「セッションが切れても、開いた瞬間に user 側のボールがわかる」状態を維持するためのもの。
セッション終了時 / 状況が変わった時に必ず更新する。**日本語可・内部用。**

---

## 最終更新

- 日付: 2026-06-30
- develop tip: `3bb82c4` (**PR #45 merged** =
  `feat(ai): streaming + cooperative cancel + token meter
  (ADR-0026 Phase 4 Stage 2 Group B)` 着地)
- 作業ブランチ: `chore/post-pr45-doc-sync` (本 PR 作業中、未 push / 未 PR)
- 直近ハイライト: **ADR-0026 (Phase 4 Stage 2 Group B = streaming +
  cancel + token meter) `develop` 着地完了。** これで Phase 4 Stage 2 で
  in-process スコープの 2 大 Group (A = ADR-0025 / B = ADR-0026) が両方
  クローズ。残りは Group C (history.jsonl AI 記録 + v:2 schema bump =
  web mirror 必要) と Group D (full DDL + function-calling = in-process
  完結)、いずれも独立 ADR で順不同 = menu-not-sequence。
  - PR #45 6 commits 内訳: `3f16697` ADR draft + `2cb012e` Slice a +
    `e5f49d0` Slice b + `e8f5fd5` Slice c + `fff669c` Slice d + `806b04a`
    docs close-out。
  - ワークスペース test count: 全 crate 緑、`dbboard-ui` のみで 145 件 pass
    (Slice c+d で +22 件の streaming/cancel テスト追加)。
  - ドキュメント反映済 (PR #45 内 `806b04a`): `docs/decisions.md`
    ADR-0026 Accepted (2026-06-30) + 4 slice 着地 commit ID 列挙 /
    `docs/roadmap.md` Phase 4 Stage 2 Group B 項目を完了マーク /
    `README.md` AI セクションに streaming + cancel + token meter 段落
    追加 / deferred リストから streaming 削除 / issue 0009 closed。

## モード

**in-use / continuous-improvement (menu-not-sequence)** — 2026-06-24 以降。
ロードマップ順ではなく実利用の摩擦報告を優先。
Group A / Group B 両方クローズしたので、次は **本 chore PR を捌いた後、
別 Group (C/D) に進むか実利用 friction に切り替えるかの判断段階**。

---

## user 側のボール (= 次に着手する時の選択肢)

### **★ 最優先: 本 chore/post-pr45-doc-sync を push → PR → merge**

- **何**: `chore/post-pr45-doc-sync` (現在 1 commit 程度の予定) を
  origin に push し、`develop` への chore PR を作る。
- **手順 (CLAUDE.md ルール: 私はコミット、push は user)**:
  ```sh
  git push -u origin chore/post-pr45-doc-sync
  gh pr create --base develop \
    --title "chore(status): record PR #45 close-out for ADR-0026 Phase 4 Stage 2 Group B" \
    --body "..."
  ```
  PR body 雛形が欲しければ「post-PR45 doc-sync の PR 文書いて」と一言下さい。
- **過去パターン**: PR #38 (post-PR37) / #40 (post-PR39) / #42 (post-PR41) /
  #44 (post-PR43) と完全同型 = `.claude/*` のみ触る極小 chore PR。

### 選択肢 1: 次の Group に着手 (chore マージ後)

- **Group C** (history.jsonl AI records + v:2 schema bump):
  - web 側 mirror **必要** (history JSON schema 変更のため) =
    cross-repo coordination の手筈 (まず `0NNN-web-history-v2-mirror.md`
    fresh brief を起こす) が前段。
  - desktop 側は `dbboard-core::history::Record` に `Ai { … }` variant 追加、
    `emit_history_fixture` 更新、AiPanel から worker 経由で記録投入。
- **Group D** (full DDL extraction + function-calling):
  - in-process のみ = web 影響なし = 即着手可能。
  - 範囲広め (function-calling は ADR が別途必要そう、まず planner agent に
    投げて分解するのが筋)。
- どちらも ADR-0023 §9 + ADR-0026 Out-of-scope で deferred 済み。

### 選択肢 2: 実利用 friction 報告

- streaming + cancel + token meter を実際に触って気になった点があれば
  Group C / D 着手より優先する余地あり (menu-not-sequence)。
- キックオフの一言例:
  > 「token meter の表示が見づらい、〜〜にしたい」
  > 「Cancel ボタンの位置を変えたい」
  > 「Anthropic key 入れてないと streaming トグル出ないけど、もうちょい
  >  分かりやすくしてほしい」

### 選択肢 3: web 側状態の確認

- PR #29 で渡した fixture 受領後、web 側で `describe.skip` をフリップする
  作業が残っている。完了通知が来たら memory ([[dbboard-web-state]]) を
  更新する。
- もし「web 側どうなってる？」と訊かれたら、まず memory を読み、必要なら
  `gh repo view meta-taro/dbboard-web --json updatedAt` 等で fresh 確認。

---

## web 側 (情報のみ・ボールは web 側)

- PR #29 で渡した fixture 受領後、web 側で `describe.skip` をフリップする
  作業が残っている。完了通知が来たらこちらの memory
  ([[dbboard-web-state]]) を更新する。
- ADR-0026 (Group B) は **web 側ミラー不要** = PR #33 の
  `0007-web-ai-phase6-no-contract-mirror.md` brief で explicit-no-op
  済み。追加 brief 不要。
- 上記以外の coordination は現時点で pending なし。
- **Group C 着手時には新しい explicit-mirror brief が必要** (history
  JSON schema の v:2 bump = HTTP contract 影響)。

---

## 私単独で進められる作業がない理由 (確認用)

1. push は user (CLAUDE.md mandate)。PR 作成も基本は user 主導。
2. 次の Group 着手は user の優先順位選択 (Group C vs Group D vs friction)。
3. friction 報告は user が実利用してこそ出てくる情報 = 私が先回りで
   feature 追加するのは menu-not-sequence の原則に反する。

→ 本 chore PR を push → merge する段取りが user 側のボール。
   その後の選択 (次 Group / friction / web 側コーディネーション) も同様。

---

## このファイルのメンテ規約

- セッション終了時、状況が動いた時は **必ず最新化**。
- 「最終更新」「develop tip」「test count」「選択肢」の 4 ブロックは
  毎回見直す。
- 大きな状態は memory ([[project-status-in-use]] など) に書き、ここは
  「user が次の一言を選ぶための短い案内」に留める。
