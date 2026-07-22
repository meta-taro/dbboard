# 次のアクション案内 (user 側ボール常設ファイル)

このファイルは「セッションが切れても、開いた瞬間に user 側のボールがわかる」状態を維持するためのもの。
セッション終了時 / 状況が変わった時に必ず更新する。**日本語可・内部用。**

---

## 最終更新

- 日付: 2026-07-22 (**v0.3.0 リリース完了 + MCP サーバ着地。次の user 側ボール =
  DL ページ (GitHub Pages) の可否確認と着手判断。**)
- develop tip: `97ed4ef` (PR #101 まで merged)。main = `70ecb93` = **v0.3.0 タグ**。
- **▶ 今の user 側ボール: DL ページ / GitHub Pages。** GitHub Pages は public リポで
  **無料**。提案 = 別 feat PR (`feat/download-page` + **ADR-0047**) で、first-party
  action (`actions/configure-pages` / `upload-pages-artifact` / `deploy-pages`) を
  使った Pages デプロイ workflow + DL ページ (最新リリース資産へのリンク +
  `SHA256SUMS.txt` 検証手順 + 未署名バイナリの注意書き)。**未着手・scope 未確定** =
  次に user と詰める (このリリース資産だけ静的に貼るか / Releases API を叩いて
  自動追随するか)。※ この chore (doc-sync) とは別 PR。
- **✅ v0.3.0 リリース済 (2026-07-22):** 目玉 = read-only MCP サーバ
  `dbboard-mcp` ([ADR-0046](../docs/decisions.md), PR #95)。dbboard を AI
  *サーバ* にもした (stdio 5 ツール固定・秘密非露出・read-only エンジン強制)。
  併せて着地: #92 AI エラー本文修正 / #93 AI アシスタント help / #94 既定モデル
  `claude-sonnet-5` / #96 AI パネル表示スコープ。リリース = #97 bump →
  #98 main マージ・タグ → macOS CI 2 連敗 (cargo-bundle の `--package` 非対応 →
  #99、`version.workspace = true` 不読 → #100 で version inline) →
  publish が `release not found` (`gh release upload` は作成しない) →
  `gh release create` 先行 + `gh run rerun --failed` で解消。詳細は
  project-status.md と [[project-release-ci-needs-release-object]]。
  最終 CI 全 green、Release 非 draft・Latest・資産 4 点。
- **✅ 候補 A (AI プロバイダ実地テスト) は事実上完了。** 実地テストで拾った
  3 findings (error-body #92 / model #94 / scope #96) が全て develop→v0.3.0 に着地。
- **✅ 候補 B (ローカル注釈 ADR-0045, PR #90) も v0.3.0 に同梱。**
- **✅ OSS 公開前 PII スイープ済 (user 依頼):** 追跡ツリーは実名/個人情報 0 件、
  唯一の実 PII = project-status のローカルユーザ名 → #101 で伏字化。公開 exe も
  スキャン 0 件・SHA256 一致確認済。
- **進行中の目標: 収集担当への内々配布 (Windows-only)。** store-a
  (Cloudflare D1) / store-b (Aurora DSQL IAM) / store-c
  (Supabase) の 3 接続を収集する担当に dbboard デスクトップを渡す。
  ※ id は中立サンプル名。実際の店舗名との対応は非公開メモリ側にのみ保持。

## モード

**in-use / continuous-improvement (menu-not-sequence)** — 2026-06-24 以降。
配布 (#14) は 2026-07-16 に完了済、v0.3.0 公開済。今は「配布済 exe を担当が実際に
使うか」を update-check で観測しつつ、次の実利用改善 (下記の user 側ボール) を
摩擦順に進めるフェーズ。

---

## user 側のボール (= 次に着手する時の選択肢)

### ★ 候補 A: DL ページ / GitHub Pages (未着手・今回の user 関心)

GitHub Pages は public リポで無料。別 feat PR (`feat/download-page` + ADR-0047)
で Pages デプロイ workflow + DL ページを用意する。**着手前に scope を確定**:
(1) 最新リリース資産へのリンクを静的に貼るだけか、(2) GitHub Releases API を
叩いて最新版を自動追随させるか。いずれも `SHA256SUMS.txt` 検証手順と未署名
バイナリ (SmartScreen/Gatekeeper) の注意書きを載せる。first-party action 3 種
(`configure-pages`/`upload-pages-artifact`/`deploy-pages`) を使う。GH Actions
追加なので merge 前にセキュリティレビュー。

### 候補 B: git 履歴の実店舗名 rewrite (human ボール・破壊的・未実行)

過去コミットに実店舗名がまだ残る (`store-cabaret`/`store-lovehotel`/`vegas-gift`
系)。バイナリはCIビルドで名前を含まないためリリースは塞がないが、公開リポの
履歴には残る。`docs/maintainer/history-sanitize-runbook.md` の手順で
`git filter-repo --replace-text` → develop/main を **force-push**。全ハッシュ
変更・既存クローン/PR/フォーク破損のため **human 実行**。

### 候補 C: release.yml の publish 自己作成化 (follow-up)

現状 `gh release upload` は既存リリースにしか添付できず、タグ push だけでは
`release not found` で落ちる (毎回手動で `gh release create` が前提)。publish
ステップを `gh release view <tag> || gh release create <tag> --generate-notes`
にしてタグ push を自己完結させる。[[project-release-ci-needs-release-object]]。

### 候補 D: cargo-deny の既存ドリフト対応 (別 chore)

`cargo deny` が advisories/licenses で FAILED の可能性 (既存依存への 2026
アドバイザリ): `proc-macro-error2` (unmaintained ← age) / `option-ext`
(MPL-2.0 ← directories) / `quick-xml` (DoS ← wayland-scanner ← eframe, Linux)。
commit フックではないので緊急ではないが `deny.toml` の期限付き exception か
依存 bump で解消。着手時に現状を再確認。

### 候補 E: 既存ロードマップ機能バックログ

未着手: Saved queries / Schema diff / Export results は済 (CSV/JSON) /
Group D-2 (ADR-0029 function-calling, `feature/adr-0029-function-calling` に
planning ball)。実利用の摩擦順に着手。新 write 経路は着手前に ADR。

### 参考: 配布済 exe の使用シグナル確認 / 再配布

- **使用確認**: `gh release view v0.3.0 --json assets --jq
  '.assets[].downloadCount'` (匿名 update-check の GET 自体は観測不可、
  資産 DL 数のみ)。
- **新版を配布したくなったら**: develop から `cargo build --release` →
  次バージョンを bump → main にマージ → タグ push で Release CI が Win+Mac
  資産を自動公開。**⚠ ただしリリースオブジェクトを先に `gh release create`
  しておくこと** (publish は添付のみ)。配布済 exe が起動時に検知する。ビルド前に
  dbboard ウィンドウを閉じる (exe ロックで os error 5)。公開前に exe を実接続名で
  スキャン (0 一致)。
- **MSI / .dmg で渡す場合 (PR #88)**: ローカル MSI = WiX v3 + `cargo install
  cargo-wix` → `cd apps/dbboard && cargo wix`。Mac は `cd apps/dbboard`
  → version inline → `cargo bundle --release` → `hdiutil` で `.dmg`
  (cargo-bundle 0.6.0 は `--package` 非対応 + workspace version 不読なので
  README の macOS 手順に従う)。exe 単体で十分なら不要。
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
