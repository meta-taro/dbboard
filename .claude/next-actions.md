# 次のアクション案内 (user 側ボール常設ファイル)

このファイルは「セッションが切れても、開いた瞬間に user 側のボールがわかる」状態を維持するためのもの。
セッション終了時 / 状況が変わった時に必ず更新する。**日本語可・内部用。**

---

## 最終更新

- 日付: 2026-07-01
- develop tip: `768e009` (PR #47 = ADR-0027 Phase 4 Stage 2 Group C =
  AI history.jsonl v:2 が着地)
- 作業ブランチ: `chore/post-pr47-doc-sync` (未 push、1 commit 予定 =
  post-merge doc-sync chore)
- 直近ハイライト: **ADR-0027 (Phase 4 Stage 2 Group C = `history.jsonl`
  への AI 記録 + v:2 schema bump) の 5 commit が PR #47 として merge**、
  Phase 4 Stage 2 で in-process スコープの 3 グループ (A = ADR-0025 /
  B = ADR-0026 / C = ADR-0027) が全て `develop` 着地。Stage 2 残りは
  Group D (full DDL extraction + function-calling = in-process 完結、
  web 影響なし) のみ、独立 ADR で任意タイミング。

## モード

**in-use / continuous-improvement (menu-not-sequence)** — 2026-06-24 以降。
ロードマップ順ではなく実利用の摩擦報告を優先。
Group A / B / C 3 グループ完了で、Stage 2 残りは Group D のみ。
Group D は in-process 完結 = web 影響なし = 即着手可能だが範囲広め。
menu-not-sequence の原則通り、friction 報告があればそちらを優先。

---

## user 側のボール (= 次に着手する時の選択肢)

### **★ 最優先: `chore/post-pr47-doc-sync` を push → PR create → merge**

- **何**: `chore/post-pr47-doc-sync` (現在ローカル 1 commit) を origin に
  push し、`develop` への chore PR を作る。
- **手順 (CLAUDE.md ルール: 私はコミット、push は user)**:
  ```sh
  git push -u origin chore/post-pr47-doc-sync
  gh pr create --base develop \
    --title "chore(status): record PR #47 close-out for ADR-0027 Phase 4 Stage 2 Group C" \
    --body "..."
  ```
  PR body 雛形が欲しければ「post-PR47 chore の PR 文書いて」と一言下さい。
- **過去パターン**: PR #38 (post-PR37) / #40 (post-PR39) / #42 (post-PR41) /
  #44 (post-PR43) / #46 (post-PR45) と完全同型 = `.claude/*` のみ + 
  ADR slice (d) placeholder フィルイン + brief 0008 Anchors フィルインを
  触る極小 chore PR。

### 選択肢 1: 次の Group に着手 (chore マージ後)

- **Group D** (full DDL extraction + function-calling):
  - in-process のみ = web 影響なし = 即着手可能。
  - 範囲広め (function-calling は ADR が別途必要そう、まず planner agent
    に投げて分解するのが筋)。
  - ADR-0023 §9 + ADR-0026 Out-of-scope + ADR-0027 Out-of-scope で
    deferred 済み。
- Group A / B / C は全て閉。

### 選択肢 2: 実利用 friction 報告

- history.jsonl の AI 記録を実際に触って気になった点があれば Group D
  着手より優先する余地あり (menu-not-sequence)。
- キックオフの一言例:
  > 「AI history の record を history panel に描画してほしい」
    (ADR-0027 out-of-scope で意図的に deferred = rich viewer は
    次 PR で拾える)
  > 「prompt が長すぎて 64 KiB truncate に頻繁にひっかかる、cap 引き上げたい」
  > 「verbatim だと debug 情報を貼り込むと sensitive な内容が残るのが気になる」

### 選択肢 3: web 側状態の確認

- PR #29 で渡した v:1 fixture 受領後、web 側で `describe.skip` を
  フリップする作業が残っている (v:1 mirror = brief 0003 分)。
- ADR-0027 で v:2 が出たので web 側は追加でミラー作業が発生する
  (brief 0008 で正式に発注済 = ボールは web 側)。desktop 側の
  v:2 fixture handoff は本 chore PR マージ後、または後続 PR で
  `cargo run --example emit_history_fixture --output PATH` で
  提供予定 (brief 0008 §Handoff procedure 参照)。
- 完了通知が来たら memory ([[dbboard-web-state]]) を更新する。
- もし「web 側どうなってる？」と訊かれたら、まず memory を読み、必要なら
  `gh repo view meta-taro/dbboard-web --json updatedAt` 等で fresh 確認。

---

## web 側 (情報のみ・ボールは web 側)

- **brief 0008 (`0008-web-history-v2-mirror.md`) は Anchors 埋め済**
  (desktop 側 merge commit `768e009` を反映)。web 側で v:2 schema の
  per-record shape mirror + v:1 back-compat test + `describe.skip` flip
  (v:1 fixture 分) が pending。v:2 fixture handoff は追って提供予定
  (brief 0008 §Handoff procedure §3)。
- ADR-0025 (Group A) / ADR-0026 (Group B) は **web 側ミラー不要** = PR #33
  の `0006` / `0007` brief で explicit-no-op 済み。追加 brief 不要。
- 上記以外の coordination は現時点で pending なし。

---

## 私単独で進められる作業がない理由 (確認用)

1. push は user (CLAUDE.md mandate)。PR 作成も基本は user 主導。
2. 次の Group 着手は user の優先順位選択 (Group D vs friction)。
3. friction 報告は user が実利用してこそ出てくる情報 = 私が先回りで
   feature 追加するのは menu-not-sequence の原則に反する。

→ 本 chore commit を push → PR create → merge する段取りが user 側の
   ボール。その後の選択 (Group D / friction / web 側コーディネーション)
   も同様。

---

## このファイルのメンテ規約

- セッション終了時、状況が動いた時は **必ず最新化**。
- 「最終更新」「develop tip」「test count」「選択肢」の 4 ブロックは
  毎回見直す。
- 大きな状態は memory ([[project-status-in-use]] など) に書き、ここは
  「user が次の一言を選ぶための短い案内」に留める。
