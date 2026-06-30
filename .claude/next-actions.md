# 次のアクション案内 (user 側ボール常設ファイル)

このファイルは「セッションが切れても、開いた瞬間に user 側のボールがわかる」状態を維持するためのもの。
セッション終了時 / 状況が変わった時に必ず更新する。**日本語可・内部用。**

---

## 最終更新

- 日付: 2026-06-30
- develop tip: `6e6eb83` (PR #42 merged = post-PR41 doc sync 着地後)
- 作業ブランチ: `feature/ai-streaming-cancel-tokens`
  (Phase 4 Stage 2 Group B = ADR-0026 / 4 slice 完了、未 push / 未 PR)
- 直近ハイライト: **ADR-0026 (Phase 4 Stage 2 Group B = streaming +
  cancel + token meter) 実装完了 / status を Accepted に切替 / push 待ち。**
  4 commits 全部緑 (fmt / clippy / check / test pre-commit hook OK)。
  - `3f16697` docs(adr): ADR-0026 draft
  - `2cb012e` feat(ai) Slice a: dbboard-ai trait 拡張
    (`stream_explain` / `stream_suggest_sql` + `StreamEvent` / `StopReason`
    + `AiCapabilities::has_streaming` activate)
  - `e5f49d0` feat(ai) Slice b: Anthropic SSE in dbboard-anthropic
    (`reqwest-eventsource` 0.6 + `RetryPolicy::Never`)
  - `e8f5fd5` feat(ai) Slice c: dbboard-ui worker 改造
    (tokio async loop + std→tokio mpsc bridge + per-request
    `CancellationToken` + `tokio::select!` cancel race)
  - `fff669c` feat(ai) Slice d: `AiPanel` state machine
    (`StreamingAcc` + Send↔Cancel toggle + token meter +
    3 Fluent keys × 11 locales)
- ワークスペース test count: 全 crate 緑、`dbboard-ui` のみで 145 件 pass
  (Slice c+d で +20 件相当の streaming/cancel テスト追加)
- ローカルブランチ状態: 5 commits ahead of `origin/feature/ai-streaming-cancel-tokens`
  (Slice d + doc sweep ぶん、まだ push していない)
- ドキュメント反映: `docs/decisions.md` ADR-0026 Accepted (2026-06-30) +
  4 slice 着地 commit ID 列挙 / `docs/roadmap.md` Phase 4 Stage 2 Group B
  項目を完了マーク / `README.md` AI セクションに streaming + cancel +
  token meter 段落追加 / deferred リストから streaming 削除

## モード

**in-use / continuous-improvement (menu-not-sequence)** — 2026-06-24 以降。
ロードマップ順ではなく実利用の摩擦報告を優先。
Group B が closed したので、次は **user が push + PR 作成** → merge 後に
別 Group に進むか実利用 friction に切り替えるかの判断段階。

---

## user 側のボール (= 次に着手する時の選択肢)

### **★ 最優先: ブランチを push して PR を作成**

- **何**: `feature/ai-streaming-cancel-tokens` (5 commits 含む) を
  origin に push し、`develop` への PR を作る。
- **コミット範囲**: `3f16697` ADR draft → `2cb012e` Slice a → `e5f49d0`
  Slice b → `e8f5fd5` Slice c → `fff669c` Slice d。
- **手順 (CLAUDE.md ルール: 私はコミット、push は user)**:
  ```sh
  git push -u origin feature/ai-streaming-cancel-tokens
  gh pr create --title "feat(ai): streaming + cooperative cancel + token meter (ADR-0026)" \
               --base develop \
               --body-file <(echo "...")
  ```
  PR body 雛形が欲しければ「ADR-0026 の PR 文書いて」と一言下さい。
- **キックオフの一言例**:
  > 「push して PR 作っといて」 (= PR body 私が下書き)
  > 「push は俺がやるから body だけ書いて」

### 選択肢 1: マージ後 doc-sync chore PR

- マージ後、`docs/architecture.md` などに ADR-0026 への参照を追記する
  pattern を継続する場合の chore PR。
  過去パターン (PR #38 / #40 / #42 / #44) を踏襲。
- 規模感: 極小。

### 選択肢 2: 次の Group に着手

- Group C (history.jsonl AI records + v:2 schema bump + web side brief)
  または Group D (full DDL extraction + function-calling)。
- どちらも ADR-0023 §9 で deferred 済み。
- Group C は web 側 mirror が必要 (history JSON schema 変更のため) =
  cross-repo coordination の手筈を組む必要あり。
- Group D は in-process のみ = web 影響なし。

### 選択肢 3: 実利用 friction 報告

- streaming + cancel + token meter を実際に触って気になった点があれば
  Group C / D 着手より優先する余地あり (menu-not-sequence)。
- キックオフの一言例:
  > 「token meter の表示が見づらい、〜〜にしたい」
  > 「Cancel ボタンの位置を変えたい」

---

## web 側 (情報のみ・ボールは web 側)

- PR #29 で渡した fixture 受領後、web 側で `describe.skip` をフリップする
  作業が残っている。完了通知が来たらこちらの memory
  ([[dbboard-web-state]]) を更新する。
- ADR-0026 (Group B) は **web 側ミラー不要** = PR #33 の
  `0007-web-ai-phase6-no-contract-mirror.md` brief で explicit-no-op
  済み。追加 brief 不要。
- 上記以外の coordination は現時点で pending なし。

---

## 私単独で進められる作業がない理由 (確認用)

1. push は user (CLAUDE.md mandate)。PR 作成も基本は user 主導。
2. 次の Group 着手は user の優先順位選択 (Group C vs Group D vs friction)。
3. friction 報告は user が実利用してこそ出てくる情報 = 私が先回りで
   feature 追加するのは menu-not-sequence の原則に反する。

→ ブランチを push → PR 作成 → merge する段取りが user 側のボール。

---

## このファイルのメンテ規約

- セッション終了時、状況が動いた時は **必ず最新化**。
- 「最終更新」「develop tip」「test count」「選択肢」の 4 ブロックは
  毎回見直す。
- 大きな状態は memory ([[project-status-in-use]] など) に書き、ここは
  「user が次の一言を選ぶための短い案内」に留める。
