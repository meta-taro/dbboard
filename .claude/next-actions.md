# 次のアクション案内 (user 側ボール常設ファイル)

このファイルは「セッションが切れても、開いた瞬間に user 側のボールがわかる」状態を維持するためのもの。
セッション終了時 / 状況が変わった時に必ず更新する。**日本語可・内部用。**

---

## 最終更新

- 日付: 2026-07-24 (**OpenAI/ChatGPT プロバイダ ADR-0052 が PR #114 で develop 着地。**
  ADR-0025 §Out-of-scope が defer した 2 つ目の AI プロバイダ = 新クレート
  `dbboard-openai` (dbboard-anthropic の兄弟) が **Chat Completions**
  (`POST /v1/chat/completions`) を実装。**フル SSE ストリーミング parity**、既定
  モデル `gpt-4o`、Bearer 認証、キーは keyring のみ。config/ui/app/i18n に配線
  (`kind = "openai"`、Add フォームに kind セレクタ ComboBox、env フォールバックは
  Anthropic 専用のまま)。ADR + README 同梱。48 unit+wiremock、全ゲート green。**今の
  user 側ボール = (1) この chore doc-sync PR (`chore/post-pr114-doc-sync`) のマージ、
  (2) OpenAI の実地スモーク (下記)、(3) restore 実地確認の積み残し、(4) 次の実利用
  摩擦テーマの選択。**)
- develop tip: PR #114 (OpenAI provider ADR-0052, merge `ba54d02`) が最新。
  直前は #112 (restore/import ADR-0051 `e624bbb`) → #113 (doc-sync)。
  main = `70ecb93` = **v0.3.0 タグ** (未リリース差分あり = MCP 以降 + backup +
  restore + OpenAI provider)。
