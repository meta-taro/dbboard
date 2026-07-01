# 次のアクション案内 (user 側ボール常設ファイル)

このファイルは「セッションが切れても、開いた瞬間に user 側のボールがわかる」状態を維持するためのもの。
セッション終了時 / 状況が変わった時に必ず更新する。**日本語可・内部用。**

---

## 最終更新

- 日付: 2026-07-01
- develop tip: `8082706` (PR #46 = post-PR45 chore doc-sync merged)
- 作業ブランチ: `feature/ai-history-v2` (未 push、5 commit 予定 =
  `958c117` ADR-0027 draft + `b16537f` Slice a + `13f7736` Slice b +
  `0e76223` Slice c + 本 slice d docs commit)
- 直近ハイライト: **ADR-0027 (Phase 4 Stage 2 Group C = `history.jsonl`
  への AI 記録 + v:2 schema bump) をローカル 4 slice で実装完了。**
  ADR-0025 (Group A) / ADR-0026 (Group B) に続き、Group C も
  in-process スコープ完結。残り Stage 2 は Group D (full DDL +
  function-calling) のみ、独立 ADR で任意タイミング。
  - Slice a (`b16537f`): `dbboard-ui::history` を v:2 reader + writer に
    アップグレード。`RecordWire` flat 化 + `kind: "query" \| "ai"`
    discriminator、`HistoryEntry::{Query, Ai}` split、64 KiB
    truncation、v:1 record は `Query` として transparent 読み出し、
    unknown `kind` / `intent` は drop + counter tick。
    `examples/emit_history_fixture` は 10 query + 1 AI (計 11 line)
    を v:2 で emit するように更新、`fixture_output_matches_brief_conventions`
    test で pin。
  - Slice b (`13f7736`): `dbboard-ai::AiProvider::identity()`
    additive 追加、`AiResponse { provider, model }` フィールド追加、
    `dbboard-anthropic` 実装、`dbboard-ui::worker` の 4 terminal AI
    reply variants が spawn-time identity snapshot を stamp。
  - Slice c (`0e76223`): `dbboard-ui::lib` に `PendingAiSubmit`
    snapshot + 4 terminal reply arm の `HistoryEntry::Ai { … }` 組立
    + `record_ai_history` で `PersistentHistoryStore` 追記。streaming/
    cancelled は panel drain 前に peek、cancel token bookkeeping、
    0-token accumulator の tokens `None` semantics (ADR-0027 Decision 5)
    遵守。18 新規 unit test。
  - Slice d (本 commit): ADR-0027 status Proposed → **Accepted (2026-07-01)**
    に flip + roadmap Group C tick + README verbatim-logging 警告段落追加
    + issue 0010 status open → closed + brief 0008 Anchors 更新 +
    project-status + 本 next-actions 更新。

## モード

**in-use / continuous-improvement (menu-not-sequence)** — 2026-06-24 以降。
ロードマップ順ではなく実利用の摩擦報告を優先。
Group A / B / C 3 グループ完了で、Stage 2 残りは Group D のみ。
Group D は in-process 完結 = web 影響なし = 即着手可能だが範囲広め。
menu-not-sequence の原則通り、friction 報告があればそちらを優先。

---

## user 側のボール (= 次に着手する時の選択肢)

### **★ 最優先: `feature/ai-history-v2` を push → PR create → merge**

- **何**: `feature/ai-history-v2` (現在ローカル 5 commit) を origin に
  push し、`develop` への feat PR を作る。
- **手順 (CLAUDE.md ルール: 私はコミット、push は user)**:
  ```sh
  git push -u origin feature/ai-history-v2
  gh pr create --base develop \
    --title "feat(history): AI calls recorded in history.jsonl v:2 (ADR-0027 Phase 4 Stage 2 Group C)" \
    --body "..."
  ```
  PR body 雛形が欲しければ「ADR-0027 の PR 文書いて」と一言下さい。
- **過去パターン**: PR #45 (ADR-0026 Group B) と同型 = 4 slice + docs
  close-out を 1 feat PR にまとめる、post-merge に短い chore PR で
  `.claude/*` を最新化する 2 段構え。

### 選択肢 1: post-merge doc-sync chore (Group C マージ後)

- ADR-0027 status に slice d 自身の commit hash を埋める
  (ADR-0026 slice d `fff669c` 埋め込みの precedent)。
- brief 0008 Anchors の "desktop merge commit ID" を実 merge hash に置換。
- `.claude/project-status.md` に PR #NN merged tip を反映。
- 過去パターン: PR #38 (post-PR37) / #40 (post-PR39) / #42 (post-PR41) /
  #44 (post-PR43) / #46 (post-PR45) と完全同型 = `.claude/*` のみ触る極小 chore PR。

### 選択肢 2: 次の Group に着手 (chore マージ後)

- **Group D** (full DDL extraction + function-calling):
  - in-process のみ = web 影響なし = 即着手可能。
  - 範囲広め (function-calling は ADR が別途必要そう、まず planner agent
    に投げて分解するのが筋)。
  - ADR-0023 §9 + ADR-0026 Out-of-scope + ADR-0027 Out-of-scope で
    deferred 済み。
- Group A / B / C は全て閉。

### 選択肢 3: 実利用 friction 報告

- history.jsonl の AI 記録を実際に触って気になった点があれば Group D
  着手より優先する余地あり (menu-not-sequence)。
- キックオフの一言例:
  > 「AI history の record を history panel に描画してほしい」
    (ADR-0027 out-of-scope で意図的に deferred = rich viewer は
    次 PR で拾える)
  > 「prompt が長すぎて 64 KiB truncate に頻繁にひっかかる、cap 引き上げたい」
  > 「verbatim だと debug 情報を貼り込むと sensitive な内容が残るのが気になる」

### 選択肢 4: web 側状態の確認

- PR #29 で渡した v:1 fixture 受領後、web 側で `describe.skip` を
  フリップする作業が残っている (v:1 mirror = brief 0003 分)。
- ADR-0027 で v:2 が出たので web 側は追加でミラー作業が発生する
  (brief 0008 で正式に発注済 = ボールは web 側)。
- 完了通知が来たら memory ([[dbboard-web-state]]) を更新する。
- もし「web 側どうなってる？」と訊かれたら、まず memory を読み、必要なら
  `gh repo view meta-taro/dbboard-web --json updatedAt` 等で fresh 確認。

---

## web 側 (情報のみ・ボールは web 側)

- **NEW: brief 0008 (`0008-web-history-v2-mirror.md`) が正式発注済**。
  web 側で v:2 schema の per-record shape mirror + v:1 back-compat
  test + `describe.skip` flip (v:1 fixture 分) が pending。desktop 側
  merge 後の fixture handoff (v:2) は post-merge chore PR で発注予定。
- ADR-0025 (Group A) / ADR-0026 (Group B) は **web 側ミラー不要** = PR #33
  の `0006` / `0007` brief で explicit-no-op 済み。追加 brief 不要。
- 上記以外の coordination は現時点で pending なし。

---

## 私単独で進められる作業がない理由 (確認用)

1. push は user (CLAUDE.md mandate)。PR 作成も基本は user 主導。
2. 次の Group 着手は user の優先順位選択 (Group D vs friction)。
3. friction 報告は user が実利用してこそ出てくる情報 = 私が先回りで
   feature 追加するのは menu-not-sequence の原則に反する。

→ 本 slice d commit を含む 5 commit を push → PR create → merge する
   段取りが user 側のボール。その後の選択 (post-merge chore / Group D
   / friction / web 側コーディネーション) も同様。

---

## このファイルのメンテ規約

- セッション終了時、状況が動いた時は **必ず最新化**。
- 「最終更新」「develop tip」「test count」「選択肢」の 4 ブロックは
  毎回見直す。
- 大きな状態は memory ([[project-status-in-use]] など) に書き、ここは
  「user が次の一言を選ぶための短い案内」に留める。
