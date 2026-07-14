# 次のアクション案内 (user 側ボール常設ファイル)

このファイルは「セッションが切れても、開いた瞬間に user 側のボールがわかる」状態を維持するためのもの。
セッション終了時 / 状況が変わった時に必ず更新する。**日本語可・内部用。**

---

## 最終更新

- 日付: 2026-07-14
- develop tip: `150c458` (PR #57 merged)
- **進行中の目標: 収集担当への内々配布 (Windows-only)。** store-cabaret
  (Cloudflare D1) / store-lovehotel (Aurora DSQL IAM) / Vegas Gift
  (Supabase) の 3 接続を収集する担当に dbboard デスクトップを渡す。
  ハンドオフ前に 4 項目を入れる方針: 段階B (トークン自動リフレッシュ) /
  About・バージョン表示 / ヘルプメニュー / テーブル右クリック簡易SQL。
- **未 push のローカルブランチ (すべて human の push + PR 待ち):**
  - `feature/aurora-dsql-iam` @ `eaa0dfa` = **PR #56 OPEN** (段階A
    aurora-dsql-iam feature `dd602b2` + reconnect ボタン `eaa0dfa`)。
    ライブテストで段階A の idle 再接続失敗を確認済 → reconnect は暫定策。
  - `feat/about-help-menu` @ `fa48490` = **About/バージョン + ヘルプ
    メニュー** (2026-07-14, develop から分岐)。全検証 green・pre-commit 通過。
  - `feat/table-quick-sql` @ `f03a0b2` = **テーブル右クリック簡易SQL**
    (SELECT * / COUNT(*)、read-only、2026-07-14, develop から分岐)。
    全検証 green・pre-commit 通過。

## モード

**in-use / continuous-improvement (menu-not-sequence)** — 2026-06-24 以降。
今は収集担当への配布に向けたハンドオフ準備が実利用ドリブンの主軸。

---

## user 側のボール (= 次に着手する時の選択肢)

### **★ 最優先: 3 ブランチの push + PR、そして PR #56 の develop マージ**

- `feature/aurora-dsql-iam` (PR #56)、`feat/about-help-menu`、
  `feat/table-quick-sql` を push し PR を作成 → develop にマージ。
  ```
  git push -u origin feature/aurora-dsql-iam   # 既に PR #56
  git push -u origin feat/about-help-menu
  git push -u origin feat/table-quick-sql
  ```
- **PR #56 のマージが以下のゲート:** 段階B (#13) と収集セットアップ pack の
  Aurora 部分 (#9) は `aurora-dsql-iam` kind に依存するため、#56 が
  develop に入ってから着手するのが筋。

### ゲート後の残タスク (agent 側で着手可能)

- **#13 段階B = Aurora DSQL IAM トークンのプール内自動リフレッシュ。**
  新規 ADR (番号は #56=ADR-0036 マージ後に確定、暫定 0037)。sqlx 0.8 は
  per-connection password callback を持たないため custom connector /
  bespoke pool が要る。ハンドオフの本命 (24/7 無人運用の要件)。
- **#9 収集セットアップ pack** = connections.toml テンプレ (D1/Supabase/
  aurora-dsql-iam の 3 kind) + Windows 資格情報マネージャーへの secret
  シード手順 + クイックスタート。**secret は一切ファイルに書かない。**
  D1/Supabase 部分は develop からでも書けるが、aurora-dsql-iam 部分は
  #56 マージ後。
- **#14 収集リリースのビルド & ハンドオフ** = 上記すべて + #10/#11/#12 が
  develop に入った後、`cargo build --release` の exe (または `cargo wix`
  の MSI) を担当に渡す。exe 単体で自己完結 (15MB)。

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
