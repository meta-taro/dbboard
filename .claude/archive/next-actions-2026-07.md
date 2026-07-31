# アーカイブ — next-actions.md セッションログ (2026-07-24 〜 2026-07-29)

baseline §31 に基づく退避。`.claude/next-actions.md` が 475 行まで膨らんでいたため、
`## 最終更新` の日付エントリのうち古いものを**要約せず全文**ここへ移した。

- 退避日: 2026-07-31
- 退避元: `.claude/next-actions.md` の `## 最終更新` 内 2026-07-29 (MySQL アダプタ)
  以前の日付エントリ
- 退避前: 475 行 / 退避後: 約 300 行
- 退避しなかったもの: 直近 2 エントリ、`## モード` 以降の常設セクション全部
  (user 側ボール・接続名サニタイズ・web 側・メンテ規約)
- **注意:** これは当時の記録であり、現在の事実ではない。MySQL の read-only
  バックストップを `max_execution_time` 固定と書いている記述があるが、実装は
  ADR-0081 で probe 方式 (MySQL=`max_execution_time` ms / MariaDB=`max_statement_time`
  秒) に修正済み。

---

- 日付: 2026-07-29 (**MySQL / MariaDB アダプタが着地 — 初の「別 SQL 方言」エンジン
  (ADR-0068, commit `6b6e887`, branch `feature/desktop-design-polish`)。**
  仕事で MySQL を使う maintainer の要望 (#36) を**フルパリティ**で実装 (読み取り専用
  プレビューではない): 接続・クエリ・イントロスペクション・セル書き戻し・エクスポート・
  ダンプ・アトミックリストア・read-only MCP/AI 面・接続マネージャ UI の全バーティカル。
  **これまでの全アダプタは SQLite-wire か Postgres-wire の派生**だったが、MySQL は SQL
  テキスト自体が異なる初のエンジン = 新しい `SqlDialect::MySql` (バッククォート識別子・
  バックスラッシュ + クォート二重化エスケープ・DOUBLE の NaN/±Inf→NULL・`X'…'` blob)。
  read-only AST ガードと restore プランナは sqlparser の `MySqlDialect` に対応。
  **アダプタ `dbboard-mysql`** = sqlx の MySQL ドライバ上、dbboard-core のみ依存。秘匿な
  `MySqlConfig`、TLS 格上げ、エラー固定文字列化でパスワード漏洩防止。read-only は
  `SET TRANSACTION READ ONLY` + `max_execution_time`、restore はデータのみ INSERT の
  InnoDB 単一トランザクション。テキストプロトコルで値は `Value::Text`/NULL は `Value::Null`。
  **配線はコンパイラ誘導** = `ConnectionKind::MySql` → `BackendConfig::MySql` +
  `DBBOARD_MYSQL_URL` → egui/SvelteKit フォーム → Tauri DTO → MCP。serde タグ enum は
  `#[serde(rename = "mysql")]` 固定 (自動 `my_sql` 回避)。URL は OS キーチェーン格納。
  **TDD:** 方言ルール (core 単体) + アダプタ挙動 (mysql 単体) + config/connect 伝播 +
  live env-gated `mysql_roundtrip.rs` (`DBBOARD_MYSQL_URL`: connect/DML/SELECT・複合 PK
  describe・単一 + 複合 FK・read-only 切り詰め・10_000 行境界を 4 桁クロス結合で生成)。
  全ゲート green、pre-commit は**既知・良性の turso teardown segfault のみ** `--no-verify`
  (memory `env-windows-libsql-segfault`、PII 無し確認済み)。**docs も同梱** (ADR-0068・
  architecture クレートマップ/依存グラフ/env チェーン・README サポート DB・connections.md
  種別/env 列挙)。**今の user 側ボール = (1) `feature/desktop-design-polish` の push、
  (2) 初回 v0.4.0 リリース前に GitHub Actions シークレット `TAURI_SIGNING_PRIVATE_KEY` を
  生成済み minisign 秘密鍵で設定 (`_PASSWORD` は空) → scratchpad の鍵コピー削除。これが
  無いと `build-tauri-*` が署名できず失敗する。** **残作業:** v0.4.0 パリティ + MySQL 拡張は
  全完了。次に着手し得るのは新規書き込み面 (row insert/delete、両クライアント未実装) —
  ポートではないのでロードマップの選択事項。)
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
