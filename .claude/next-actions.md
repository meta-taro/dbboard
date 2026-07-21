# 次のアクション案内 (user 側ボール常設ファイル)

このファイルは「セッションが切れても、開いた瞬間に user 側のボールがわかる」状態を維持するためのもの。
セッション終了時 / 状況が変わった時に必ず更新する。**日本語可・内部用。**

---

## 最終更新

- 日付: 2026-07-21
- develop tip: `0f734ff` (PR #90 まで merged)。main = v0.2.0 タグ (`891d2cc`)。
- **✅ 候補 B (ローカル注釈, ADR-0045) develop 着地 (PR #90, merge commit `0f734ff`)。**
  config ディレクトリの `annotations.toml` (キー = 接続 **id**/テーブル/カラム) に
  注釈を持ち、Structure タブに編集可能な Note 列を追加。DB 非書き込み・read-only
  接続でも可・全アダプタ一律・全13ロケール i18n。検証全 green (281 tests, うち
  annotations 15) + `rust-reviewer` Approve (CRITICAL/HIGH ゼロ)。残 MEDIUM
  (Structure render のファイル/関数サイズ, per-frame clone) は既存債務の継続 =
  `.claude/issues/0016` に follow-up 化。この doc-sync (`chore/post-pr90-doc-sync`)
  = roadmap tick + project-status + 本ファイル。
- **→ 次の user 側ボール: 候補 A (AI プロバイダ実地テスト)。** maintainer 意向で
  B と同リリース同梱予定だった片割れ。着手に必要な決定 = **キーの渡し方**
  (`.dbbx` バンドル経由 / 直接入力)。下記「候補 A」参照。
- **✅ 配れるインストーラ + Release CI を PR #88 で整備 (ADR-0044):** MSI
  ビルド不能の WiX v3 属性バグ修正 (ローカルで MSI 生成確認) + macOS
  `.app`/`.dmg` 用 `cargo-bundle` 設定を in-tree 化 + `v*.*.*` タグ push で
  Win(exe+MSI)+Mac(.dmg) を `SHA256SUMS.txt` 付きで GitHub Release 公開する
  `release.yml`。GH Actions 追加のセキュリティレビューで HIGH2/MEDIUM1 修正済。
  **CI は未実走 (Windows で作成) = 初回タグ push か dispatch 空撃ちが初テスト。**
  未署名 = SmartScreen/Gatekeeper 警告残 (署名は roadmap の新規項目)。
- **✅ v0.2.0 リリース済 (PR #84 bump → PR #85 release merge)。** `develop→main`
  規約どおり 0.1.0→0.2.0 bump 後 main にマージ、v0.2.0 タグ + exe 資産公開
  (`gh release create`)。`releases/latest` = v0.2.0 を確認済 (= update-check の
  GET 対象)。公開前に exe を実接続名でスキャン (0 一致)。
- **✅ #14 ハンドオフ exe は 2026-07-16 に配布済 (usage 未確認)。** 配布した
  exe は番号 0.1.0 だが中身は update-check 入りの develop ビルド。v0.2.0 公開は
  「その exe が起動時に更新を検知できるか」の実地プローブを兼ねる。観測できる
  唯一の使用シグナル = リリース資産の downloadCount (匿名 API GET は観測不可)。
  → `gh release view v0.2.0 --json assets --jq '.assets[].downloadCount'`。
- **✅ Help メニュー更新通知の 2 バグを PR #86 で修正:**
  - ①メニューがクリックで即閉じ、リンク/変更点を操作できない →
    `CloseOnClickOutside` 化 (`MenuButton`/`MenuConfig`)
  - ②変更点が生 Markdown 表示 → `egui_commonmark 0.23` で描画 (ADR-0043)。
    MSRV 1.75→1.92 (egui_commonmark 要件)。
  roadmap 追随注記 + project-status + 本ファイルの sync は
  `chore/post-pr86-doc-sync` (このブランチ)。
- **✅ 実利用バックログ 0012–0015 (PR #76/#77/#78/#79) + 実機 4 バグ PR #82** も
  全て develop 着地済 (前セッション)。
- **進行中の目標: 収集担当への内々配布 (Windows-only)。** store-a
  (Cloudflare D1) / store-b (Aurora DSQL IAM) / store-c
  (Supabase) の 3 接続を収集する担当に dbboard デスクトップを渡す。
  ※ id は中立サンプル名。実際の店舗名との対応は非公開メモリ側にのみ保持。

## モード

**in-use / continuous-improvement (menu-not-sequence)** — 2026-06-24 以降。
配布 (#14) は 2026-07-16 に完了済。今は「配布済 exe を担当が実際に使うか」を
v0.2.0 の update-check で観測しつつ、次の実利用改善 (下記の user 側ボール) を
摩擦順に進めるフェーズ。

---

## user 側のボール (= 次に着手する時の選択肢)

### ★ 候補 A: AI プロバイダの実地テスト (未実施)

AI プロバイダ (Anthropic) はコード実装済だが**実地テスト未了**。テスト手順 =
接続に Anthropic API キーを入れて自然文→SQL を 1 回流す。詰まりが出れば
friction として拾い、次リリースに反映。**キーの渡し方** (`.dbbx` バンドル経由
/ 直接入力) を決めれば着手可。maintainer は「ローカルメモ機能と一緒に出したい」
意向 (2026-07-17)。

### ✅ 候補 B: ローカルメモ機能 (Structure タブに注釈列) — 完了 (PR #90 merged)

**develop 着地済** (ADR-0045, PR #90, merge commit `0f734ff`)。config ディレクトリの
`annotations.toml` (キー = 接続ID/テーブル/カラム, 接続 **id** 固定なので接続名変更
でも残る) に注釈を持ち、Structure タブに編集可能な Note 列を追加。DB には一切
書かない (権限不要・read-only 接続でも可)。
- 意図的に範囲外 (別 ADR): Postgres `pg_description` 併記は `describe_table`
  (adapter+core) 改修が要るので延期。`.dbbx` 同梱は却下 (暗号 secret bundle と
  非 secret ドキュメントは intent 不一致)、共有が要るなら別の plain-text export。
- follow-up debt: `.claude/issues/0016` (render 抽出 / per-frame clone 除去 / テスト追加)。

### 候補 C: cargo-deny の既存ドリフト対応 (別 chore)

`cargo deny` が advisories/licenses 3 件 FAILED (PR #86 とは無関係・既存依存に
RustSec 新規 2026 アドバイザリが後から命中): `proc-macro-error2` (unmaintained
← age) / `option-ext` (MPL-2.0 ← directories) / `quick-xml` (DoS ←
wayland-scanner ← eframe, Linux のみ)。cargo-deny は commit フックではないので
緊急ではないが、`deny.toml` に一時 exception (期限付き) か依存 bump で解消する。

### 候補 D: 既存ロードマップ機能バックログ

未着手: Export results は済 (CSV/JSON) / Saved queries / Schema diff /
Group D-2 (ADR-0029 function-calling, `feature/adr-0029-function-calling` に
planning ball)。実利用の摩擦順に着手。新 write 経路は着手前に ADR。

### 参考: 配布済 exe の使用シグナル確認 / 再配布

- **使用確認**: `gh release view v0.2.0 --json assets --jq
  '.assets[].downloadCount'` (匿名 update-check の GET 自体は観測不可、
  資産 DL 数のみ)。
- **新版を配布したくなったら**: develop から `cargo build --release` →
  次バージョンを bump → main にマージ → タグ + `gh release create` で exe 資産。
  配布済 0.1.0 exe が起動時に検知する。ビルド前に dbboard ウィンドウを閉じる
  (exe ロックで os error 5)。公開前に exe を実接続名でスキャン (0 一致)。
- **MSI / .dmg で渡す場合 (PR #88)**: ローカル MSI = WiX v3 + `cargo install
  cargo-wix` → `cd apps/dbboard && cargo wix` (`Absent` 属性バグ修正済で通る)。
  Mac は `cargo bundle --release` → `hdiutil` で `.dmg`。あるいは `v*.*.*`
  タグ push で Release CI が Win+Mac 資産を `SHA256SUMS.txt` 付きで自動公開
  (**ただし CI 初実走は要 shake-out**)。exe 単体で十分なら不要。
- secret 移送 = **推奨 (ADR-0038)**: 手元で 3 接続を Export → `.dbbx` を渡し
  パスフレーズは別経路。担当機は Import 1 回。旧 cmdkey 手順は
  `docs/collector-setup/README.md`。**secret は一切ファイルに書かない。**

---

## ⚠️ 接続名サニタイズ (2026-07-15 着手)

- **経緯**: public リポジトリのソース/テスト/テンプレに実業務接続名が
  露出していた (2026-07-13〜14 のハンドオフ準備でテストのサンプルデータ
  として実名を使ってしまったのが原因)。**出荷 exe には非埋め込み**
  (テストは `#[cfg(test)]`、テンプレは `tests/` の include_str! のみ)。
- **現行置換 = 実施済み** (このブランチ `chore/sanitize-connection-names`)。
  実名を中立サンプル id (store-a / store-b / store-c) + サンプル行データ
  (Alpha / Beta) に一括置換。実名↔サンプルの対応は非公開メモリのみ保持。
- **履歴書き換え = human のボール (未実行)**: 過去コミットにはまだ実名が
  残る。`docs/maintainer/history-sanitize-runbook.md` の手順で
  `git filter-repo --replace-text` → develop/main を force-push する。
  破壊的操作 (全ハッシュ変更・既存クローン/PR/フォーク破損) のため human 実行。

---

## web 側 (情報のみ・ボールは web 側)

- brief 0008 = v:2 schema mirror が web 側 pending。
- ADR-0030/0031 (query-UX) / ADR-0032 (Windows packaging) / ADR-0036 /
  ADR-0037 (aurora-dsql-iam 段階A/B) はいずれも in-process ないし build
  のみ = web ミラー不要 (確定)。
- ADR-0029 (D-2) も同 posture の見込み、確定は起票時。

---

## このファイルのメンテ規約

- セッション終了時、状況が動いた時は **必ず最新化**。
- 「最終更新」「develop tip」「選択肢」ブロックは毎回見直す。
- 大きな状態は memory ([[project-status-in-use]] /
  [[project-windows-internal-distribution]] など) に書き、ここは
  「user が次の一言を選ぶための短い案内」に留める。
