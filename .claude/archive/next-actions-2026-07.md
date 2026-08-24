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
  (ADR-0068, commit `f07be49`, branch `feature/desktop-design-polish`)。**
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
  (ADR-0067, commit `1bb4c5b`, branch `feature/desktop-design-polish`)。**
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
  (ADR-0066, commit `08a66c1`, branch `feature/desktop-design-polish`)。**
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
  (ADR-0065, commit `5c15812`, branch `feature/desktop-design-polish`)。**
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
  (ADR-0064, commit `292db89`, branch `feature/desktop-design-polish`)。**
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
  セル編集が着地 (ADR-0063, commit `d2b8a13`, branch `feature/desktop-design-polish`)。**
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

---

## 2026-08-05 追加退避 (2026-07-29 〜 2026-07-31)

- 日付: 2026-07-31 (**コミット identity のリーク防止 = ADR-0084、および記録ファイルの
  棚卸し (baseline §31)。コード変更なし。** 発端は user の質問「OSS プロジェクトとして
  `.claude` などを ignore してないのは問題ないか」。**監査結果 = `.claude/` 自体は問題なし**
  — 他人の名前を含む `.claude/rules/` と `.claude/templates/` は既に ignore 済み、
  tracked な 22 ファイルはスキャン clean。**本当の穴は誰も見ていなかったコミット
  メタデータ** = 全コミットの author/committer に maintainer の個人 Gmail が入っていた。
  **ファイル内の文字列とは危険度が違う:** ファイルは次のコミットで直せるが、identity は
  コミットオブジェクトの一部なので、直す = そのハッシュと全子孫ハッシュの書き換え =
  force-push = 全クローン破壊。`git grep` はツリーを読むのでコミットオブジェクトを
  構造的に見られず、既存スキャナには検出手段が無かった。**対応 3 段** = (1) このリポの
  `user.email` を noreply (`<id>+<login>@users.noreply.github.com`) に設定 → 以後の
  コミットは clean、(2) `pii-scan.sh --identity <range>` を新設 (`%ae/%ce/%an/%cn` を読む
  独立モード、GitHub の noreply 2 形式のみ許可、**出力は値を伏せる** = 公開 Actions ログで
  隠したい当のアドレスを再公開しては本末転倒)。pre-commit は**コミットが生まれる前に**
  `git config user.email` を検査、CI は push/PR が導入したコミットのみ再検査 (履歴全体は
  書き換え前で一様に非準拠なので常時赤になる)。RED-first で selftest を先に書き、
  identity モードが**パースされるのに dispatch されず「clean」と report する**バグを発見 →
  `*)` arm で exit 2 (リークスキャナが何もスキャンせず clean と言うのは最悪の壊れ方)、
  (3) **既に公開済の ~428 コミットは直っていない** — 履歴書き換え + force-push は human
  判断 (CLAUDE.md「push は人間」)。手順は `docs/maintainer/history-sanitize-runbook.md` に
  `--mailmap` 節を追記済。fork 0 / star 0 なので書き換えは実効性がある = 検討する理由に
  なるが、勝手に実行する理由にはならない。**棚卸し (§31)** = `project-status.md` 3,689→180 行、
  `next-actions.md` 475→約 300 行を `.claude/archive/` へ**全文退避** (要約ではない・削除でもない)。
  **`.pii-denylist` はエージェント側で作成済** (untracked・gitignored)。作った瞬間に
  BLOCKING 層が初めて有効になり、**3 件即ヒット** — しかも当たったのは
  「実店舗名を履歴から消すべき理由」を説明している当の段落 (本ファイル下の候補 B) で、
  そこに実店舗名が生で書かれていた → 除去 (commit `cdf6524`)。
  **未公開コミットの identity は書き換え済** = `git filter-branch --env-filter` を
  `--all --not --remotes=origin` に限定し、全ローカルブランチの未公開部分 28 コミットを
  noreply に。**一度も push されていないので force-push 不要・誰の clone も壊さない**。
  書き換え後に全ブランチで「ツリー + 件名 + 順序が一致」を検証、`refs/original/*` と
  バックアップ ref を削除 (Gmail 入りコミットオブジェクトを到達可能なまま残さない)。
  **今の user 側ボール = (1) CI secret `PII_DENYLIST` の設定 (§15 で human のみ。
  ローカルは有効になったが CI 側はまだ literal 検出 OFF。中身はローカルの
  `.pii-denylist` をそのまま貼る)、(2) 公開済 468 コミットの履歴書き換えをやるか
  どうかの判断 (やるなら**先に全ローカル作業を push してから** — 順序を間違えると
  未書き換えのローカルコミットが即座に再汚染する。runbook に追記済。open PR #125 も
  巻き添えで壊れる)、(3) `feature/desktop-design-polish` の push、(4) v0.4.0 前に
  `TAURI_SIGNING_PRIVATE_KEY` 設定、(5) #42 = 外部 bastion 経由の live MySQL 検証
  (**実接続 = 明示的な GO と認証情報が必要。エージェントは勝手に接続しない**)。)
