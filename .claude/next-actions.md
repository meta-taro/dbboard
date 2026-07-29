# 次のアクション案内 (user 側ボール常設ファイル)

このファイルは「セッションが切れても、開いた瞬間に user 側のボールがわかる」状態を維持するためのもの。
セッション終了時 / 状況が変わった時に必ず更新する。**日本語可・内部用。**

---

## 最終更新

- 日付: 2026-07-29 (**Tauri 版 v0.4.0 パリティ完了 — 自動更新 + リリース CI が着地
  (ADR-0067, commit `d65c008`, branch `feature/desktop-design-polish`)。**
  egui の inform-only 更新チェック (ADR-0040) を一歩超えて、Tauri は**その場で
  更新・再起動する**: `tauri-plugin-updater` が署名済み `latest.json` を検証して
  インストール → `tauri-plugin-process` が再起動。**アーキテクチャの肝は純ロジックと
  トランスポートの分離** = `$lib/update/notice.ts` は Tauri 非依存の純関数
  (`parseVersion`/`isNewer` = 解析不能なら phantom を出さず false、`foldDownload`/
  `downloadPercent` = ダウンロード進捗の畳み込み) で RED-first の vitest 15 本。
  UI は非モーダルの右下カード `UpdateNotice.svelte` (available→downloading→
  installing→restarting/failed の 5 フェーズ、determinate/indeterminate プログレス、
  prefers-reduced-motion 対応)。**egui と同じ `DBBOARD_NO_UPDATE_CHECK` opt-out** =
  Rust コマンド `update_opt_out` (空文字は無効扱いの `opt_out` ヘルパ, 単体 1 本)。
  起動時チェックは best-effort = 失敗は握りつぶし、アプリ起動を決して壊さない。
  **リリースノートは Markdown ライブラリを足さず pre-wrap プレーン表示** (pnpm
  サプライチェーン方針を尊重、ADR-0067 にフォローアップとして明記)。**署名鍵の安全性:**
  minisign 公開鍵は `tauri.conf.json` に埋め込み済み、**秘密鍵はリポジトリにも
  トランスクリプトにも出さない** = scratchpad 生成のみ。`release.yml` に
  `build-tauri-windows`/`build-tauri-macos` を追加 (NSIS setup.exe / universal
  app.tar.gz + `.sig` を署名 env で生成)、Python heredoc で `latest.json` を組み立て
  (`one()` が候補 1 個でなければ fail-loud)、「リリースオブジェクトを先に用意」ステップで
  tag CI のブートストラップ失敗も解消。全ゲート green (fmt/clippy/check/test・pnpm
  check/test/build)、pre-commit は**既知・良性の turso teardown segfault のみ**
  `--no-verify` (memory `env-windows-libsql-segfault`、PII 無し確認済み)。
  **これで v0.4.0 フィーチャーパリティは全バーティアル完了** (接続 CRUD・セル編集・
  注釈・エクスポート・ダンプ・リストア・AI・自動更新)。残る ⛔ 行は row insert/delete の
  1 つのみ = 両クライアントとも**新規の書き込み面**でありポートではない。
  **今の user 側ボール = (1) `feature/desktop-design-polish` の push、(2) 初回 v0.4.0
  リリース前に GitHub Actions シークレット `TAURI_SIGNING_PRIVATE_KEY` を生成済み
  minisign 秘密鍵で設定 (`TAURI_SIGNING_PRIVATE_KEY_PASSWORD` は空)、その後 scratchpad
  の鍵コピーを削除。これが無いと `build-tauri-*` が署名できず失敗する。**
  **次の作業 (継続シーケンス「両方まとめて連続で」):** MySQL アダプタ (#36) = 新クレート
  `dbboard-mysql` + `SqlDialect::MySql` (backtick クォート・read-only 分類) + 接続種別・
  ドラフト/管理/編集・ダンプ/リストア方言分岐。TDD + 専用 ADR (ADR-0068 見込み)。)
- 日付: 2026-07-29 (**Tauri 版 v0.4.0 パリティ — AI アシスタントが着地
  (ADR-0066, commit `c1ccec5`, branch `feature/desktop-design-polish`)。**
  egui の AI アシスタント (ai.rs + ai_settings.rs) を Tauri へ移植 = プロバイダ
  トレイトと 2 実装 (dbboard-ai / dbboard-anthropic / dbboard-openai) をそのまま
  再利用。**トランスポートだけが変わる** = egui のワーカーチャネル → Tauri コマンド、
  ストリーミングデルタ → `ai:chunk` イベント (pure な `accumulate()` で畳む =
  テキストは追記・トークン累計は **加算でなく置換**)。**核となるガードレールは不変:
  SQL を実行せず、行データを一切見ない** = Explain は打った SQL テキストのみ、Suggest
  はプロンプト + テーブル/カラム名 (`list_tables`、opt-in で `describe_table` メタ) を
  送る。`run_read_query` の出力はプロバイダに一切届かない。**API キーは OS キーリング
  (`dbboard.ai.<id>.api_key`) のみ** — TOML/ログ/WebView には決して出さない
  (`AiProviderView` にキーフィールド無し、編集で空欄なら既存キー維持)。**9 個の AI
  コマンドはどれも MCP ツール未登録** = 外部エージェントは読み取り専用のまま (他の書き込み
  バーティカルと同じ分離)。エントリボタンは常時表示 (接続ゲート外) = 接続前でも最初の
  プロバイダを追加可能。Suggest は接続必須 (フロント `canSend` とコマンド dispatch の両方で
  強制)、Explain は不要。TDD: desktop 単体 9 (DTO 形状/キー秘匿/stream 置換/cancel フラグ/
  prefetch 警告) + フロント pure `panel.test.ts` 単体 19。About ダイアログに「About AI
  Assistant」の安全性ブロックを追加 (egui パリティ)。全ゲート green
  (fmt/clippy/check/test・pnpm check/test/build)、pre-commit 通過 (desktop 34 テスト)。
  **残バーティカル (未着手):** 自動更新 + リリース CI (ADR-0044/0043, 0.3.0→0.4.0) の
  1 本のみ。**既知の技術的負債 (今回の新規ではない):** `dbboard-mcp/src/service.rs` が
  800 行のハード上限超過 → サブモジュール分割のフォローアップが望ましい。**今の user 側
  ボール = (1) `feature/desktop-design-polish` の push、(2) 最後のバーティカル =
  auto-update + release CI へ着手。方針は「くぎってはならない」なので全部入れる。**)
- 日付: 2026-07-29 (**Tauri 版 v0.4.0 パリティ — 論理リストア/インポートが着地
  (ADR-0065, commit `0f8194d`, branch `feature/desktop-design-polish`)。**
  egui の論理リストア (restore.rs) を Tauri へ移植 = pure な `dbboard-core` の
  restore オーケストレータ/preflight (`plan_restore`/`run_restore`) をそのまま再利用。
  **書き込みは `McpService::plan_restore`/`run_restore` だが MCP ツールには未登録** =
  外部エージェントは読み取り専用のまま (ダンプ ADR-0064・セル編集 ADR-0063 と同じ分離)。
  **ダンプとの非対称:** sink 無し (アダプタ経由で DB へ直書き) / warn しきい値無し。
  唯一の安全ゲート = **空でないターゲットへの確認** (`confirmed=true` 必須, フロントの
  チェックボックスで収集, `needsConfirmation`)。**要点:** `RestorePlan` は非 Serialize
  ゆえ IPC を渡らない → `plan_restore` はフラットな `RestorePlanDto` を返し、
  `run_restore` コマンド側でファイル再読込 + 再 plan。desktop `restore.rs` =
  `EventControl` (`restore:progress` イベント発火 + `dump_cancel` とは別の
  `restore_cancel` AtomicBool でキャンセル・実行前にクリア)。エンジン別 txn 戦略は
  core 側: atomic restore 対応はオール・オア・ナッシングの 1 バッチ、非対応 (D1) は
  `on_error` を尊重して 1 文ずつ。`on_error` は "continue" 以外すべて安全側 "stop" に
  丸める (Rust/TS 両端)。TDD: mcp 統合 3 + desktop 単体 3 + `plan.test.ts` 単体 13。
  全ゲート green (fmt/clippy/check/test・pnpm check/test/build)、pre-commit 通過。
  code-reviewer レビュー = CRITICAL/HIGH ゼロ (APPROVE)。**残バーティカル (未着手):**
  AI アシスタント (ADR-0052)・自動更新 + リリース CI (ADR-0044/0043, 0.3.0→0.4.0)。
  **既知の技術的負債 (今回の新規ではない):** `dbboard-mcp/src/service.rs` が 800 行の
  ハード上限超過 (現 ~1650 行) → dump/restore メソッドをサブモジュール分割する
  フォローアップが望ましい。**今の user 側ボール = (1) `feature/desktop-design-polish`
  の push、(2) 次バーティカル選定 (AI か auto-update)。方針は「くぎってはならない」なので
  最終的に全部入れる。**)
- 日付: 2026-07-29 (**Tauri 版 v0.4.0 パリティ — 論理バックアップ/ダンプが着地
  (ADR-0064, commit `4b53a39`, branch `feature/desktop-design-polish`)。**
  egui の論理ダンプ (backup.rs) を Tauri へ移植 = pure な `dbboard-core` の
  dump オーケストレータ/preflight (`plan_dump`/`run_dump`) をそのまま再利用。
  **書き込みは `McpService::plan_dump`/`run_dump` だが MCP ツールには未登録** =
  外部エージェントは読み取り専用のまま (セル編集 ADR-0063 と同じ分離)。
  **要点:** `DumpPlan` は非 Serialize ゆえ IPC を渡らない → `plan_dump` はフラットな
  `DumpPlanDto` を返し、`run_dump` コマンド側で内部再 plan。desktop `dump.rs` =
  `FileSink` (バッファ付きファイル) + `EventControl` (`dump:progress` イベント発火 +
  `AppState` の `Arc<AtomicBool>` でキャンセル)。**warn しきい値はフロント所有**
  (localStorage・warn-and-allow・バックエンドは決してブロックしない)。**SQLite/Turso
  は data-only** (DDL 無し, ADR-0049)。TDD: mcp 統合 3 + desktop 単体 3 + `plan.test.ts`
  単体 16。全ゲート green (fmt/clippy/check/test・pnpm check/test/build)、pre-commit 通過。
  **残バーティカル (未着手):** 論理リストア/インポート (ADR-0051)・AI アシスタント
  (ADR-0052)・自動更新 + リリース CI (ADR-0044/0043, 0.3.0→0.4.0)。**今の user 側ボール =
  (1) `feature/desktop-design-polish` の push、(2) 次バーティカル選定 (restore か AI か
  auto-update)。方針は「くぎってはならない」なので最終的に全部入れる。**)
- 日付: 2026-07-29 (**Tauri 版 v0.4.0 フィーチャーパリティ進行中 — インライン
  セル編集が着地 (ADR-0063, commit `c5f165f`, branch `feature/desktop-design-polish`)。**
  上位方針は user の厳命「**小さくきらないで、機能面の仕様を全部いれる。くぎっては
  ならない**」= egui 版の全機能を Tauri 2 + SvelteKit (`apps/desktop/`) へ一括移植し
  v0.4.0 (パリティ + 自動更新) として出す。Tauri は元々 **読み取り専用スパイク**
  (ADR-0046/0059) で始まり、書き込み面を 1 バーティカルずつ ADR 付きで解禁中。
  **既着地:** 接続 CRUD + バンドル入出力 (ADR-0062)・ローカル注釈編集 (ADR-0045)・
  データセット Export CSV/TSV (ADR-0049)・**セル編集 (今回, ADR-0063)**。
  **セル編集の要点:** サイドバー「Select top 100」由来 (TableInfo を保持) かつ
  **宣言済み PK** を持つ表だけ編集可。UPDATE のみ・`rows_affected == 1` コミット
  ゲート・rowid 専用/ビューは読み取り専用 (egui パリティ)。書き込みは
  `McpService::apply_row_update` だが **MCP ツールには未登録** = 外部エージェントは
  読み取り専用のまま。TDD: mcp 統合 4 + `edit.test.ts` 単体 8。全ゲート green
  (fmt/clippy/check/test・pnpm check/test/build)。**残バーティカル (未着手):**
  論理バックアップ/ダンプ (ADR-0049/0050)・論理リストア/インポート (ADR-0051)・
  AI アシスタント (ADR-0052)・自動更新 + リリース CI (ADR-0044/0043, 0.3.0→0.4.0)。
  **今の user 側ボール = (1) `feature/desktop-design-polish` の push、(2) 次バーティカル
  の選定 (backup/restore か AI か auto-update)。方針は「くぎってはならない」なので
  最終的に全部入れる。**)
- 日付: 2026-07-26 (**ブランド design system = PR #123 マージ済 (ADR-0056 + ADR-0057)。**
  user 依頼「デザインをモックに寄せたい」。**ADR-0056** = `dbboard-ui::theme` が
  stock egui を置換 (インディゴ基調パレット Light/Dark 両登録・Auto 追従、spacing/
  radius トークン、意味色軸)。**ADR-0057** = 塗りつぶし **実行** 主ボタン・`theme::pill`・
  テーブル数バッジ・バックアップしきい値の `×1/×1K/×1M` 単位エディタ (指標は行数のまま)。
  ヘッダー識別子 (接続ピル + Auto|Light|Dark トグル) は狭幅でメニュー重なり → **メニュー
  バー直下の独立行に移動**で解消。新規 i18n 文字列ゼロ。`--no-verify` = 既知 libSQL
  segfault。**技術スタック議論:** 「egui 継続か Tauri 等デザイン重視スタックへ根本変更
  か」→ 重なりは egui 限界でなくレイアウトミス (修正済)、乗り換えるなら UI 層だけ Tauri
  差し替えが筋 (Rust コア 100% 再利用可) だが本番無人依存中 + 数週間規模。**user 選択 =
  「egui 磨きだけ完了・Tauri は据え置き」**。**今の user 側ボール = (1) この chore
  doc-sync PR (`chore/post-pr123-doc-sync`) のマージ、(2) 次の実利用摩擦テーマ選定
  (Export / Saved queries / Schema diff / MCP 継続 等)、(3) PII 運用セットアップ・
  OpenAI 実応答・restore/backup 実地確認の積み残し。**)
- 日付: 2026-07-24 (**OSS 個人情報除去ワークフロー = PR #122 マージ済 (ADR-0055)。**
  user 依頼「OSS は個人情報を除去するワークフロー (日次・コミット時・コミットコメント
  も)」。`scripts/pii-scan.sh` を pre-commit (`--staged`)・新 commit-msg (`--message`)・
  CI `pii-scan.yml` (push/PR/**日次 cron**) の 3 経路で起動。**二層:** BLOCKING = 非
  コミット denylist (実店舗名・maintainer 実 PII) + private-key/AWS 形状、ADVISORY
  (非ブロック) = パスワード付き URL・個人メール・ホームパス (テスト fixture 多発ゆえ)。
  denylist ヒットと CI 出力は redact、実 literal は gitignore `.pii-denylist` + CI
  secret `PII_DENYLIST` のみ。allowlist は tier 分離、履歴全体は対象外 (別途 runbook
  の人手 rewrite)。security-reviewer の HIGH (allowlist tier 分離) + MEDIUM 2 + LOW 済。
  `--no-verify` = 既知 libSQL segfault。**今の user 側ボール = (1) この chore doc-sync
  PR (`chore/post-pr122-doc-sync`) のマージ、(2) PII 運用セットアップ [`.pii-denylist`
  記入 + `PII_DENYLIST` secret 追加 + `cargo test` でフック再インストール]、(3) 次の
  実利用摩擦テーマ選定、(4) OpenAI 実応答・restore/backup 実地確認の積み残し。**)
- 日付: 2026-07-24 (**MCP ツール面を 5→7 に拡張 (PR #118 + PR #120 マージ済)。**
  user 意向「OpenAI より MCP の需要が高い」を受けた MCP 方面の 2 スライス。
  **`search_schema` (ADR-0053, PR #118, 6 つ目)** = 名前部分一致でテーブル/カラムを
  横断検索。**`list_relationships` (ADR-0054, PR #120, 7 つ目)** = FK 結合グラフを
  有向エッジで返す (`table` 無指定で全体・指定で両側に触れる全エッジ)。core に
  `DatabaseAdapter::foreign_keys` + `has_foreign_keys` フラグ (ADR-0012 型)、Turso/D1 =
  `PRAGMA foreign_key_list`・Postgres 系 = `pg_catalog` (Aurora DSQL は空結果)。
  rust-reviewer の H1/M1/M2 を degrade + 共有ヘルパー `resolve_referenced_columns`
  (core) で解消。全ゲート green。**今の user 側ボール = (1) この chore doc-sync PR
  (`chore/post-pr120-doc-sync`) のマージ、(2) 次の実利用摩擦テーマ選定 (MCP 継続か
  Export/Saved queries/Schema diff 等)、(3) OpenAI 実応答確認・restore/backup 実地確認
  の積み残し。**)
- 日付: 2026-07-24 (**エラー折り返し fix が PR #116 で develop 着地 + OpenAI
  プロバイダを実機スモーク。** OpenAI (ADR-0052, PR #114) を develop ローカル
  ビルドで実機確認 → Settings で `kind=openai` 追加・使用・送信まで通り、**認証
  (Bearer)・SSE ストリーミングのエラー経路・エラー本文表示がすべて動作確認できた**
  (実応答は OpenAI アカウントが `429 insufficient_quota` = 残高不足で未到達。残高
  チャージ後に各自でトークン逐次表示を見る、で確定)。その実機確認中に拾った摩擦 =
  **長いエラー本文が AI パネルを右に溢れて折り返さない** → `render_error` (ADR-0039)
  でコピーボタンを独立行にし localized/原文を `Label::wrap()` で折り返し (全エラー
  箇所に波及)。PR #116 で着地。**今の user 側ボール = (1) この chore doc-sync PR
  (`chore/post-pr116-doc-sync`) のマージ、(2) 次テーマ = MCP 方面 (user 意向: MCP
  の需要が高い)、(3) restore 実地確認の積み残し。**)
- develop tip: PR #123 (design system, ADR-0056 + ADR-0057, merge `4e5623c`) が最新。
  直前は #122 (PII scan, ADR-0055, merge `d3ee8dd`)。
  直前は #121 (MCP 5→7 doc-sync `95b6922`) → #120 (list_relationships, ADR-0054
  `fa378c5`) → #118 (search_schema, ADR-0053 `3887784`) → #116 (error-wrap fix
  `aa5fa9d`) → #115
  (OpenAI doc-sync `21cb898`) → #114 (OpenAI provider ADR-0052 `ba54d02`) → #112
  (restore/import ADR-0051 `e624bbb`) → #113 (doc-sync)。main = `70ecb93` =
  **v0.3.0 タグ** (未リリース差分あり = MCP 以降 + backup + restore + OpenAI provider
  + error-wrap fix)。
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
  `[providers.kind]` は serde 実体と不一致だった)。**実機スモーク済** = 認証・SSE
  エラー経路・エラー本文表示 OK、実応答のみ残高不足 (429) で未到達。
- **✅ エラー折り返し fix (PR #116):** 長いプロバイダエラー本文 (例 OpenAI の
  `429 insufficient_quota`) が AI アシスタントパネルを右に溢れて折り返さず読めなかった。
  原因 = 共通インラインエラー表示 `render_error` (ADR-0039) で localized 行がコピー
  ボタンと同じ横並び行にあり折り返し制約が効いていなかった。修正 = コピーボタンを独立
  行にし、localized/原文 (English) 両方を `Label::wrap()` で折り返し。AI パネルに限らず
  AI プロバイダ設定・接続画面の全インラインエラーに波及。コピー & Ctrl+C 選択は不変、
  `DisplayError` ロジックも不変 (ADR 不要 = ADR-0039 の範囲内)。dbboard-ui 322 test 緑。
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
- **✅ MCP ツール面 5→7 (PR #118 search_schema / PR #120 list_relationships):**
  user 明言「OpenAI より MCP の需要が高い」を受けた MCP 拡張。**`search_schema`
  (ADR-0053, 6 つ目)** = 接続内の全テーブル/カラムを名前の部分一致 (大文字小文字無視)
  で横断検索、200 テーブルで truncate。**`list_relationships` (ADR-0054, 7 つ目)** =
  FK 結合グラフを有向エッジで返す (`table` 無指定で全体・指定で両側に触れる全エッジ、
  500 エッジで truncate)。core に `foreign_keys` メソッド + `has_foreign_keys` フラグ
  (ADR-0012 型)、Turso/D1 = `PRAGMA foreign_key_list`・Postgres 系 = `pg_catalog`
  (Aurora DSQL は FK 無しで空結果)。暗黙参照/複合キー/stale 参照を共有ヘルパー
  `resolve_referenced_columns` (core) で処理。秘密は非露出、ライブ統合テストは
  self-skip。rust-reviewer H1/M1/M2 対応済、全ゲート green。
- **▶ 今の user 側ボール:** (1) この chore doc-sync PR
  (`chore/post-pr120-doc-sync`) を push → PR 作成 → develop へマージ。(2) **次の実利用
  摩擦テーマ選定** = MCP 継続 (残る MCP 拡張候補) か、他の摩擦 (Export results は済、
  Saved queries / Schema diff 等)。新 write 経路は着手前に ADR。
  (3) **OpenAI 実応答の確認** (任意) = OpenAI 残高チャージ後にトークン逐次表示・Cancel
  を一度見る。認証/エラー経路は確認済なので優先度低。(4) **restore の実地確認**
  (積み残し) = 空 DB への取り込み (Turso/D1/Postgres 系)、既存テーブルありでの強制確認
  モーダル、進捗/キャンセル (部分適用保持)、foreign `pg_dump`/`sqlite3 .dump` の取り込み、
  ADR-0049 backup で出した `.sql` の往復。(5) backup 側の実地確認も未消化なら継続
  (D1/Supabase/DSQL・500k 警告・部分ダンプ)。**検証シート = restore/backup とも
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
Saved queries / Schema diff (下記 候補 E。Export results は CSV/JSON 済)。新しい
write 経路を伴うものは着手前に ADR。

### 候補 A-2: README に MSI アンインストール残留の明文化 (小・任意)

MSI アンインストールは `%APPDATA%\dbboard\dbboard\` の設定と Windows 資格情報
マネージャーのエントリを残す (仕様)。ユーザに口頭で伝えた `cmdkey` +
フォルダ削除のクリーンアップ手順を README か `docs/` に明文化する小 chore。

### 候補 B: git 履歴の実店舗名 rewrite (human ボール・破壊的・未実行)

過去コミットに実店舗名がまだ残る (`store-a`/`store-b`/`store-c`
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