- **✅ OpenAI/ChatGPT プロバイダ (PR #114, ADR-0052):** Claude と並ぶ 2 つ目の
  AI プロバイダ。新クレート `dbboard-openai` が **Chat Completions**
  (`POST /v1/chat/completions`) を実装 (Responses API ではなく安定面を選択)。
  **フル SSE ストリーミング** = 実パーサ (`data:` フレーム・`[DONE]` センチネル・
  `stream_options.include_usage` 経由の usage) を既存 `StreamEvent` 列に正規化、
  Claude 同様トークン逐次表示。既定モデル `gpt-4o` (model 空欄時)、認証は
  `Authorization: Bearer`、キーは keyring のみ (Debug/log/error に非露出)。
  **配線:** `AiProviderKind::OpenAi` (`kind = "openai"`)、Add フォームの kind
  セレクタ ComboBox、Edit は kind 読み取り専用、kind 切替は `KindMismatch`
  (delete+add)。`build_provider_for_kind` が keyring から構築。**env
  (`DBBOARD_ANTHROPIC_*`) は Anthropic 専用のまま** — OpenAI は
  `ai-providers.toml` か Settings 窓で設定。i18n `ai-settings-kind-openai` 全 11
  ロケール。README の toml 例をフラット `kind` スキーマに修正 (旧 nested
  `[providers.kind]` は serde 実体と不一致だった)。**実地スモークは user 側ボール (下記)。**
- **✅ 論理リストア/インポート (PR #112, ADR-0051):** ツールバー **Restore…** で
  `.sql` を現接続へ流し込む (ADR-0049 backup の読み側)。core = 字句スプリッタ
  `split_statements` + sqlparser `classify_script` の 2 層 (他形式 `.sql` も受容、
  パース不能文は degrade-open)、`run_restore` が空ターゲットゲート + エンジン別
  トランザクション。Turso/Postgres = アトミック、**Aurora DSQL / D1 = per-statement
  fallback**。UI = `BackupState` 鏡写しの `RestoreState` + worker plumbing、
  進捗/確認/完了/失敗パネル。**空 DB 限定** = 既存テーブルありは強制確認 (merge/diff
  なし)。i18n 17 キー全 11 ロケール。全ゲート green。**実地確認は user 側ボール (下記)。**
- **✅ バックアップ警告閾値の設定化 (PR #110, ADR-0050):** メニューバー Theme 隣の
  **Backup** サブメニュー (`DragValue`、下限 1) で warn 閾値を変更でき、
  `ui-settings.toml` に保存され再起動後も保持。既定 500k は dbboard-core の定数に
  一本化 (dbboard-config は非依存の `Option<u64>`、`None`→アプリ層でフォールバック)。
  永続化を全て load-modify-save (`persist_ui_settings`) 経由にして theme↔閾値の
  clobber を防止。i18n 3 キー全 11 ロケール。rust-reviewer Approve。
- **✅ 論理バックアップ = dump-only (PR #108, ADR-0049):** クエリツールバーの
  **Backup…** で接続全体を 1 つの `.sql` にダンプ。SQLite 系 (Turso/D1) は
  `sqlite_master` 逐語 DDL、Postgres 系 (Neon/Supabase/Aurora DSQL) は catalog
  から DDL 再構築 (DSQL は FK/sequence 省略で degrade)。keyset ページングで
  ストリーム書き出し、preflight `COUNT(*)` が 500k 行超で warn-and-allow、進捗
  ウィンドウ (table/row カウンタ + % バー + Cancel = 部分ダンプ保持)、完了
  サマリが skip/truncate を表出。i18n 全 11 ロケール。**restore は将来 ADR。**
  md-business 用検証シート = `.claude/verification/adr-0049-backup.md` (33 ケース)。
  rust-reviewer Approve (LOW 2・非ブロッキング)、リリースゲート緑、cargo deny clean。
- **✅ DL ページ (GitHub Pages) 完了 (PR #104, ADR-0047):**
  https://meta-taro.github.io/dbboard/ が live。Pages workflow は `site/**` 変更を
  検知して develop merge で自動デプロイ。`.exe` = primary (塗り) / `.msi` =
  secondary (アウトライン) の 2 段ボタン (意図的、そのまま維持で user 合意)。
  in-app update 通知の「download page」リンクが実在するページに解決するようになった。
- **✅ 結果グリッド 2 機能を develop 着地 (実利用で発覚した moれ):**
  - **マルチカラムソート (PR #106, ADR-0048):** ヘッダークリックで昇順→降順→解除、
    Ctrl/Shift で第二・第三キー (最大 3)。順序ロジックは `dbboard-core::sort` に分離
    (UI にビジネスロジックを置かない規則)、`result.rows` は不変で行選択・インライン
    編集のインデックスを保持。core 10 + UI 9 テスト。
  - **MSI ショートカット (PR #105):** スタートメニュー + デスクトップ。非アドバタイズ
    型 (Shortcut + HKCU RegistryValue key-path + RemoveFolder)、ICE69 回避のため
    Binaries フィーチャに同居。アンインストールで削除。
- **▶ 今の user 側ボール:** (1) この chore doc-sync PR (`chore/post-pr114-doc-sync`)
  を push → PR 作成 → develop へマージ。(2) **OpenAI/ChatGPT の実地スモーク** =
  Settings 窓で `kind = openai` プロバイダを Add (実キー) → Use に切替 → AI パネルで
  ストリーミング逐次表示・Cancel・エラー本文が出ることを確認。model 空欄で `gpt-4o`
  既定、model 明示で上書き。keyring にキーが入り Debug/log に漏れないこと。(3)
  **restore の実地確認** (積み残し) = 空 DB への取り込み (Turso/D1/Postgres 系)、既存
  テーブルありでの強制確認モーダル、進捗/キャンセル (部分適用保持)、foreign
  `pg_dump`/`sqlite3 .dump` の取り込み、ADR-0049 backup で出した `.sql` の往復。(4)
  backup 側の実地確認も未消化なら継続 (D1/Supabase/DSQL・500k 警告・部分ダンプ)。(5)
  次の実利用摩擦テーマの選択 (下記 候補)。**検証シート = restore/backup とも
  md-business 用は「ちょい待ち」で保留中** (`.claude/verification/adr-0049-backup.md`
  の 33 ケースは既存、restore 用シートは未着手)。
- **MSI アンインストールの残留 (user 質問への回答済み):** MSI は exe/PATH/フォルダ/
  ARP エントリを削除するが、`%APPDATA%\dbboard\dbboard\` の設定ファイルと Windows
  資格情報マネージャーのエントリは残す (仕様どおり)。クリーンアップ手順は口頭提示済。
  README への明文化は未 (任意 follow-up)。
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
配布 (#14) は 2026-07-16 に完了済、v0.3.0 公開済、DL ページも live。今は
「配布済 exe を担当が実際に使うか」を update-check で観測しつつ、次の実利用改善
(下記の user 側ボール) を摩擦順に進めるフェーズ。直近は結果グリッドのソート漏れと
MSI ショートカット漏れを補完し、次いで maintainer 要望の**論理バックアップ
(ダンプ)** を ADR-0049 として実装・着地 (PR #108)。

---

## user 側のボール (= 次に着手する時の選択肢)

### ★ 候補 A: 実利用摩擦の次テーマ (menu-not-sequence)

直近 3 PR (DL ページ / ソート / MSI ショートカット) はいずれも実利用で挙がった
摩擦。次も同様に「実際に使って気づいた困りごと」を摩擦順に拾う。未着手候補は
Export results (CSV/JSON) / Saved queries / Schema diff (下記 候補 E)。新しい
write 経路を伴うものは着手前に ADR。

### 候補 A-2: README に MSI アンインストール残留の明文化 (小・任意)

MSI アンインストールは `%APPDATA%\dbboard\dbboard\` の設定と Windows 資格情報
マネージャーのエントリを残す (仕様)。ユーザに口頭で伝えた `cmdkey` +
フォルダ削除のクリーンアップ手順を README か `docs/` に明文化する小 chore。

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
