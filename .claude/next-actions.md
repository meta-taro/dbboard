# 次のアクション案内 (user 側ボール常設ファイル)

このファイルは「セッションが切れても、開いた瞬間に user 側のボールがわかる」状態を維持するためのもの。
セッション終了時 / 状況が変わった時に必ず更新する。**日本語可・内部用。**

---

## 最終更新

- 日付: 2026-07-14
- develop tip: `ced0941` (PR #56 aurora-dsql-iam 段階A merged)
- **進行中の目標: 収集担当への内々配布 (Windows-only)。** store-cabaret
  (Cloudflare D1) / store-lovehotel (Aurora DSQL IAM) / Vegas Gift
  (Supabase) の 3 接続を収集する担当に dbboard デスクトップを渡す。
  ハンドオフ前に 4 項目を入れる方針: 段階B (トークン自動リフレッシュ) /
  About・バージョン表示 / ヘルプメニュー / テーブル右クリック簡易SQL。
- **4 項目の進捗:**
  - ✅ テーブル右クリック簡易SQL = develop マージ済。
  - 🔲 About/バージョン + ヘルプメニュー = **PR #60 OPEN・MERGEABLE**
    (`feat/about-help-menu`, 3 commits)。**human のマージ待ち。**
  - 🔲 段階B = **実装完了・push 待ち**
    (`feature/adr-0037-dsql-token-refresh`, 3 commits, 下記)。
- **human のボール (push / merge 待ち):**
  - **PR #60 (`feat/about-help-menu`)** = MERGEABLE。develop にマージするだけ。
  - **`feature/adr-0037-dsql-token-refresh`** = 段階B 実装 (ADR-0037)。
    未 push。3 commits: `90b2392` ADR-0037 + `1811fa5` 実装 + `38e556f`
    README。pre-push ゲート (build --release / test --release) まで green。
    ```
    git push -u origin feature/adr-0037-dsql-token-refresh
    ```

## モード

**in-use / continuous-improvement (menu-not-sequence)** — 2026-06-24 以降。
今は収集担当への配布に向けたハンドオフ準備が実利用ドリブンの主軸。

---

## user 側のボール (= 次に着手する時の選択肢)

### **★ 最優先: PR #60 マージ + 段階B ブランチ push**

1. **PR #60 (`feat/about-help-menu`) を develop にマージ** (MERGEABLE)。
2. **段階B ブランチを push** して PR 作成 → develop にマージ:
   ```
   git push -u origin feature/adr-0037-dsql-token-refresh
   ```
- どちらも #56 マージで解消済。段階B は ADR-0037 として実装完了、
  ゲート待ちは無い。

### ゲート後の残タスク (agent 側で着手可能)

- **#9 収集セットアップ pack** = connections.toml テンプレ (D1/Supabase/
  aurora-dsql-iam の 3 kind) + Windows 資格情報マネージャーへの secret
  シード手順 + クイックスタート。**secret は一切ファイルに書かない。**
  aurora-dsql-iam 部分の依存 (#56) は develop 入り済 → 今すぐ着手可。
- **#14 収集リリースのビルド & ハンドオフ** = 上記すべて + #60 + 段階B が
  develop に入った後、`cargo build --release` の exe (または `cargo wix`
  の MSI) を担当に渡す。exe 単体で自己完結 (15MB)。

### doc-sync (house パターン: 別 chore ブランチ)

- 段階B / #56-merge / #60-merge の後追いで `docs/roadmap.md` の tick +
  `.claude/project-status.md` を `chore/post-adr-0037-doc-sync` に載せる。
  feat ブランチには載せない。

### 参考: MSI 実ビルド手順 (配布したくなったら)

- PR #52 で MSI **ソース**は揃済。human 手順:
  1. WiX Toolset v3 をインストール (candle.exe / light.exe を PATH に)
  2. `cargo install cargo-wix`
  3. `cd apps/dbboard && cargo wix` → `target\wix\dbboard-0.1.0-x86_64.msi`
- **exe 単体配布なら不要** = `target\release\dbboard.exe` をそのまま渡せる。

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
