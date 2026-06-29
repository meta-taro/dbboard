# 次のアクション案内 (user 側ボール常設ファイル)

このファイルは「セッションが切れても、開いた瞬間に user 側のボールがわかる」状態を維持するためのもの。
セッション終了時 / 状況が変わった時に必ず更新する。**日本語可・内部用。**

---

## 最終更新

- 日付: 2026-06-29
- develop tip: `6e6eb83` (PR #42 merged = PR #41 post-PR doc sync 完了)
- 直近ハイライト: **ADR-0025 slice (b) ship 完了** = `feature/ai-settings-ui`
  に 5 コミット (`a1eae06` / `e087ac8` / `99e0ba4` / `11a5ef6` / `e00ae20`)
  着地、push 待ち = **maintainer 操作 (= user 側のボール)**。
  これで **Phase 4 Stage 2 Group A クローズ** = `ai-providers.toml` +
  Settings UI + runtime provider switcher の全体像が in-process で完成。
- ワークスペース test count: **474 件 pass** (461 → +13 = `AiSettingsView` 単体テスト)
- ローカルブランチ状態: `feature/ai-settings-ui` が `develop` から 5 コミット先行、
  release build / release test も clean

## モード

**in-use / continuous-improvement (menu-not-sequence)** — 2026-06-24 以降。
ロードマップ順ではなく実利用の摩擦報告を優先。
私から自走で進めるべき作業はなし。**user 側のインプット待ち。**

---

## user 側のボール (= 次に着手する時の選択肢)

### 選択肢 0: `feature/ai-settings-ui` を push & PR 化 — *最優先 (= 直前作業のクローズ)*

- **何**: `feature/ai-settings-ui` を `origin` に push、`develop` に対して
  PR を切る。5 コミット = slice (b) + 11 ロケール + ワイヤリング + apps
  配線 + docs sweep。
- **なぜ最優先**: 直前セッションの成果物がローカルにしか居ない。push
  しないと CI も走らず、レビューも始まらない。
- **キックオフの一言例**:
  > 「feature/ai-settings-ui を push して PR 化して」
- **規模感**: 5 コミット、Rust + Fluent + Markdown、HTTP contract 無変更、
  web 側影響ゼロ。PR description は full commit history を分析して生成。
- **依存**: なし。`develop` (= `6e6eb83`) からクリーン分岐。

### 選択肢 1: ADR-0025 slice (b) PR マージ後の post-PR doc sync

- **何**: PR がマージされた後、PR #40 / #42 と同じパターンで
  `chore/post-prNN-doc-sync` ブランチを切り、`.claude/project-status.md` の
  PR ヘッダ更新 + `next-actions.md` 再生成。
- **なぜ**: doc-fresh feedback ([[feedback-keep-docs-fresh]]) で
  「feat PR の merge 直後に short chore PR」が pattern として定着。
- **キックオフの一言例**:
  > 「PR マージしたので post-pr-doc-sync を切って」
- **規模感**: 1 PR、docs / status のみ、Rust 無改変、CI grep 想定。

### 選択肢 2: 実利用の摩擦報告

- **何**: dbboard を実際に使っていて気になった点を口頭で渡す。
- **なぜ**: 現モードでは roadmap 順より friction 駆動が優先。
- **キックオフの一言例**:
  > 「Turso 接続で〜〜が辛かった、対処したい」
  > 「AI Providers Settings 画面で〜〜が使いにくい」
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

### 選択肢 4: Phase 4 Stage 2 の残りグループ (B / C / D)

- Group A クローズで Stage 2 全体への足場が整った = 順不同で着手可。
- **Group B**: streaming + cancel + token meter (`AiProvider` trait 拡張)
- **Group C**: `history.jsonl` への AI 記録、v:2 schema bump、**web 側
  fresh brief 必要** (= 0NNN-web-*.md を新規に書く工程込み)
- **Group D**: full DDL extraction + function-calling
- **キックオフの一言例**:
  > 「Group C の v:2 schema bump を設計するところから」

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

1. `feature/ai-settings-ui` 5 コミットはローカルに居る = push は user 操作。
2. push 前に追加で触ると review が崩れる = 静止すべき。
3. 現モードは menu-not-sequence。Group B/C/D は standing next の優先度が
   slice b より低く、user 不在で勝手に開始するのは過剰。
4. web 側依頼は現時点で着信なし。

→ どれを選ぶかは user 判断。上の選択肢 0〜4 のどれかを伝えてもらえれば
即着手可能 (選択肢 0 = push & PR 化が最優先)。

---

## このファイルのメンテ規約

- セッション終了時、状況が動いた時は **必ず最新化**。
- 「最終更新」「develop tip」「test count」「選択肢」の 4 ブロックは
  毎回見直す。
- 大きな状態は memory ([[project-status-in-use]] など) に書き、ここは
  「user が次の一言を選ぶための短い案内」に留める。
