# 次のアクション案内 (user 側ボール常設ファイル)

このファイルは「セッションが切れても、開いた瞬間に user 側のボールがわかる」状態を維持するためのもの。
セッション終了時 / 状況が変わった時に必ず更新する。**日本語可・内部用。**

---

## 最終更新

- 日付: 2026-08-03 (**identity 赤の解消 = ADR-0085 (PR #128 / #129)、および CI の
  denylist 層が初めて実稼働。** セッション開始時の §18 手順で `develop` の `pii-scan` が
  赤だったのが発端。中身は**本物 1 件と誤検出 1 件が重なっていた**。
  **本物** = GitHub の「Squash and merge」は web UI 側でコミットを作るので、この clone の
  `git config user.email` が noreply でも**アカウントのプライマリアドレスが author に入る**
  (PR #127 の squash `e15dcff`)。→ user が GitHub の Settings → Emails →
  **Keep my email addresses private** を ON (§15 = human 操作)。次の squash `d7ed16b` の
  author が noreply になったことで効果は実証済み。
  **誤検出** = 同じ `d7ed16b` の *committer* が `noreply@github.com` (GitHub 自身の web-flow
  アドレス)。`users.` 配下ではないので ADR-0084 の許可正規表現が弾いていた。**ADR-0084 が
  着地して以来 web マージのたびに出ていた**が、上の本物と同じ赤い X に見えるので
  **片方がもう片方を隠していた**。→ ADR-0085 で許可を全文 alternation にしてこの 1 個を追加。
  **`PII_DENYLIST` secret が存在しなかった** = 日次スキャンのログが
  `literal name detection off` で、**CI は実店舗名 3 件を含む `develop` を、clean だからでは
  なく照合対象を持っていなかったから緑にしていた**。§15 に従い手順のみ提示、user が作成 →
  run 30784716586 で `materialized from secret` を確認、BLOCKING 無し。
  **`develop` は green** (push run 30786499201 / 日次 run 30803841065)。
  **今の user 側ボール = (1) 公開済 468 コミットの履歴書き換えをやるかどうかの判断
  (やるなら**先に全ローカル作業を push してから** — 順序を間違えると未書き換えの
  ローカルコミットが即座に再汚染する。runbook に追記済)、(2)
  `feature/desktop-design-polish` の push、(3) v0.4.0 前に `TAURI_SIGNING_PRIVATE_KEY` 設定、
  (4) #42 = 外部 bastion 経由の live MySQL 検証 (**実接続 = 明示的な GO と認証情報が必要。
  エージェントは勝手に接続しない**)。**エージェント側の次候補** = `--denylist-digest`
  モード (ローカル `.pii-denylist` と CI secret の中身を sha8 で突き合わせる — 現状、
  貼り間違い・部分コピーでも CI は緑になる)、issue #130 (`dbboard-desktop` のフル再
  コンパイルで pre-push 約 4 分。原因確定は `CARGO_LOG` を先に取ってから)。)
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
  `671d805`、force-push 不要 = そのコミットは一度も push されていない)。`git log --all -S`
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
  Tauri 版が egui を追い越した (ADR-0069, commit `22892b6`, branch
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

### 候補 B: git 履歴の一括サニタイズ (human ボール・破壊的・未実行)

**1 回の rewrite で 2 つが同時に片付く。** どちらも「ファイルなら次のコミットで
直せるが、既に公開された過去コミットは書き換えないと消えない」もの:

1. **実店舗名** — 過去コミットに残る (実名は**非公開メモリと `.pii-denylist`
   のみ**。ここには書かない。対応表からローカルで `replacements.txt` を作る)。
   バイナリは CI ビルドで名前を含まないためリリースは塞がない。
2. **コミット identity** — 公開済 468 コミットの author/committer が個人 Gmail
   (ADR-0084)。**未公開のローカル 28 コミットは書き換え済** (2026-07-31、
   force-push 不要だったので実行した)。以後の新規コミットも noreply で clean。
   残るのは origin 上の分だけ。

手順は `docs/maintainer/history-sanitize-runbook.md` (Step 1-3 = 文字列置換、
Step 3b = `--mailmap` で identity、Step 4 = force-push)。全ハッシュ変更・既存
クローン/PR/フォーク破損のため **human 実行**。fork 0 / star 0 なので実効性は
ある = 検討する理由になるが、勝手に実行する理由にはならない。

**順序を間違えると全部無駄になる:** rewrite + force-push は**未 push の
ローカル作業を全部 push してから**やり、その後クローンを捨てて re-clone する。
先に rewrite すると、残った未書き換えのローカルコミットを次の `git push` が
そのまま remote に戻して再汚染する (git から見れば単なる新規コミットなので
警告も出ない)。`git pull --rebase` では直らない。runbook の「Ordering」節参照。
open PR **#125** (`feature/cjk-font-and-ai-menu`) は rewrite で壊れるので、
先に merge / close するか、書き換え後のブランチから立て直す。

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
