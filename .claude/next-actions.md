# 次のアクション案内 (user 側ボール常設ファイル)

このファイルは「セッションが切れても、開いた瞬間に user 側のボールがわかる」状態を維持するためのもの。
セッション終了時 / 状況が変わった時に必ず更新する。**日本語可・内部用。**

---

## 最終更新

- 日付: 2026-06-29
- develop tip: `5124b00` (PR #43 merged = ADR-0025 slice (b) 着地)
- 直近ハイライト: **ADR-0025 Phase 4 Stage 2 Group A クローズ** =
  `ai-providers.toml` + Settings UI + runtime in-process provider
  switcher の全体像完成。4 slice (a-1 PR #37 / a-2-α PR #39 / a-2-β
  PR #41 / b PR #43) 全 landed。次は本 chore PR (post-PR43 doc sync) の
  push & PR 化 → マージで Stage 2 Group A の対外的なクローズ宣言完了。
- ワークスペース test count: 474 件 pass
- ローカルブランチ状態: `chore/post-pr43-doc-sync` が `develop` から
  1 コミット先行 (status + next-actions 更新のみ)、push 待ち

## モード

**in-use / continuous-improvement (menu-not-sequence)** — 2026-06-24 以降。
ロードマップ順ではなく実利用の摩擦報告を優先。
私から自走で進めるべき作業はなし。**user 側のインプット待ち。**

---

## user 側のボール (= 次に着手する時の選択肢)

### 選択肢 0: `chore/post-pr43-doc-sync` を push & PR 化 — *最優先 (= 直前作業のクローズ)*

- **何**: 本 chore ブランチを `origin` に push、`develop` に対して
  PR を切る。`.claude/project-status.md` の PR #43 クローズ記録 +
  `next-actions.md` の Group A 完了後 menu 再生成。
- **なぜ最優先**: PR #40 / #42 と同じ post-PR doc sync パターン継続。
  Rust 無改変 / docs 無改変 / 内部メモのみ = 小さい PR、すぐマージ可。
- **キックオフの一言例**:
  > 「chore/post-pr43-doc-sync を push して PR 化して」
- **規模感**: 1 PR、`.claude/*` の 2 ファイルのみ、CI grep 想定。
- **依存**: なし。

### 選択肢 1: Phase 4 Stage 2 残り (Group B / C / D) を着手

- Group A クローズで Stage 2 全体への足場が整った = 順不同で着手可。
- **Group B**: streaming + cancel + token meter (`AiProvider` trait 拡張)
  - **規模感**: 中。`AiProvider::explain_sql` / `suggest_sql` を stream
    バージョンに変える設計判断 (trait method 追加 or 別 method)、
    worker channel に `Reply::AiChunk` / `Command::CancelAi` 追加、
    AI panel に cancel button + 部分表示。HTTP contract 無変更。
- **Group C**: `history.jsonl` への AI 記録、v:2 schema bump、**web 側
  fresh brief 必要** (= 0NNN-web-*.md を新規に書く工程込み)
  - **規模感**: 大。schema v:1 → v:2 migration、web 側の `desktop-history.jsonl`
    fixture 再生成 (PR #29 の `emit_history_fixture` を使う)、web 側に
    対応する `0008-web-*.md` ブリーフ。**唯一の cross-repo coordination**。
- **Group D**: full DDL extraction + function-calling
  - **規模感**: 中〜大。`AdapterCapabilities::extract_full_ddl()` 追加
    (Turso / Postgres flavors すべてに実装)、`AiProvider::suggest_sql` の
    schema 引数を `Vec<TableInfo>` から full DDL string に拡張。
- **キックオフの一言例**:
  > 「Group B から着手したい、streaming の trait 設計を検討して」
  > 「Group C の v:2 schema bump を設計するところから」

### 選択肢 2: 実利用の摩擦報告

- **何**: dbboard を実際に使っていて気になった点を口頭で渡す。
- **なぜ**: 現モードでは roadmap 順より friction 駆動が優先。
  Group A クローズで「AI Settings UI が初めて user の手に渡る」段階 =
  friction 報告が来やすいタイミング。
- **キックオフの一言例**:
  > 「AI Providers Settings 画面で Anthropic 以外を追加できないのが辛い」
  > 「Active subtitle が小さくて読みづらい」
  > 「Turso 接続で〜〜が辛かった、対処したい」
- **規模感**: 内容次第。issue / ADR を起こすかはこちらで提案。

### 選択肢 3: dbboard-web 側からの依頼を反映

- **何**: web 側で発生した coordination ネタを desktop 側に反映。
- **現時点で動いている web 側待ち**:
  - `dbboard-web/apps/api/test/fixtures/desktop-history.jsonl` (PR #29
    で渡した fixture) を使った `describe.skip` フリップ = **web 責務**、
    desktop 側からは何もしない。
  - ADR-0025 全体は web 側ミラー不要 (in-process AI 設計)。
  - その他、web 側からの contract 変更要求があれば随時。
- **キックオフの一言例**:
  > 「web 側から `/views` endpoint を追加したいと依頼が来た」
- **規模感**: 内容次第。HTTP contract に触る場合は ADR + 双方ミラー。

---

## web 側 (情報のみ・ボールは web 側)

- PR #29 で渡した fixture 受領後、web 側で `describe.skip` をフリップする
  作業が残っている。完了通知が来たらこちらの memory
  ([[dbboard-web-state]]) を更新する。
- ADR-0025 関連は in-process AI 設計のため web 側ミラー不要、明示的に
  notified 済み (Group C で AI を `history.jsonl` に書く段階で初めて
  web 側ブリーフが必要)。
- 上記以外の coordination は現時点で pending なし。

---

## 私単独で進められる作業がない理由 (確認用)

1. `chore/post-pr43-doc-sync` 1 コミットはローカルに居る = push は user 操作。
2. push 前に Rust に手を入れると review が崩れる = 静止すべき。
3. 現モードは menu-not-sequence。Group B/C/D は order が user 判断、
   勝手に開始するのは過剰。
4. web 側依頼は現時点で着信なし。

→ どれを選ぶかは user 判断。上の選択肢 0〜3 のどれかを伝えてもらえれば
即着手可能 (選択肢 0 = push & PR 化が最優先)。

---

## このファイルのメンテ規約

- セッション終了時、状況が動いた時は **必ず最新化**。
- 「最終更新」「develop tip」「test count」「選択肢」の 4 ブロックは
  毎回見直す。
- 大きな状態は memory ([[project-status-in-use]] など) に書き、ここは
  「user が次の一言を選ぶための短い案内」に留める。