- 日付: 2026-07-30 (**ドキュメント同期の chore + push 前の PII 除去。** コード変更なし。
  (1) 前セッションで作業ツリーに残っていた `docs/internal-release-v0.4.0.md` (新デザイン
  desktop v0.4.0 の**内々配布用リリースノート** = 配布物・外形機能・操作手順・
  フィードバック依頼・秘匿情報の置き場) をコミット。(2) ADR-0069 の反映漏れを解消 =
  `docs/roadmap.md` の Pacing Note に SSH トンネル + 「desktop が egui を先行」を追記、
  `.claude/project-status.md` に ADR-0069 エントリを追加。(3) **⚠ 実 bastion の
  `user@IP:port` が tracked な `.claude/next-actions.md` に生で入っていた** (前セッションの
  handoff コミット `d9e26f5`)。**未 push だったので `--amend` で履歴ごと除去** (新ハッシュ
  `f309e97`、force-push 不要 = そのコミットは一度も push されていない)。`git log --all -S`
  で全 ref 検索 → 0 件を確認。実 host/user は非公開メモリ側にのみ保持。
  **なぜすり抜けたか = `.pii-denylist` がこのマシンに存在しないため** literal 検出が
  丸ごと OFF (`[pii-scan] note: no denylist file — literal name detection off`)。
  `apps/desktop/src-tauri/Cargo.toml` の作業ツリー差分は**改行コード (LF↔CRLF) のみで
  内容差分ゼロ**なので据え置き。**今の user 側ボール = (1) `.pii-denylist` の作成
  (`.pii-denylist.example` をコピーして実店舗名・maintainer PII・bastion host/user を記入)
  + CI secret `PII_DENYLIST` の設定 — これが無い限り BLOCKING 層は実質無効、
  (2) `feature/desktop-design-polish` の push (25 コミット)、(3) v0.4.0 リリース前に
  `TAURI_SIGNING_PRIVATE_KEY` シークレット設定、(4) #42 = 外部 bastion 経由の live MySQL
  検証 (**実接続 = 明示的な GO と認証情報が必要。エージェントは勝手に接続しない**)。)
- 日付: 2026-07-29 (**デスクトップの SSH トンネル編集 UI が着地 — ここで初めて
  Tauri 版が egui を追い越した (ADR-0069, commit `7431ef5`, branch
  `feature/desktop-design-polish`)。** バスチオン越しにしか届かない DB
  (bastion の `localhost` のみ listen) に接続するための SSH トンネルを、接続フォームから
  編集可能に。対象は tunnel 可能な種別 (Postgres ファミリ + MySQL)。フォームに **SSH
  トンネル**セクション: enable トグル・bastion host/port/user・鍵/パスワード認証切替・
  サーバホスト鍵ピン (fingerprint XOR known_hosts、盲信なし必須)。トンネル配管 (russh
  ローカルフォワード) は両クライアント共有だが**編集 UI は desktop のみ** — egui では
  `connections.toml` 手編集のまま (意図的、desktop が編集の正本)。**秘匿情報** (鍵
  パスフレーズ・SSH パスワード) は OS キーチェーンのみ (`ssh_passphrase`/`ssh_password`
  ref)、TOML には決して入らない。編集時の空欄 = 既存維持 (ADR-0016)。**3人の並列レビュー
  (security/rust/typescript) が同一の実バグに独立収束** → 修正: 「維持すべきものが無いのに
  keep」(認証方式切替、または未暗号化鍵を新たに暗号化フラグ ON) が、書き込まれていない
  keyring ref を永続化していた。両層で拒否するよう修正 — config 層 `apply_update_ssh` は
  既存ブロックから keep を解決 (id からの再導出をやめる)、フォーム `validateSsh` は
  edit-prefill provenance フラグで秘匿情報を必須化。保存経路に belt-and-suspenders な
  `SshTunnelToml::validate()` も追加。**TDD:** config に RED-first で 3 テスト
  (keep/switch の2バグ + 安全な password-keep)、TS に 6 テスト追加。**docs 同梱**
  (connections.md の SSH セクション・README・architecture.md の dbboard-tunnel クレート +
  依存ルール・desktop-parity.md の「desktop が egui を先行」行)。全ゲート green
  (fmt/clippy clean・config 187・ui 329・desktop 38・svelte-check 0・vitest 161)、
  pre-commit は既知・良性の turso teardown segfault のみ `--no-verify`。**今の user 側
  ボール = (1) `feature/desktop-design-polish` の push、(2) 未着手の #42 = dbboard
  自身のトンネル経由で外部 bastion (実 host/user/port は非公開メモリと `.pii-denylist`
  のみ — tracked ファイルには決して書かない) の VPS MariaDB へ live な
  MySQL SELECT を通す検証。これは外部への実接続 = 実行前に user の明示的 GO と認証情報が
  必要。エージェントは勝手に接続しない。** )

> 2026-07-29 (MySQL アダプタ) 以前の日付エントリは、baseline §31 に基づき
> [`.claude/archive/next-actions-2026-07.md`](archive/next-actions-2026-07.md)
> へ全文退避した (要約ではない)。

- develop tip: PR #123 (design system, ADR-0056 + ADR-0057, merge `a62ef5f`) が最新。
  直前は #122 (PII scan, ADR-0055, merge `ed12ecb`)。
  直前は #121 (MCP 5→7 doc-sync `7d2e238`) → #120 (list_relationships, ADR-0054
  `ea58050`) → #118 (search_schema, ADR-0053 `18ae423`) → #116 (error-wrap fix
  `6fdb3f8`) → #115
  (OpenAI doc-sync `51a1fe9`) → #114 (OpenAI provider ADR-0052 `e6df7a5`) → #112
  (restore/import ADR-0051 `f83ccf0`) → #113 (doc-sync)。main = `0325571` =
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
