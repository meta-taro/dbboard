# 次のアクション案内 (user 側ボール常設ファイル)

このファイルは「セッションが切れても、開いた瞬間に user 側のボールがわかる」状態を維持するためのもの。
セッション終了時 / 状況が変わった時に必ず更新する。**日本語可・内部用。**

---

## 最終更新

- 日付: 2026-07-10
- develop tip: `1cec10f` (**PR #52 = Windows 内々配布パッケージング
  (ADR-0032) merged**)
- 作業ブランチ: `chore/post-pr52-doc-sync` (develop `1cec10f` から分岐、
  project-status / roadmap / next-actions の tick、**push + chore PR
  create 待ち**)
- 直近ハイライト:
  - **PR #52 (2026-07-10): Windows 内々配布パッケージング完了
    (ADR-0032)。** exe 整備 3 点 (コンソール窓抑止 / アイコン+製品情報 /
    CRT 静的リンク) + cargo-wix MSI ソース。build/packaging のみ =
    contract 不変、非 Windows は no-op。全検証 green。
  - **PR #51 (2026-07-10): query-UX 摩擦バッチ完了 (ADR-0030/0031)。**
    run trigger / result grid 刷新 / auto-LIMIT / structure タブ。

## モード

**in-use / continuous-improvement (menu-not-sequence)** — 2026-06-24 以降。
ロードマップ順ではなく実利用ドリブン。今回の query-UX (PR #51) と
Windows 配布 (PR #52) がその典型。Phase 4 Stage 2 Group A/B/C/D-1 完了、
残りは D-2 (ADR-0029 = function-calling) のみで
`feature/adr-0029-function-calling` に planning ball あり (別ストリーム)。

---

## user 側のボール (= 次に着手する時の選択肢)

### **★ 最優先: `chore/post-pr52-doc-sync` の push + chore PR 作成**

- **何**: この doc-sync ブランチ (project-status / roadmap /
  next-actions の tick) を push し、develop に対して chore PR を立てる。
  ```
  git push -u origin chore/post-pr52-doc-sync
  ```
- doc-only なので pre-commit は `--no-verify` 済 (Windows libsql teardown
  segfault flake 回避の常設慣例)。希望あれば PR 本文ドラフトを用意する。

### 選択肢 1: Windows MSI の実ビルド (配布したくなったら)

- PR #52 で MSI **ソース**は揃った。実ビルドは human 手順:
  1. WiX Toolset v3 をインストール (candle.exe / light.exe を PATH に)
  2. `cargo install cargo-wix`
  3. `cd apps/dbboard && cargo wix` → `target\wix\dbboard-0.1.0-x86_64.msi`
- **exe 単体配布なら不要** = `cargo build --release` の
  `target\release\dbboard.exe` (自己完結・15MB) をそのまま渡せる。
- release CI (`cargo wix` on tag) は未着手 = 任意の follow-up。

### 選択肢 2: ADR-0029 (Group D-2 = function-calling) draft 着手

- `feature/adr-0029-function-calling` ブランチに draft ball あり。
- D-1 の `describe_table` primitive が landed = D-2 の前提が揃った。
- AI provider 側に tool-use surface を追加し、`describe_table` を最初の
  callable tool として expose。in-process only の見込み。

### 選択肢 3: 現状 friction 報告

- 実利用で困っていることがあれば優先。既知の deferred 候補:
  > 「AI history record を history panel に描画してほしい」(ADR-0027 で
    意図的に deferred = rich viewer)
  > 「Include column details / auto-LIMIT を session 跨ぎで記憶」
    → `ui-preferences.toml` 系の小 ADR で拾える
  > 「大規模 schema で prompt が重い」→ ADR-0028 open question 再訪

---

## web 側 (情報のみ・ボールは web 側)

- brief 0008 = v:2 schema mirror が web 側 pending。
- ADR-0030/0031 (query-UX) / ADR-0032 (Windows packaging) はいずれも
  in-process ないし build のみ = web ミラー不要 (確定)。
- ADR-0029 (D-2) も同 posture の見込み、確定は起票時。

---

## このファイルのメンテ規約

- セッション終了時、状況が動いた時は **必ず最新化**。
- 「最終更新」「develop tip」「選択肢」ブロックは毎回見直す。
- 大きな状態は memory ([[project-status-in-use]] /
  [[project-windows-internal-distribution]] など) に書き、ここは
  「user が次の一言を選ぶための短い案内」に留める。
