# プロジェクトステータス (内部メモ・日本語)

このファイルは作業中のセッション状態を記録する内部用ドキュメント。
外部公開向けの内容ではないため日本語で書く。セッション終了時に更新する。

## 最終更新

- 日付: 2026-06-23 (PR #27 マージクローズ、`dbboard-ui` AI panel
  slice (b) + 11-locale Fluent + docs sweep shipped / ADR-0023
  Phase 4 Stage 1 = issue 0005 完了)
- ブランチ: `develop` (= `c86424a`)、ローカル
  `chore/post-pr27-doc-sync` 作業中 (`feat/ai-panel-slice-b` は
  merge 済 / origin 側も削除済)
- 現在の Phase: **Phase 2 + 2.5 + 3 + Phase 4 Stage 1 = ADR-0023
  全 4 implementation PR shipped (trait crate / Anthropic provider /
  apps env-var wiring / dbboard-ui AI panel)。issue 0005 は PR #27
  でクローズ。Phase 4 Stage 2 (Settings UI / 永続化キー /
  streaming / multi-provider switcher / DDL extraction /
  function-calling / AI 履歴記録) は ADR-0023 §9 通り deferral 継続、
  次セッションは新規 ADR + 新規 issue で開く想定。Phase 2 ADR-0024
  at-rest hardening (PR #25, 2026-06-22) もそのまま load-bearing。**

### PR #27 (`dbboard-ui` AI panel slice (b) + 11-locale Fluent + docs sweep / ADR-0023 issue 0005) マージクローズ (本セッション / 2026-06-23)

- PR #27 (`feat/ai-panel-slice-b` → `develop`) マージ済 = `c86424a`
  (mergedAt 2026-06-23T02:31:38Z = JST 11:31)。
- ローカル `develop` は `origin/develop` (= `c86424a`) と
  fast-forward sync 済 (`409fa54..c86424a`、5 commit ぶん advance:
  feat commits `e56d58d` + `1ba5660` + `a676ea7` + 前段 chore
  `0eaab59` (PR #26 = post-PR25-doc-sync) + merge commits)。
- マージ済 feature ブランチ (`feat/ai-panel-slice-b`) は local /
  remote 両方削除済。本 chore (`chore/post-pr27-doc-sync`) は
  `develop` ベース、本セッションで切り直し。
- 本 PR の scope: ADR-0023 Phase 4 Stage 1 の最後のスライス
  (slice (b))。21 ファイル / +1221 / −87。中身:
  - `crates/dbboard-ui/src/ai.rs` (新規): `AiPanel` 構造体 =
    `egui::Window` 形式の AI パネル。`AiMode::{Explain, Suggest}`
    トグル、`input: String`、`busy: bool`、`last_response:
    Option<Result<AiResponse, String>>`。`prepare_send(dialect:
    Option<String>, schema: &[TableInfo]) -> Option<Command>` で
    送信時 `Command::AiExplain` / `Command::AiSuggest` を生成
    (Suggest アームでのみ `schema.to_vec()`)。`on_response` /
    `on_error(&AiError)` で busy 解除 + 結果反映。11 unit tests
    (open/close/toggle, mode switch, empty/whitespace noop, explain,
    suggest + schema, send-while-busy noop, on_response, on_error,
    fresh-replaces-stale, `ai_error_display` の 5 variant カバー)。
  - `crates/dbboard-ui/src/worker.rs`: `Command::AiExplain` /
    `Command::AiSuggest` 受け取り → `tokio::runtime::Handle::block_on`
    で `provider.explain` / `provider.suggest_sql` を駆動 →
    `Reply::AiResponded { text, tokens_in, tokens_out }` /
    `Reply::AiFailed { error }`。`ai_provider == None` は防御で
    `Reply::AiFailed { AiError::Configuration }`。5 dispatch tests
    (explain success / suggest success / provider error /
    no-provider Configuration 失敗 / unchanged SwitchConnection
    smoke)。ADR-0020 + ADR-0022 の switcher と同じ runtime pattern。
  - `crates/dbboard-ui/src/lib.rs`: `DbboardApp` から `AiPanel` を
    保有、`Reply::AiResponded` / `Reply::AiFailed` を受信ループに
    配線、`self.tables.as_ref().map_or(&[], Vec::as_slice)` で
    per-frame の `Vec<TableInfo>` clone を排除し slice 越しに渡す。
  - `apps/dbboard/src/main.rs`: メニューバーの AI ボタンを
    `app.has_ai_provider()` で gate (provider が None の時はボタン
    ごと不在 = ADR-0023 Decision 11 の graceful degradation =
    absence)。
  - `crates/dbboard-i18n/i18n/<locale>/dbboard.ftl` × 11 ロケール
    (en/ja/ko/zh-CN/zh-TW/de/fr/es/pt-BR/ru/it): `ai-menu-button` /
    `ai-panel-title` / `ai-mode-explain` / `ai-mode-suggest` /
    `ai-input-explain` / `ai-input-suggest` / `ai-send-button` /
    `ai-busy` / `ai-empty` / `ai-error-prefix-{configuration,
    network,provider,quota,cancelled}` (= 5 AiError variant) の
    新キー群を翻訳。ADR-0022 Consequences の "Tier 1 + Tier 2 を
    同期させる" ルールに準拠。
  - Docs sweep: `docs/architecture.md` (AI layer 段落に slice (b)
    の worker block_on + Reply 配線 + hide-on-absence を追記)、
    `docs/roadmap.md` (Phase 4 bullets を slice (b) shipped に
    ティック + Stage 1 exit criteria 達成、Stage 2 = ADR-0023 §9
    参照)、`README.md` (AI integration subsection を panel surface
    も含む形に書き換え)、`crates/dbboard-anthropic/README.md`
    (Configuration section の "sibling PR" 文言を削除して slice (b)
    完了済の表現に書き換え、Decision 11 を再参照)、`.claude/issues/0005-*.md`
    の全 acceptance ボックスをチェック。
- コミット粒度: 3 commits (CLAUDE.md "small focused chunks per
  logical change" に従い、(コード) / (翻訳) / (ドキュメント) に分離):
  - `e56d58d` `feat(ui): wire AI Explain/Suggest panel into the
    desktop worker (ADR-0023)`
  - `1ba5660` `feat(i18n): translate AI panel keys for slice (b)
    across all 11 locales`
  - `a676ea7` `docs: tick slice (b) acceptance and refresh AI
    panel surface (ADR-0023)`
- 検証: mandatory verification 4 コマンド (`cargo fmt --all --
  check` / `cargo clippy --all-targets --all-features -- -D
  warnings` / `cargo check --all-targets --all-features` /
  `cargo test --all-features`) と release build / release test、
  いずれもグリーン。cargo-husky の pre-commit / pre-push が同セット
  を各 commit / push 直前に再実行。本 PR で追加された
  `dbboard-ui` 側テスト = `ai.rs` 11 + `worker.rs` 5 = 16 件。
- レビュー: rust-reviewer 実施。
  - **MEDIUM #1**: `AiPanel::on_error(&mut self, error: AiError)`
    が `AiError` を value で受けるが consume するだけ。
    → 修正: `on_error(&AiError)` に変更 (clippy
    `needless_pass_by_value` 同時解消)、call site 5 箇所更新。
  - **MEDIUM #2**: `ui()` 内で `Vec<TableInfo>` を per-frame clone
    していた。Suggest アームが実際に send されるまで必要ない。
    → 修正: `ui(dialect: Option<&str>, schema: &[TableInfo])`
    に変更、`prepare_send(schema: &[TableInfo])` 内の Suggest
    アームでのみ `schema.to_vec()`。`lib.rs` 側は
    `self.tables.as_ref().map_or(&[], Vec::as_slice)` で
    pass-through (一瞬 `clippy::map_unwrap_or` に当たり
    `map_or` に書き換え)。
  - HIGH / CRITICAL: 0 件。
- 着地中に当てたミニ事象:
  - **clippy 4 件で task #32 ブロック**: rust-reviewer fix 後の
    再実行で `needless_pass_by_value` ×2 (`ai.rs` の `on_error` +
    `dialect: Option<String>`) + `semicolon_if_nothing_returned`
    (`worker.rs:76` の `run_worker(...)` 末尾 `;`) +
    `items_after_test_module` (`worker.rs:203` の
    `#[cfg(test)] mod tests` を `execute()` の前ではなくファイル
    末尾に移動)。一括修正後クリーン。
  - `dialect: Option<String>` → `Option<&str>` 化に伴い `lib.rs`
    側で `let dialect: Option<&str> = None;` を per-frame に
    定義 (Stage 1 では dialect 取得経路がまだ無く、Stage 2 で
    loopback server からの adapter id 解決を入れる予定; `ui()`
    と `prepare_send()` のシグネチャはその時に値が入る形)。
- web 影響: なし。Phase 4 Stage 1 全体が in-process / 環境変数
  経由ベースで HTTP contract も history JSON schema も
  unchanged (ADR-0023 §9 で Stage 1 は wire に出さない方針が確定)。
  `dbboard-web-state.md` メモを slice (b) shipped に書き換え済。
  web 側に mirror brief 不要。
- issue 0005 ステータス: `.claude/issues/0005-*.md` を closed に
  更新。slice (a) = trait crate (PR #20, 2026-06-15) + Anthropic
  provider (PR #22, 2026-06-15) + apps env-var wiring (PR #24,
  2026-06-17)、slice (b) = `dbboard-ui` AI panel (PR #27,
  2026-06-23)。Stage 1 全 acceptance box チェック済。
- 次セッション分岐候補:
  - **Phase 4 Stage 2 ADR (新規)**: ADR-0023 §9 defer の中から
    1 つ選んで新 ADR + 新 issue 起票。優先度の自然な順は
    Settings UI + `ai-providers.toml` + OS keychain 永続化 →
    multi-provider switcher → streaming → function-calling →
    full-DDL schema snapshot → ADR-0017 拡張 (AI 履歴記録)。
    Settings UI が一段目だと環境変数依存から脱却できる。
  - **新規 connector の追加**: Phase 3 で flavored kind 化した
    Postgres-wire 系の延長で、PlanetScale (Vitess) /
    CockroachDB / TiDB / Yugabyte 等。トライ前に
    `dbboard-core` Capabilities / `DbError` taxonomy に
    dialect-specific 概念が不足していないか軽く再点検。
  - **web 同期 / coordination**: 現状 contract も history schema
    も unchanged のまま slice (b) shipped、web 側にも mirror
    不要。次の coordination trigger は `feat(contract)` commit
    or `v: 2` history bump or Tier 2 i18n lift。

### PR #25 (at-rest file permissions / ADR-0024) マージクローズ (前セッション / 2026-06-22)

- PR #25 (`feat/secure-fs-permissions` → `develop`) マージ済 =
  `5590996` (mergedAt 2026-06-22T05:52:56Z)。
- ローカル `develop` は `origin/develop` (= `5590996`) と
  fast-forward sync 済 (`6ad670d..5590996`、5 commit ぶん advance:
  feat commits `36daa95` + `9e26456` + `ef1380b` + `47ed5c4` +
  merge commit `5590996`)。
- マージ済 feature ブランチ (`feat/secure-fs-permissions`) は
  origin 側で自動削除済、ローカルは `47ed5c4 [origin/...: gone]`
  状態で残置 (clean-up は次セッション or 任意)。
- 本 PR の scope: at-rest secret 保護 + cloud-sync 警告 + ADR-0024
  策定の 8 ファイル / +690 / −22。きっかけはユーザの「PC 紛失したと
  想定したセキュリティ check + 必要なら実装」要望。スコープは
  audit 結果に基づき (a) at-rest secret 保存と (b) in-memory 漏洩
  経路に絞り、env var docs と history.jsonl 中身フィルタは scope 外。
- セッション流れ:
  - **Audit phase**: security-reviewer agent を at-rest + in-memory
    scope で走らせ、3 件発見:
    - (HIGH→LOW) `connections.toml.tmp` が umask デフォルトで
      作られていた → 再評価で LOW (TOML 自体はシークレットを含まず
      keyring_*_ref のみ、`directories::config_dir()` も他ユーザ
      readable な場所ではない)。
    - (MEDIUM) `history.jsonl` が umask デフォルト = 通常 `0o644`
      で landing。Linux multi-user で他アカウントから読める。
    - (LOW) Windows の OneDrive Known Folder Move で
      `%APPDATA%\Roaming\` が `%OneDrive%\` 配下に飛ぶケース、
      `history.jsonl` が暗黙的にクラウド同期される。
  - **Fix scope 決定**: ユーザ確認で「MEDIUM + LOW (defense-in-depth)
    + OneDrive doc」→ `unsafe_code = "forbid"` 制約に当たり再確認
    → 「Unix 0o600 + Windows は継承 ACL 依存 + OneDrive 警告 +
    ADR-0024」で確定。`SetNamedSecurityInfoW` 経路は `windows-sys`
    の `unsafe` ブロックが必要なため却下、`icacls.exe` shell-out も
    locale 依存で却下、`%LOCALAPPDATA%` migration も Windows-only
    分岐の保守コスト > リターンで却下。
  - **TDD 実装**: secure_fs Red → Green。`create_new_user_only` /
    `open_append_user_only` / `is_likely_cloud_synced_path` の
    3 公開関数。`#[cfg(unix)]` で `OpenOptionsExt::mode(0o600)`、
    `#[cfg(not(unix))]` は parent DACL 継承に委ねる。15 tests
    (Unix-gated 3 + 共通 12; rebase 前は 12 だったが reviewer
    指摘で Google Drive 系を +3)。
  - **配線**: `store.rs::write_new_file` と
    `history.rs::append_record` を secure_fs に切替。
    `apps/dbboard::default_path` で起動時に
    `is_likely_cloud_synced_path` を呼び、hit したら stderr
    1 行 warning (panic / exit はせず継続)。
  - **Docs sweep**: ADR-0024 を `docs/decisions.md` に追記
    (Context / Decision / Alternatives / Consequences)、README の
    Security checks 段落に at-rest 姿勢を追記、`docs/connections.md`
    に新 section "File permissions and at-rest posture (ADR-0024)"
    を追加 (Unix 0o600 / Windows 継承 ACL / cloud-sync 警告 /
    BitLocker 推奨 / keychain 不変動の 5 点)。
- レビュー: rust-reviewer + security-reviewer 並列実行。
  - rust-reviewer **HIGH (BLOCK)**: `open_append_user_only` の
    drop→reopen に Unix で symlink-substitution TOCTOU。
    → 修正: `O_CREAT|O_EXCL|O_APPEND|mode(0o600)` の単一 atomic
    open に書き換え、返却 fd は作成時のものそのまま。close-and-reopen
    window を完全に消した。
  - rust-reviewer **MEDIUM**: cloud-sync matcher が Google Drive
    for Desktop の `My Drive` / macOS `CloudStorage` / `GoogleDrive-*`
    を見逃す。→ 修正: matcher に追加 + テスト 2 件追加。
  - rust-reviewer **LOW** (doc 文言): 修正済 (atomic open の文言に
    更新、heuristic 限界を明記)。
  - security-reviewer は rust-reviewer H1 と同じ問題を MEDIUM A で
    指摘、修正後に閉じた。MEDIUM B (chmod→open TOCTOU) は脅威モデル
    外として ADR と doc コメントで明記。LOW (NTFS junction false
    negative / stderr username / ADR 文言) も heuristic 限界 / 妥当な
    判断としてドキュメント化。
- 検証: pre-commit hook (fmt / clippy `-D warnings` / check / test)
  全 4 コミットで green、workspace 全テスト緑 (dbboard-config 15
  unit / dbboard-ui 67 unit 含む)。pre-push release build + release
  test も human 側で green 確認済 (push 通過実績)。
- 着地中に当てたミニ事象:
  - **rustfmt diff** 2 件: secure_fs.rs の multi-line chain →
    1 行 / multi-line let assignment → 1 行、`cargo fmt --all`
    で自動修正。
  - **clippy err_expect**: `.err().expect(...)` を `.expect_err(...)`
    に置換。
  - **clippy doc_markdown**: doc コメントの "OneDrive" を
    backtick で囲む。
  - **`unsafe_code = "forbid"` 制約**: 当初 windows-sys 直 DACL も
    検討したが workspace 全体の `unsafe` 禁止に直撃。ADR-0024
    Alternatives で正式に却下を記録し、Windows は inherited ACL
    依存 + ADR で reopen 条件を明示する形に落とした。
- web 側への影響: **HTTP contract / JSON schema 変更なし**。secure_fs
  は desktop binary 内のファイルシステム層、web 側 (Nuxt + NestJS)
  には対応物が存在しない (web 側は browser 環境で sqlite ファイル
  を扱わない)。web 側 mirror brief 不要。
- 次セッション分岐候補:
  - **issue 0005 slice (b)** = `dbboard-ui` AI panel + worker
    `Command::AiExplain` / `Command::AiSuggest` + 11-locale Fluent
    + 状態機械テスト。本セッションで audit のため一時中断したスコープ。
  - **`feat/dbboard-ui-ai-panel` ローカル空ブランチ** が `6ad670d`
    時点で残置中。slice (b) 着手時に rebase or 削除 + 切り直し。

### PR #24 (apps/dbboard AI 起動配線) マージクローズ (前セッション / 2026-06-17)

- PR #24 (`feat/apps-dbboard-ai-wiring` → `develop`) マージ済 =
  `6ad670d` (mergedAt 2026-06-17T04:03:12Z)。
- ローカル `develop` は `origin/develop` (= `6ad670d`) と
  fast-forward sync 済 (`1459899..6ad670d`、2 commit ぶん advance:
  feat commit `481c667` + merge commit `6ad670d`)。
- マージ済ローカル feature ブランチ (`feat/apps-dbboard-ai-wiring`
  = `481c667`) は `git branch -d` 済。
- 本 PR の scope: `apps/dbboard` 起動経路 + `dbboard-ui::DbboardApp`
  シグネチャ拡張 + README + issue 0005 ティック の 7 ファイル /
  +204 / −6。中身:
  - `apps/dbboard::resolve_ai_provider`: 新ヘルパー。
    `DBBOARD_ANTHROPIC_API_KEY` を読み取り、未設定 / trim 後空白なら
    silent `None`。設定済みなら `DBBOARD_ANTHROPIC_MODEL` (任意、空
    文字なら無視) を見て `AnthropicProvider::new` or
    `with_default_model` を呼び分け、失敗時は stderr へ
    `"dbboard: AI provider init failed, AI panel disabled: <err>"` を
    出して `None`。`dbboard_i18n::init` / `install_cjk_font` と同じ
    "optional layer が壊れても起動を bricks しない" パターン。
  - `dbboard-ui::DbboardApp`: 新 field `ai_provider:
    Option<Arc<dyn AiProvider>>` + 新 accessor `has_ai_provider()
    -> bool`。`connect()` / `new()` 両方に `ai_provider` 引数を
    追加 (6 args → 7 args)。`AiProvider` を `dbboard_ai` から
    re-export (`DbError` と同じ "binary が trait crate 直接 dep
    せず済む" 配慮)。
  - 既存 lib テスト (`build` / `build_with_persistent`) を
    `None` 渡しで signature 一致に追従、`build_with_ai_provider`
    helper + tiny `StubAiProvider` (5 メソッド) + 2 つの新規 test
    (`has_ai_provider_is_false_when_none_was_injected` /
    `has_ai_provider_is_true_when_some_was_injected`) で round-trip
    を保証。`async-trait` を `dbboard-ui` の dev-dep に追加
    (`#[async_trait]` decoration が要るため; production code は
    `Option<Arc<dyn ...>>` を握るだけで impl しない)。
  - `apps/dbboard/Cargo.toml`: `dbboard-ai` + `dbboard-anthropic`
    を deps に追加。`crates/dbboard-ui/Cargo.toml`: `dbboard-ai`
    deps + `async-trait` dev-dep。
  - `README.md`: `## Run` の最後に新 subsection
    "### AI integration (optional)" を追加。env var 表
    (`DBBOARD_ANTHROPIC_API_KEY` = required gate、
    `DBBOARD_ANTHROPIC_MODEL` = default `claude-sonnet-4-6`)、
    graceful-degradation 説明 (key 未設定なら panel hidden、key
    は process メモリのみで `Debug` / `history.jsonl` には出ない)、
    ADR-0023 §9 Stage 2 deferrals (streaming / multi-provider
    switcher / keychain `ai-providers.toml` / history mirror /
    full-DDL / function-calling) への link。
  - `.claude/issues/0005`: `apps/dbboard wiring` の 3 つを `[x]`
    化、各 item に "where it landed" コメント追加。残り
    `dbboard-ui` 5 項目 + `Documentation` 4 項目 (10 項目中
    `dbboard-anthropic/README.md` のみ既に `[x]`) は slice (b)
    送り。
- 検証: pre-commit hook (fmt / clippy `-D warnings` / check /
  test) 全 green、workspace 全クレートテスト緑 (dbboard-ui
  89 unit, 新規 2 件含む / dbboard-ai 15 / dbboard-anthropic
  24 unit + 7 wiremock 統合 / 他 既存)。pre-push hook の
  release build + release test も human 側で green を確認済
  (push 通過実績)。
- 着地中に当てたミニ事象: なし (clippy / fmt 一発緑、
  signature 追加だけ呼び出し側全箇所通る形だった)。
- web 側への影響: **HTTP contract / JSON schema 変更なし**。AI
  呼び出しは `dbboard-server` を介さず in-process (ADR-0023
  Decision 3)、env var も desktop only。web 側 mirror brief 不要
  (ADR-0023 trait crate / 具象 provider と同じ desktop-side-only
  カテゴリ継続)。
- 次セッション分岐候補 (slice (b) = issue 0005 残り 1 段):
  - **`dbboard-ui` AI panel + worker** — `egui::Window` を menu
    bar から toggle、`has_ai_provider()` true 時のみ register。
    `Command::AiExplain { sql }` / `Command::AiSuggest { prompt,
    schema }` / `Reply::AiResponded { text, tokens_in, tokens_out
    }` / `Reply::AiFailed { err }` を worker に追加。
    `tokio::runtime::Handle::block_on` パターンは
    `ConnectionSwitcher` と同型 (`AiProvider` は `Arc<dyn>` で
    worker thread に shared、async は worker runtime 上で走らせる)。
    UI state machine の単体テスト (mode switch / send while busy
    は noop / response が stale を置換 / error が stale を置換)。
  - **Fluent keys 11 locale 全件** — panel title / 2 mode label
    (Explain / Suggest) / send button / 5 error category prefix
    (AiError 5 variant) を Tier 1 (en / ja) → Tier 2 (ko / zh-CN
    / zh-TW / de / fr / es / pt-BR / ru / it) の順で追加。ADR-0022
    Consequences rule で Tier 1+2 同期必須。
  - **docs sweep** — `docs/architecture.md` AI Layer
    詳細 (panel registration の `has_ai_provider()` ゲート、
    worker thread での block_on パターン)、
    `crates/dbboard-anthropic/README.md` の "where used" 行
    更新、`.claude/issues/0005` 全 checkmark 確定 + Status →
    closed。
  - **規模**: panel 本体 (200-300 行) + worker 拡張 (100-150
    行) + Fluent 11 locale × ~10 key (110 行) + 状態機械テスト
    (5-7 件、150 行) + docs sweep。1 PR にまとめても review
    可能サイズに収まる想定。

### PR #22 (dbboard-anthropic 具象 provider) マージクローズ (本セッション / 2026-06-15)

- PR #22 (`feat/dbboard-anthropic-provider` → `develop`) マージ済 =
  `c705918` (mergedAt 2026-06-15T11:51:41Z)。
- ローカル `develop` は `origin/develop` (= `c705918`) と
  fast-forward sync 済 (`c7fca0b..c705918`、2 commit ぶん advance:
  feat commit `89f0cdf` + merge commit `c705918`)。
- マージ済ローカル feature ブランチ (`feat/dbboard-anthropic-provider`
  = `89f0cdf`) は `git branch -d` 済。
- 本 PR の scope: `crates/dbboard-anthropic` 新規 + workspace 配線
  (workspace member 追加、`wiremock = "0.6"` dev-only workspace dep
  追加) + issue 0005 チェックマーク更新の 7 ファイル / +1143 / −25。
  中身:
  - `AnthropicProvider` struct (`reqwest::Client` + 事前解決
    `messages_url: String` + `api_key: String` (private) + `model:
    String`)。3 つの constructor: `new(api_key, model)` /
    `with_default_model(api_key)` (default `claude-sonnet-4-6` —
    `rules/performance.md` の Best coding model) /
    `with_config(AnthropicConfig)` (test の base_url override 用)。
    いずれも空文字 / whitespace の key/model を construction-time に
    `AiError::Configuration` で reject。
  - `AiProvider` impl: `id()` → `"anthropic"`、`capabilities()` →
    `AiCapabilities::default()` (Stage 1 は all-false)、async
    `explain` / `suggest_sql` は内部 `call_messages` に委譲。Anthropic
    Messages API (`POST /v1/messages`) に `x-api-key` +
    `anthropic-version: 2023-06-01` headers + JSON body
    (`model` / `system` / `messages: [{role, content}]` / `max_tokens`)
    を POST、response の `content: [{type:"text",text:...}]` blocks を
    concatenate して `AiResponse { text, tokens_in, tokens_out }` を
    返す。
  - 未知の content block (`tool_use` / `image` / 将来追加) は
    `#[serde(tag="type")] enum ResponseBlock { Text { ... }, #[serde(other)] Other }`
    で forward-compat に parse、無視。`deny_unknown_fields` を避けて
    脆くしない。
  - エラー分類 (ADR-0023 §8 / issue 0005): construction-time の
    空 key/model → `Configuration`。HTTP 4xx (401 auth / 429 rate-limit
    含む) → `Provider`。HTTP 5xx → `Provider`。malformed body →
    `Provider`。timeout / TLS / transport → `Network` (`reqwest::
    Error::without_url` で URL を scrub、log に key 含む URL が漏れ
    ない)。runtime 401 は **意図的に** `Configuration` ではなく
    `Provider` (`authentication_failure_becomes_a_provider_error_
    not_configuration` テストで保証)。
  - Security posture: api_key は struct field private、`Debug` impl で
    `<redacted>` 化 + `finish_non_exhaustive()` (reqwest::Client field
    を非表示にしたまま clippy `missing_fields_in_debug` を回避)。
    `https_only(true)` がデフォルト。ただし wiremock のためだけに
    narrow な `is_localhost(base)` 述語 (`http://127.0.0.1` /
    `http://localhost` / `http://[::1]` のみ match) で `https_only`
    を off にし、**それ以外の任意 URL に対する production の TLS
    guard は維持**。
  - 24 unit tests inline (URL build / payload shape / response parse /
    error 分類 / truncation / Debug redaction / localhost predicate)
    + 7 wiremock 統合テスト (`tests/messages_roundtrip.rs`) で
    success (explain/suggest) / request body 検査 / 429 / 5xx / 401 /
    malformed をカバー。live network 不要、env var 不要。live
    round-trip (`DBBOARD_ANTHROPIC_API_KEY` gated) は follow-up
    issue 送り。
  - Cargo.toml deps: `dbboard-ai` + `async-trait` + `reqwest` +
    `serde` + `serde_json` (production)、dev-only に `tokio` +
    `wiremock`。`dbboard-ui` / `dbboard-server` への依存なし
    (ADR-0023 Decision 1 通り、provider は in-process で binary に
    plug する)。
- 検証: pre-commit hook (`cargo fmt --check` / `cargo clippy
  --all-targets --all-features -- -D warnings` / `cargo check
  --all-targets --all-features` / `cargo test --all-features`) 全
  green、`cargo test --all-features` workspace 全クレート緑
  (dbboard-anthropic 24 unit + 7 integration 含む)。
- 着地中に当てたミニ事象:
  - `rustc E0382`: `transport_error` で `reqwest::Error::without_url`
    が `self` を consume するため `err.is_timeout()` が move 後呼び
    出しになる。`let timed_out = err.is_timeout();` を `without_url`
    前に capture して回避。
  - `clippy missing_fields_in_debug`: `AnthropicProvider` の `Debug`
    で `reqwest::Client` field を surface していない警告。`.finish()`
    を `.finish_non_exhaustive()` に変更 + 説明コメントで対応。
  - `clippy default_trait_access`: test 内 `Default::default()` 警告。
    `AiCapabilities` を import して `AiCapabilities::default()` に
    explicit 化。
- web 側への影響: **HTTP contract / JSON schema 変更なし**。AI 呼び出し
  は `dbboard-server` を介さず in-process (ADR-0023 Decision 3) なので
  contract に届かない。web 側 mirror brief 不要 (ADR-0013 / 0015 /
  0016 / 0018 / 0019 / 0020 / 0021 / 0022 / 0023 trait crate と同じ
  desktop-side-only カテゴリ継続)。
- 次セッション分岐候補 (issue 0005 split-by-crate の残り 2 段):
  - (a) **`apps/dbboard` 起動配線** — `DBBOARD_ANTHROPIC_API_KEY`
    (required) / `DBBOARD_ANTHROPIC_MODEL` (optional, default
    `claude-sonnet-4-6`) を `apps/dbboard::main` で resolve、
    `AnthropicProvider::with_default_model` または
    `AnthropicProvider::new` を試行 → 成功なら `Option<Arc<dyn
    AiProvider>>` を `DbboardApp::new` (相当) に渡す、失敗は log 出力
    のみで graceful (env var 無し時は `None` で AI panel 非表示)。
    README の "Run" セクションに "AI integration (optional)"
    subsection を追加。
  - (b) **`dbboard-ui` AI panel + worker** — `egui::Window` toggled
    from menu bar、`has_ai_provider()` true 時のみ register。
    `Command::AiExplain { sql }` / `Command::AiSuggest { prompt,
    schema }` / `Reply::AiResponded { text, tokens_in, tokens_out }` /
    `Reply::AiFailed { err }` を worker に追加。`tokio::runtime::
    Handle::block_on` パターンは `ConnectionSwitcher` と同型。
    Fluent keys を 11 locale 全件に追加 (ADR-0022 Consequences rule)。
    UI state machine の単体テスト (mode switch / send while busy
    は noop / response が stale を置換 / error が stale を置換)。
    docs sweep (`docs/architecture.md` AI Layer 詳細補完、README
    の AI panel 説明追記、`.claude/issues/0005` 全 checkmark 確定 +
    Status → closed)。
  - (c) **web 側 cross-repo** — web 側 Claude が `0004` Postgres
    adapter 着手 → `0009` (history schema impl) unblock。desktop
    側からは観察のみで OK。
  - これらは (a) + (b) を 1 PR にしても良いし、UI 周りが膨らみそう
    なら (a) 単独 → (b) で分離可能 (issue 0005 split-by-crate note
    が明示)。

### PR #20 (dbboard-ai trait crate) マージクローズ (前セッション / 2026-06-15)

- PR #20 (`feat/dbboard-ai-crate` → `develop`) マージ済 = `584348f`
  (mergedAt 2026-06-15T06:21:33Z)。
- ローカル `develop` は `origin/develop` (= `584348f`) と
  fast-forward sync 済 (`d7e6ac9..584348f`、2 commit ぶん advance:
  feat commit `8b582a7` + merge commit `584348f`)。
- マージ済ローカル feature ブランチ (`feat/dbboard-ai-crate` =
  `8b582a7`) は `git branch -d` 済。リモートも GitHub 側で削除済
  想定 (Settings の auto-delete branch on merge が ON なら自動、
  OFF でも次回 `git fetch --prune` で剥がれる)。
- 本 PR の scope: `crates/dbboard-ai` 新規 + workspace 配線 + issue
  0005 のチェックマーク更新の 9 ファイル / +525 / −8。中身:
  - `AiProvider` trait (`#[async_trait]`, `Send + Sync`,
    `Arc<dyn AiProvider>` で object-safe) — `id` / `capabilities` /
    `async explain` / `async suggest_sql` の 4 メソッド surface
  - `AiCapabilities` (flat all-false bool struct,
    `has_streaming` / `has_function_calling`) — `dbboard-core::
    Capabilities` と同型 (`#[derive(Copy, Debug, Default, Deserialize,
    Serialize)]`)
  - `ExplainRequest { sql, dialect }` / `SuggestRequest { prompt,
    dialect, schema: Vec<TableInfo> }` / `AiResponse { text,
    tokens_in, tokens_out }` — `TableInfo` は `dbboard-core` から
    re-export (provider crate が直接 `dbboard-core` 依存しないで
    済むように)
  - `AiError` 5 variants (`Configuration` / `Network` / `Provider`
    / `Quota` / `Cancelled`) + `AiResult<T>` alias — `DbError` と
    独立、HTTP contract に乗らないので translation 層不要
  - 単体テスト 15 本: object-safety、capability JSON round-trip、
    `AiError` Display 全 variant、value-type 等価、`Arc<dyn
    AiProvider>` 経由のリクエスト伝搬とエラー伝搬
  - Cargo.toml deps: `dbboard-core` + `async-trait` + `serde` +
    `thiserror`、dev-only に `tokio` + `serde_json`。`reqwest` は
    無し (trait crate には I/O なし、ADR-0023 Decision 1 通り)
- 検証: pre-commit hook (`cargo fmt --check` / `cargo clippy
  --all-targets --all-features -- -D warnings` / `cargo check
  --all-targets --all-features` / `cargo test --all-features`) 全
  green、`cargo test --all-features` workspace 全クレート緑
  (dbboard-ai 15 / dbboard-config 55 / dbboard-core 45 / dbboard-d1
  21 + 3 / dbboard-i18n 9 / dbboard-postgres 10 + 7 / dbboard-server
  40 + 12 / dbboard-turso 13 + 8 / dbboard-ui 87)。
- 着地中に当てたミニ事象:
  - `cargo fmt`: `display_covers_every_variant` テストの配列リテラル
    が 1 行に詰まっていて改行整形が入った (自動修正のみ)。
  - `cargo clippy -D warnings`: `result_alias_round_trips` テストの
    `let ok: AiResult<u32> = Ok(7);` が `unnecessary_literal_unwrap`
    / `unnecessary_wraps` 両方に当たった。意図 (alias の round-trip
    保証) を保ったまま、`Vec<AiResult<u32>>` 経由で値を構築する
    パターンに書き直して回避。
- web 側への影響: **HTTP contract / JSON schema 変更なし**。AI 呼び出し
  自体まだ存在しない、trait crate 単独追加なので contract に届かない。
  web mirror brief 不要 (ADR-0013 / 0015 / 0016 / 0018 / 0019 / 0020 /
  0021 / 0022 / 0023 と同じ desktop-side-only カテゴリ継続)。
- 次セッション分岐候補 (issue 0005 split-by-crate の残り 3 段):
  - (a) **`dbboard-anthropic` 具象 provider 着手** — 次の自然なステップ。
    新規 `crates/dbboard-anthropic`、deps: `dbboard-ai` + `reqwest`
    (`tls-rustls-ring`) + `tokio` + `serde_json` + (dev) `mockito`。
    `AnthropicProvider::new(api_key, model)` /
    `with_default_model(api_key)` (default `claude-sonnet-4-6`)、
    `id()` → `"anthropic"`、`capabilities()` → default (Stage 1)。
    `explain` / `suggest_sql` は Anthropic Messages API
    (`https://api.anthropic.com/v1/messages`) を POST、レスポンス
    envelope を parse して `AiResponse` を返す。エラー分類 (4xx
    rate_limit / overloaded → Provider、5xx → Provider、network →
    Network、malformed → Provider、API key 系 → Configuration)。
    `Debug` impl で api_key を redact。`mockito` で success /
    rate-limit / 5xx / malformed / timeout を網羅、live test は
    follow-up issue 送り。
  - (b) **`apps/dbboard` 起動配線 + `dbboard-ui` AI panel** — issue
    0005 の (a) の後段、`DBBOARD_ANTHROPIC_API_KEY` /
    `DBBOARD_ANTHROPIC_MODEL` env var 解決、`DbboardApp::new` が
    `Option<Arc<dyn AiProvider>>` を受け取る、AI panel は egui::Window
    で provider 存在時のみ render、Worker side に `Command::AiExplain`
    / `Command::AiSuggest` / `Reply::AiResponded` / `Reply::AiFailed`、
    Fluent key を 11 locale 全件に追加 (ADR-0022 Consequences
    rule)。`(a)` と統合して 1 PR にしても良いが、UI 周りで膨らみ
    そうなら分離可。
  - (c) **web 側 cross-repo** — web 側 Claude が `0004` Postgres
    adapter 着手 → `0009` (history schema impl) unblock。desktop
    側からは観察のみで OK。

### PR #18 (ADR-0023) マージクローズ (前セッション / 2026-06-12)

- PR #17 (`chore/post-pr16-doc-sync` → `develop`) マージ済 (前段)、
  続けて PR #18 (`docs/adr-0023-ai-provider-trait` → `develop`)
  マージ済 = `673a0c2`。
- ローカル `develop` は `origin/develop` (= `673a0c2`) と
  fast-forward sync 済 (`5a06a00..673a0c2`、4 commit ぶん advance)。
- マージ済ローカルブランチ 2 本 (`chore/post-pr16-doc-sync` =
  `a520b54` / `docs/adr-0023-ai-provider-trait` = `07b932c`) は
  `git branch -d` 済。リモート `docs/adr-0023-ai-provider-trait`
  は GitHub 側で削除済 (`git fetch --prune` で
  `[deleted] (none) -> origin/docs/adr-0023-ai-provider-trait` 確認)。
- 本 PR の scope: docs-only / 3 ファイル / +397 / −4
  (`docs/decisions.md` ADR-0023 append、`docs/roadmap.md` Phase 4
  ADR ピン留め、`.claude/issues/0005-dbboard-ai-trait-and-anthropic
  -provider.md` 新規起票)。
- ADR-0023 確定内容 (詳細は本ファイル下の "Phase 4 (ADR-0023)
  起票準備" セクション参照):
  - `dbboard-ai` crate (trait + 値型 + `AiError`、I/O なし)
  - First provider: `dbboard-anthropic` (reqwest + Anthropic API、
    default model `claude-sonnet-4-6`)
  - 配線: in-process (`apps/dbboard` で `Option<Arc<dyn AiProvider>>`
    を構築し UI worker に渡す、HTTP contract 不変)
  - Stage 1 設定: `DBBOARD_ANTHROPIC_API_KEY` env var のみ。
    Settings UI / `ai-providers.toml` + keychain は **Stage 2 ADR**
    送り。
  - Stage 1 commands: "Explain this query" + "Suggest SQL from
    prompt" (現在の `list_tables` 結果を schema snapshot として
    渡す、full DDL extraction は Stage 2)。
  - Graceful degradation: env var 未設定 → provider `None` → AI
    panel 非表示。
- web 側への影響: **HTTP contract / JSON schema 変更なし**。AI 呼び出し
  は `dbboard-server` を介さず in-process なので web mirror 不要
  (ADR-0013 / 0015 / 0016 / 0018 / 0019 / 0020 / 0021 / 0022 と
  同じ desktop-side-only カテゴリ)。
- 次セッション分岐候補:
  - (a) **issue 0005 実装着手** — `dbboard-ai` crate skeleton から。
    trait shape + 値型 (`ExplainRequest` / `SuggestRequest` /
    `AiResponse`) + `AiError` enum + capabilities struct。
    `dbboard-core` と同じく依存なし、`#[cfg(test)]` で trait の
    contract test (mock provider) を併走させて TDD 起点に。
  - (b) **`dbboard-anthropic` 先行起票** — reqwest 直叩きで Anthropic
    Messages API を呼ぶ first provider。env var resolution は
    `apps/dbboard` 側、`dbboard-anthropic::AnthropicProvider::new(
    api_key, model)` だけを公開。
  - (c) **web 側 cross-repo** — web 側 Claude が `0004` Postgres
    adapter 着手 → `0009` (history schema impl) unblock。desktop
    側からは観察のみで OK。

### Phase 4 (ADR-0023) 起票準備 (前セッション / 2026-06-11)

ユーザ指示 (`(a) Phase 4 dbboard-ai ADR 起票お願いします。`) を
受けて `develop@99c11b0` から `docs/adr-0023-ai-provider-trait` を
切り、設計判断を固めた段階で usage limit 接近のため区切り終了。
ADR 本文・issue 0005・roadmap / status 更新は次セッション。

**ピン留め済の設計判断** (次セッションで `docs/decisions.md` 末尾に
ADR-0023 として append する内容):

- **Status**: Accepted (2026-06-11)
- **Crate 構造**: `dbboard-ai` (trait only、I/O なし、`dbboard-core`
  と同じレイヤリング方針) + 初実装 `dbboard-anthropic` (reqwest +
  Anthropic API)。adapter パターンの機械的再利用 — `dbboard-core` /
  `dbboard-turso` / `dbboard-postgres` / `dbboard-d1` の関係を AI 層
  にミラー。
- **`AiProvider` trait shape**: `async_trait` + `Send + Sync`、
  `fn id() -> &'static str` (e.g. `"anthropic"`)、`fn capabilities()
  -> AiCapabilities` (flat bool struct、`DatabaseAdapter::capabilities`
  と同型)、Stage 1 surface は 2 必須メソッド:
  - `async fn explain(req: &ExplainRequest) -> AiResult<AiResponse>`
  - `async fn suggest_sql(req: &SuggestRequest) -> AiResult<AiResponse>`
  - object-safe (`Arc<dyn AiProvider>` 想定)。streaming 用 optional
    capability accessor (`fn streaming(&self) -> Option<&dyn
    StreamingProvider>`) は Stage 2 ADR の余地として trait に残す
    構造を採るが Stage 1 では追加しない。
- **配線**: **HTTP contract 不変**。AI 呼び出しは `dbboard-server`
  を介さず `apps/dbboard` バイナリで `Option<Arc<dyn AiProvider>>`
  を構築し UI worker に渡す in-process 方式 (ADR-0020 swap_backend
  / ADR-0022 set_language と同じ desktop-side-only カテゴリ)。理由:
  ループバック HTTP 経由はレイテンシ追加するだけ + DTO 層が増える +
  web 側は別 provider story (NestJS 内) なので contract 共有しても
  benefit ない。
- **First provider**: Claude (Anthropic API)。default model は
  `claude-sonnet-4-6` (rules/performance.md "Best coding model")。
  Stage 1 では model は env var で override 可能、ハードコード fallback
  あり。
- **設定**: Stage 1 は **`DBBOARD_ANTHROPIC_API_KEY` env var のみ**。
  Settings UI / `ai-providers.toml` 永続化 / SecretStore 統合は
  **Stage 2 ADR** に分離 (ADR-0013 connections.toml が Stage 2 の
  template になる)。env var 未設定なら provider 構築失敗 → graceful
  degradation 経路へ。
- **Graceful degradation**: provider 未構築 (`None`) なら UI は AI
  panel を render しない。runtime fallback (provider 故障で AI 機能
  だけ無効化) はやらない — env var の有無が toggle。
- **Stage 1 commands**:
  1. "Explain this query" — 現在の SQL を AI に渡して自然言語の解説
     を返す。schema 不要、SQL だけ。
  2. "Suggest SQL from prompt" — 自然言語プロンプト + 現在の
     `list_tables` 結果 (`Vec<TableInfo>`) を schema snapshot として
     AI に渡し、SQL を返す。full DDL extraction は Stage 2 (`DatabaseAdapter`
     に `dump_schema` 追加が必要)。
- **エラー型**: `AiError` enum を新設 (`Configuration` / `Network`
  / `Provider` / `Quota` / `Cancelled`)。`DbError` と独立、HTTP contract
  に乗らないので translation 層も不要。
- **Defer (Stage 2 以降)**: streaming、token budget meter、multi-provider
  switcher UI、ai-providers.toml + keychain SecretStore、history への
  AI 呼び出し記録、conversation history 保持、`dump_schema` extension。
- **Web mirror**: 不要。ADR-0013 / 0015 / 0016 / 0018 / 0019 / 0020 /
  0021 / 0022 と同じ desktop-side-only カテゴリ (HTTP contract /
  JSON schema 変更なし)。web 側は独自の provider story を持つので
  `0NNN-web-ai-mirror` brief は作らない。
- **SemVer impact** (ADR-0011): additive。新規 crate 2 つ追加、既存
  signature 不変、新規 env var のみ。

**次セッション再開タスク** (順序):
1. `docs/decisions.md` に ADR-0023 を append (ADR-0022 末尾の後ろ、
   format は ADR-0022 と完全同型: Status / Context / Decision 番号
   付き / Alternatives considered / Consequences / SemVer impact)
2. `.claude/issues/0005-dbboard-ai-trait-and-anthropic-provider.md`
   を起票 (issue 0004 の closure pattern を参考)
3. `docs/roadmap.md` Phase 4 セクション (lines 223-236) に ADR-0023
   リファレンスを追加。Phase 4 bullet 自体は `[ ]` のまま (trait
   決定のみ shipped、impl 未着手)
4. 本ファイル `.claude/project-status.md` の本セクションを「ADR-0023
   起票完了」状態に更新
5. 単一 docs commit: `docs: ADR-0023 dbboard-ai provider trait + open
   issue 0005`
6. 人間が push & PR 作成 (agent-commits-human-pushes rule)

**Open design questions** (PR レビューで decide すれば良いもの、
ADR drafting 時点で先送り):
- `dbboard-anthropic` 内部の HTTP client は reqwest 直叩きか
  `anthropic-rs` 等の community crate か (Anthropic 公式 Rust SDK は
  まだない)
- `ExplainRequest` / `SuggestRequest` に dialect hint (`"postgres"` /
  `"sqlite"` / `"d1-sql"`) を入れるかどうか (たぶん入れる)
- AI worker thread を query worker と分けるか共有するか (Stage 1 は
  共有で YAGNI、レイテンシ問題出たら Stage 2 で分離 ADR)

**並行して push 待ちの別件**:
- `chore/post-pr16-doc-sync@fb6085d` — PR #16 マージクローズの
  status / memory 同期 commit。本セッション末記録 (本コミット) を
  乗せた 2 commit ぶん。1 PR としてまとめて push 可。

### PR #16 (ADR-0022) マージクローズ (前セッション同日 / 2026-06-11)

- PR #16 (`feature/runtime-locale-switcher` → `develop`) マージ済 =
  `99c11b0` (mergedAt 2026-06-11T09:40:38Z)。
- リモート `feature/runtime-locale-switcher` は GitHub 側で削除済
  (`git fetch --prune` で `[deleted] (none) -> origin/feature/runtime-
  locale-switcher` 確認)。ローカル feature ブランチも `git branch -d`
  済 (was `135cf79`)。
- ローカル `develop` は `origin/develop` (= `99c11b0`) と fast-forward
  sync 済 (`701422b..99c11b0`、4 commit ぶん advance)。
- 本ブランチ (`chore/post-pr16-doc-sync`) は project-status と memory
  の anchor 更新のみの 1 commit。push & merge は別 PR。
- web 側への影響: **HTTP contract / JSON schema 変更なし**。web mirror
  brief 不要 (memory `dbboard-web-state.md` ADR-0022 セクション参照)。
- 次セッション分岐候補:
  - (a) **Phase 4 着手** — `dbboard-ai` クレート + `AiProvider` trait
    ADR から。Claude (Anthropic API) を first provider、`Explain` /
    `Suggest SQL from prompt` の 2 コマンド、graceful degradation
    (provider 未設定時は AI パネル非表示)。
  - (b) **web 側 cross-repo** — web 側 Claude が `0004` Postgres
    adapter 着手 → `0009` (history schema impl) unblock。desktop 側
    からは観察のみで OK。
  - (c) **後続 UX 磨き** — locale 永続化 (last-active hint 保存)、
    Tier 2 (ar / hi) lockstep lift、ConnectionAdmin の add/edit UI
    の小磨きなど。currently 緊急性なし。

### ADR-0022 runtime locale switcher 実装 (前セッション同日 / 2026-06-11)

ADR-0020 PR #14 マージクローズ直後、`develop@209fd81` を起点に
`feature/runtime-locale-switcher` を切って issue 0004 を実装、
合計 4 commit ぶん advance。

本セッションで追加した commit (古い順):

- `8ddd7e1` `feat(i18n): expose set_language / current_language for runtime swap (ADR-0022)`
- `3be9845` `feat(i18n): add language-menu key across all 11 locales`
- `1057ff7` `feat(ui): add Language submenu to the menu bar (ADR-0022)`
- (4 つめ = 本コミット) `docs: ADR-0022 + supersede ADR-0015 startup-only + close issue 0004`

実装の要点 (issue 0004 の予測より大幅にシンプル):

- **`dbboard-i18n` API**: `init()` がすでに「Safe to call more than
  once — later calls reselect without rebuilding the bundle cache」と
  documented されており fluent-rs runtime swap は自前実装不要。新規
  surface は `set_language(tag) -> Result<&'static FluentLanguageLoader,
  I18nEmbedError>` (thin wrapper around `init(Some(tag))`) と
  `current_language() -> LanguageIdentifier` の 2 関数のみ。
  `Arc<RwLock<FluentBundle>>` 再設計は **不要**。`t!()` / `t_args!()`
  マクロも変更なし。
- **テスト**: `set_language_swaps_active_bundle_at_runtime` を追加、
  ja → en → zh-CN を歩いて `t!("connect-button")` と
  `current_language()` の両方が毎ステップ flip することを assert。
  全 9 tests pass。
- **`apps/dbboard/src/main.rs`**: `SUPPORTED_LOCALES: &[(&str, &str)]`
  定数テーブル (タグ + native 名) + `language_menu(ui)` ヘルパー
  関数。各エントリに「✓ 」/「    」で active 表示、クリックで
  `set_language` + `ui.ctx().request_repaint()` + `ui.close()`。
  メニューバーで Connections ボタンの直後に配置。
- **Command/Reply パターンを採用しなかった**: locale switch は I/O
  なし → UI スレッド同期実行 + `request_repaint()` が正解。worker
  経由はレイテンシ追加するだけ。ADR-0022 "Alternatives considered"
  に文書化。
- **CJK font 再登録 不要**: `install_cjk_font` は egui の font stack
  に CJK fallback を *append* するだけなので、ja/ko/zh-CN/zh-TW
  すべて起動時 1 回の登録で covered。ja → zh-CN switch で tofu に
  なる心配なし。
- **egui 0.34 deprecation**: `ui.close_menu()` が deprecated になって
  おり、pre-commit hook の `-D warnings` を通すため `ui.close()` に
  修正。

ドキュメント整合 (本コミットに同梱):

- **docs/decisions.md**: ADR-0015 Status を「Superseded in part by
  ADR-0022 (2026-06-11) for the 'startup-only resolution' decision」に
  更新、その後に ADR-0022 を append (8 decisions / 5 alternatives /
  8 consequences)。
- **docs/roadmap.md**: Phase 2.5 に runtime switcher 完了の `[x]`
  bullet 追加、exit criteria に「menu-bar Language submenu (ADR-0022)
  switches it at runtime」追記。
- **README.md**: Connections 段落の直後に「Language / 言語 submenu」
  段落を追加 (ADR-0022 link)。
- **.claude/issues/0004-runtime-locale-switcher.md**: Status →
  closed、Closed 行追加、全 8 acceptance items `[x]`、Notes 全面
  改訂 (実装が予測より simpler だった旨)。
- **memory** (`dbboard-web-state.md` / `MEMORY.md`): ADR-0022 を
  「web 側 mirror 不要」リストに追加 (ADR-0015/0020 と同じカテゴリ:
  desktop 側 UX 変更で HTTP contract / JSON schema 影響なし)。

次セッション分岐候補:

- (a) **本ブランチ push & PR 作成** — user による push 待ち
  (agent は push しない workflow rule)、PR タイトル候補
  `feat(ui): runtime locale switcher (ADR-0022, closes #4)`。
- (b) **Phase 4 着手** — `dbboard-ai` クレート + `AiProvider` trait
  ADR から。Claude (Anthropic API) first provider、`Explain` /
  `Suggest SQL from prompt` の 2 コマンド、graceful degradation。
- (c) **web 側 cross-repo** — web 側 Claude が `0004` Postgres
  adapter 着手 → `0009` (history schema impl) unblock。desktop 側
  からは観察のみ。

### ADR-0020 PR #14 マージクローズ (前セッション同日 / 2026-06-11)

- PR #14 (`feature/in-process-connect-switching` → `develop`) マージ済 =
  `209fd81` (mergedAt 2026-06-11T08:34:03Z)。
- リモート `feature/in-process-connect-switching` は GitHub 側で削除済
  (`git fetch --prune` で `[deleted] (none) -> origin/feature/in-process
  -connect-switching` 確認)。ローカル feature ブランチも `git branch -d`
  済 (was `85e0cae`)。
- ローカル `develop` は `origin/develop` (= `209fd81`) と fast-forward
  sync 済 (`cdb35bc..209fd81`、8 commit ぶん advance)。
- README / docs/roadmap / .claude/issues/0004 / memory を一括で
  209fd81 整合状態に更新:
  - **README.md**: connections.toml 説明の直後に「Connections ウィンドウ
    + per-row Connect ボタンでリスタート不要に in-place swap」の段落を
    追加 (ADR-0020 link)。
  - **docs/roadmap.md**: Phase 2 末尾に ADR-0020 done 行を追加、Phase 3
    exit criteria の「without restarting the app」に「(the in-process
    swap mechanism is delivered by ADR-0020 under Phase 2)」と補足。
  - **.claude/issues/0004-runtime-locale-switcher.md**: Status を
    「open (unblocked)」に、`Blocked by` 行を取り消し線で残しつつ
    「PR #14 でブロック解除、ConnectionSwitcher パターンが直接の
    テンプレート」と上書き。
  - **memory** (`dbboard-web-state.md` / `MEMORY.md`): anchor を
    `desktop@209fd81 / 2026-06-11` に更新、ADR-0020 用「web 側 mirror
    不要」セクションを追加 (ADR-0019/0021 と同じカテゴリ)、MEMORY.md
    index 行を対応更新。
- web 側への影響: **HTTP contract: 変更なし**、**history JSON schema:
  変更なし**。swap は server 内部の `AppState` 更新のみ。web 側 mirror
  brief 不要。
- 次セッション分岐候補:
  - (a) **Phase 4 着手** — `dbboard-ai` クレート + `AiProvider` trait の
    ADR 起票から。Claude (Anthropic API) を first provider、`Explain` /
    `Suggest SQL from prompt` の 2 コマンド、Graceful degradation
    (provider 未設定時は AI パネル非表示)。
  - (b) **issue 0004 runtime locale switcher** — ADR-0020 unblocked。
    fluent-rs runtime swap + `Arc<RwLock<FluentBundle>>` + egui
    `request_repaint()`。font 再登録の要否は実装中に判断。先行 ADR
    起票 (ADR-0015 の startup-only 決定を partial supersede) が必要。
  - (c) **web 側 cross-repo** — web 側 Claude が `0004` Postgres adapter
    着手すると `0009` (history schema impl) が unblock される。desktop
    側からは観察のみで OK、impl は web 側担当。

---

## 以下は過去セッションの記録 (履歴目的、整合性は最新セッション側を優先)

### ADR-0020 in-process connection switching (前セッション / 2026-06-05)

`develop` (= `d7c58ad`) から `feature/in-process-connect-switching`
(ADR-0020 + issue 0004 と同居) で `swap_backend` server API → UI
worker `SwitchConnection` → 一覧 UI の `Connect` ボタン、と 3 段で
段階的に実装。Phase 3 Aurora DSQL (ADR-0021) は別ブランチで先行
shipped 済 (`d7c58ad` 含まれ済) なので本ブランチには重複しない。

本セッションで追加した commit (古い順):

- `fd3e36f` `feat(server): allow live adapter swap on a running AppState (ADR-0020)`
- `0237a45` `feat(ui,bin): wire SwitchConnection through UI worker and desktop app (ADR-0020)`
- `6f63382` `feat(ui): add per-row Connect button to the connection list`

実装の要点:

- **`dbboard-server` live swap**: `AppState` を
  `Arc<RwLock<Arc<dyn DatabaseAdapter>>>` に置き換え (RwLock over
  arc-swap、`dyn DatabaseAdapter` が `!Sized` のため)。各 HTTP
  ハンドラはリクエスト開始時に `current_adapter()` で 1 回スナップ
  ショットしてリクエスト中固定 → in-flight クエリは古いアダプタで
  完了、新規リクエストは新アダプタへ。`swap_backend(state, adapter)`
  公開関数、`RunningServer::state()` を pub にして desktop バイナリ
  から swap 可能。`PoisonError::into_inner()` で poisoned-lock graceful
  recovery。`http.rs` に in-flight swap roundtrip 2 件
  (`swap_backend_routes_next_request_to_new_adapter` /
  `running_server_state_lets_swap_take_effect_over_loopback`) 追加。
- **`dbboard-ui` worker + DbboardApp**: `Command::SwitchConnection
  { id }` / `Reply::ConnectionSwitched { id }` / `Reply::SwitchFailed
  { id, err }` を追加 (additive、既存 reply への影響なし)。
  `ConnectionSwitcher` trait (Send + Sync + 'static) を worker thread
  に inject、`tokio::runtime::Handle::block_on` で同期実行
  (アダプタ build はネストされた block_on ではないので安全)。
  `DbboardApp` に `switch_connection(id)` / `active_connection_id()`
  / `pending_switch_error()` を追加、`connection_switched_reply_*` /
  `switch_failed_reply_*` / `successful_switch_clears_a_prior_*` の
  3 状態遷移テストを追加。`pub use dbboard_core::DbError;` を
  追加して binary の dbboard-core 直接依存を避ける (architectural
  rule)。worker の `match &cmd` を `if let ... else` に refactor
  (clippy `single_match_else`)。
- **`apps/dbboard` 配線**: `DesktopSwitcher` (本物、`Arc<Mutex<
  ConnectionAdmin>>` + `Arc<dyn SecretStore>` + `RunningServer`
  state + `tokio::runtime::Runtime` をクローズ) と `NullSwitcher`
  (headless / config なし fallback) を実装。`backend_config_for_entry`
  を pub にして re-use。`DbboardApp::connect` に switcher を inject、
  `DesktopApp::ui` で admin を `Arc<Mutex<_>>` で UI / switcher で
  共有し、UI フレーム毎に `pending_connect` を drain して
  `switch_connection` へ転送。`PoisonError::into_inner()` で
  ロック poisoned 時の graceful recovery。
- **Connections 一覧の `Connect` ボタン (#48)**: `ConnectionsView`
  に `pending_connect: Option<String>` + `request_connect(id)` +
  `take_pending_connect()` を追加。`render_list` シグネチャを
  `(... , pending_connect: &mut Option<String>, active_id: &str)`
  に拡張、各エントリ行に小さな `Connect` ボタン (active 行は
  `egui::Button::small()` を `add_enabled(!is_active, ...)` で disable、
  `connections-active-marker` ラベル付与)。Fluent key
  `connections-connect-button` / `connections-active-marker` を
  11 locale 全件に追加 (en "Connect"/"(active)"、ja「接続」「（接続中）」、
  zh-TW「連線」「（目前）」、ru「Подключиться」「(активно)」など、
  ADR-0015 tier stability を維持)。新規テスト 3 (`new_view_has_no_
  pending_connect_request` / `request_connect_records_id_then_taking
  _clears_it` / `request_connect_overwrites_a_prior_unread_request`)。

検証状況 (本セッション末):

- `cargo fmt --all -- --check`: pass
- `cargo clippy --all-targets --all-features -- -D warnings`: pass
- `cargo check --all-targets --all-features`: pass
- `cargo test --all-features`: **全クレート green** (dbboard-config
  55 / dbboard-core 45 / dbboard-d1 21 / dbboard-postgres 10 +
  pg_roundtrip 7 / dbboard-server 40 + http 12 (in-flight swap 2 件
  追加) / dbboard-ui 87 (switch state machine 3 件 + pending_connect
  3 件 = 6 件追加))
- `cargo build --release`: pass
- `cargo test --all-features --release`: 全クレート green
- pre-commit hook (cargo-husky) は本ブランチの 3 機能 commit すべて
  fmt/clippy/check/test green でブロック通過。

次のステップ (人間担当):

1. `git push -u origin feature/in-process-connect-switching` (Norton
   の release build スキャン挙動については `env-windows-norton.md`
   参照)。
2. GitHub で PR open: base = `develop`, head =
   `feature/in-process-connect-switching`, title 例
   `feat: in-process connection switching (ADR-0020)`。本文に
   3 機能 commit (fd3e36f / 0237a45 / 6f63382) + ADR-0020 +
   issue 0004 を引用、scope (UI restart 不要で adapter を live swap、
   in-flight クエリは古い adapter で完了する semantics) を明記。
3. 動作確認 (#50 マニュアル部、任意): Supabase など複数接続を
   `connections.toml` に登録し、UI の「Connect」ボタンで切替→
   テーブル一覧更新→クエリ実行を確認。失敗時の `pending_switch_error`
   表示も確認。
4. merge 後にローカル feature ブランチを `git branch -d`、`develop`
   を fast-forward sync、次セッション開始時に本ファイルを更新。

web 側への影響:

- **HTTP contract: 変更なし**。`/capabilities` レスポンス shape も
  error category も触れていない (swap は server 内部での `AppState`
  更新のみ、外向き API は無変更) ので web 側 mirror は不要。
- **history per-record JSON schema: 変更なし**。
- 次セッションで `dbboard-web-state.md` memory を更新する際に
  「ADR-0020 は web 側 mirror 不要」を ADR-0013 / 0015 / 0016 /
  0018 / 0019 / 0021 と同じカテゴリに追記する。

### Phase 3 Aurora DSQL adapter (前セッション / 2026-06-04 — シップ済)

- 日付: 2026-06-04 (前セッション末、Phase 3 Aurora DSQL ADR-0021
  実装完了 + docs catch up 済、push 待ち)
- ブランチ: `feature/aurora-dsql-adapter-kind` (= `develop` (`d7c58ad`)
  から分岐、5 commit + 後続 ADR-0020 / issue 0004 含む、workspace
  tests 全 green、`cargo build --release` + `cargo test --all-features
  --release` も green、未 push)
- 現在の Phase: **Phase 3 Aurora DSQL adapter (third flavored kind
  over `dbboard-postgres`) 実装完了。ADR-0021 起票 → flavor 定数 +
  constructor → config/admin/store + server/resolver + UI 配線 →
  live test gate + 各 README/docs catch up の 3 機能 commit
  (cdca5fa / 82f8de7 / 95fe2d4)。Phase 3 の roadmap は Neon
  (ADR-0018) + Supabase (ADR-0019) + Aurora DSQL (ADR-0021) の
  3 flavored kind で完了。次は `git push -u origin
  feature/aurora-dsql-adapter-kind` → PR open against `develop`。**

### Phase 3 Aurora DSQL adapter (本セッション / 2026-06-04)

`develop` (= `d7c58ad`) から `feature/aurora-dsql-adapter-kind`
(ADR-0020 + issue 0004 と同居) で 3 機能 commit を積み、workspace
tests 全 green + release build/test も green。scope は **「pg-wire
flavored kind のみ、SDK-driven IAM token auto-refresh は future
ADR」** (ユーザ「すすめてください」で先行プラン承認済)。

積んだ commit (古い順、本セッション分):

- `36bba1c` `docs: ADR-0021 Aurora DSQL as a flavored kind over dbboard-postgres`
- `cdca5fa` `feat(postgres): add FLAVOR_AURORA_DSQL and connect_aurora_dsql (ADR-0021)`
- `82f8de7` `feat(aurora-dsql): wire ConnectionKind::AuroraDsql through config, resolver, and UI (ADR-0021)`
- `95fe2d4` `docs(aurora-dsql): add live test gate and catch up READMEs (ADR-0021)`

実装の要点:

- **ADR-0021 起票**: ADR-0018 (Neon) + ADR-0019 (Supabase) と
  同じ recipe を Aurora DSQL に機械的適用。違いは **password
  segment が短命 IAM token (~15 min TTL)** であること。SDK-driven
  auto-refresh は本 ADR の scope 外、future ADR 送り。
  rejected alternatives は (1) `dbboard-aurora-dsql` 別クレート /
  (2) `kind = "postgres"` のラベル維持 / (3) SDK refresh を同梱、の 3 件。
- **`dbboard-postgres` 4 つ目の flavor**: `FLAVOR_AURORA_DSQL =
  "aurora-dsql"` を pub const として追加 (kebab-case で TOML
  `kind` フィールドと同一文字列)。`connect_aurora_dsql(config)`
  constructor が `connect_with_flavor(config, FLAVOR_AURORA_DSQL)`
  に委譲。wire / SQL / TLS hardening (Prefer → Require) は完全同一。
  `flavor_constants_are_stable_and_distinct` を 4-way distinctness
  に拡張。
- **`dbboard-config` 第 4 variant**: `ConnectionKind::AuroraDsql {
  keyring_url_ref }` を additive v=1 として追加、`#[serde(rename =
  "aurora-dsql")]` で TOML 上の kebab-case と Rust 上の `AuroraDsql`
  を橋渡し。`ConnectionAdmin` add / update (set + keep) / delete /
  cross-kind-rejection の 5 新規テスト、`store.rs` に
  `parses_an_aurora_dsql_entry` 追加、`serialize_then_parse_is_
  identity_for_every_kind` を Aurora DSQL 込みに拡張。
- **`dbboard-server` リゾルバ**: `BackendConfig::AuroraDsql { url:
  String }` variant 追加 (`Debug` で `AuroraDsql(<redacted>)`)、
  `DBBOARD_AURORA_DSQL_URL` env var を **アルファベット tiebreaker で
  Neon / Supabase / PG の上**に配置 (`aurora-dsql` < `neon` <
  `supabase` < `postgres`)。`entry_to_backend` は AuroraDsql →
  BackendConfig::AuroraDsql、`backend.rs::connect_adapter` は
  `PostgresAdapter::connect_aurora_dsql` でディスパッチ。`label_for`
  env path で `"env:aurora-dsql"`、expired IAM token は ping() の
  `DbError::Connection` で表面化。新規テスト 5: env precedence
  (Aurora DSQL vs Neon vs Supabase vs PG)、entry → AuroraDsql
  backend、label_for env:aurora-dsql、Debug 漏洩防止。
- **`dbboard-ui` Connections フォーム**: `KindSelector::AuroraDsql`
  / `AddFormState::aurora_dsql_url` / `EditKindState::AuroraDsql {
  replace_url, new_url }` を追加、Add フォーム kind dropdown に
  "Aurora DSQL"、Edit フォームは Postgres/Neon/Supabase と同じ
  `replace_url` UI を再利用 (Fluent key `connections-field-pg-url`
  を共有、11 locale の同期コストゼロ — ADR-0015 tier stability)。
  新規 UI テスト 3 (Aurora DSQL add 経路 / edit prefill /
  replace_url=true 上書き)。
- **docs catch up** (`95fe2d4`): `crates/dbboard-postgres/README.md`
  flavor table に Aurora DSQL 行 + `DBBOARD_AURORA_DSQL_URL` の
  Tests セクション + ADR-0021 リンク、TLS hardening note 拡張。
  `docs/connections.md` Resolution order を 4 flavored kind に拡張
  (Aurora DSQL 最上位)、TOML schema 例に `kind = "aurora-dsql"`
  追加、`kind` 説明更新。`docs/compatibility.md` の pg-wire テーブル
  に Aurora DSQL Tier 1 行追加 (Postgres major version は user-
  invisible なので moving target 扱い)、SDK auto-refresh deferral
  を REST 系 deferral と並べて明記。`docs/roadmap.md` Phase 3 を
  「Neon, Supabase, and Aurora DSQL adapters ✅ done (2026-06-04)」
  に rename、3 つ目の bullet と exit criteria 更新。**top-level
  `README.md`** を catch up: 説明文 / Status / Supported Databases
  / Resolution order / pg-wire env var テーブルを 4 flavored kind
  全件に対応 (ユーザ「READMEの更新も忘れずにお願いします。」を
  最終 commit で satisfy)。
- **live test gate**: `tests/pg_roundtrip.rs` に
  `aurora_dsql_round_trip_reports_aurora_dsql_flavor` を追加、
  `DBBOARD_AURORA_DSQL_URL` set 時のみ実行 (未 set なら skip)、
  `adapter.id() == "aurora-dsql"` を end-to-end assertion。
  既存の `DBBOARD_PG_URL` / `DBBOARD_NEON_URL` /
  `DBBOARD_SUPABASE_URL` gated test は不変、1 マシンで 4
  endpoint 並行実行可能。

検証状況 (本セッション末):

- `cargo fmt --all -- --check`: pass
- `cargo clippy --all-targets --all-features -- -D warnings`: pass
- `cargo check --all-targets --all-features`: pass
- `cargo test --all-features`: **全クレート green** (dbboard-config
  55 + 4 + 8 / dbboard-server 40 / dbboard-postgres 10 +
  pg_roundtrip 7 (Aurora DSQL 1 件追加) / dbboard-ui 80 (Aurora
  DSQL UI 3 件追加))
- `cargo build --release`: pass
- `cargo test --all-features --release`: 全クレート green
- pre-commit hook (cargo-husky) は 3 機能 commit すべて
  fmt/clippy/check/test green でブロック通過。

次のステップ (人間担当):

1. `git push -u origin feature/aurora-dsql-adapter-kind` (Norton
   の release build スキャン挙動については `env-windows-norton.md`
   参照)。
2. GitHub で PR open: base = `develop`, head =
   `feature/aurora-dsql-adapter-kind`, title 例
   `feat: Aurora DSQL as flavored kind over dbboard-postgres
   (ADR-0021)`。本文に 3 commit (cdca5fa / 82f8de7 / 95fe2d4)
   と ADR-0021 を引用、scope (pg-wire flavored kind only、SDK
   auto-refresh deferred to future ADR) も明記。
3. merge 後にローカル feature ブランチを `git branch -d`、
   `develop` を fast-forward sync、次セッション開始時に本ファイル
   を更新。

web 側への影響:

- **HTTP contract: 変更なし**。`/capabilities` レスポンス shape も
  error category も触れていない (Aurora DSQL capability flags は
  すべて default-false のまま) ので web 側 mirror は不要。
- **history per-record JSON schema: 変更なし**。ADR-0017 の `conn`
  field は接続 id (例 `"aurora-dsql-prod"`) なのでアダプタ id とは
  独立、既存テストにも影響なし。
- 次セッションで `dbboard-web-state.md` memory を更新する際に
  「ADR-0021 は web 側 mirror 不要」を ADR-0013 / 0015 / 0016 /
  0018 / 0019 と同じカテゴリに追記する。

### Phase 3 Supabase adapter (本セッション / 2026-06-04)

`develop` (= `87c4eb6`) から `feature/supabase-adapter-kind` を切って
4 機能 commit + 本 close-out commit = 5 commit、workspace tests
全 green。ユーザ確認済の scope は **「pg-wire flavored kind のみ
(推奨)」** — ADR-0019 で Neon (ADR-0018) と同じ recipe を Supabase
に機械的適用、REST hybrid (PostgREST / GoTrue / Storage / Realtime)
は future ADR 送り。

積んだ commit (古い順):

- `84c1137` `docs: ADR-0019 Supabase as a flavored kind over dbboard-postgres`
- `2c0b734` `feat(postgres): add FLAVOR_SUPABASE and connect_supabase (ADR-0019)`
- `618344f` `feat(supabase): wire ConnectionKind::Supabase through config, resolver, and UI (ADR-0019)`
- `a5090af` `docs: document Supabase flavor and add DBBOARD_SUPABASE_URL live test (ADR-0019)`
- (本 commit) `chore(status): record Phase 3 Supabase ADR-0019 close-out`

実装の要点:

- **ADR-0019 起票**: `docs/decisions.md` に Accepted で append。
  ADR-0018 (Neon flavored kind) を「Supabase にも適用」する形で
  refine。rejected alternatives は (1) REST hybrid を最初から
  混ぜる / (2) `dbboard-supabase` 別クレート / (3) `kind = "postgres"`
  のラベルに留める / (4) pooler URL 用 sub-flavor を分ける、の 4 件。
  capability flags (`has_auth` / `has_storage` / `has_realtime`) は
  default-false のまま、REST 統合時に future ADR で立てる。
- **`dbboard-postgres` 3 つ目の flavor**: `FLAVOR_SUPABASE = "supabase"`
  を pub const として追加 (FLAVOR_POSTGRES / FLAVOR_NEON と並ぶ)。
  新規 `connect_supabase(config)` constructor が内部
  `connect_with_flavor(config, FLAVOR_SUPABASE)` に委譲。wire / SQL /
  TLS hardening (Prefer → Require) は完全に同一。既存 unit test
  `flavor_constants_are_stable_and_distinct` を 3-way distinctness で
  拡張。
- **`dbboard-config` 第 3 variant**: `ConnectionKind::Supabase {
  keyring_url_ref }` を additive v=1 として追加。`ConnectionAdmin`
  add / update / delete は Neon のミラー、kind 変更 (Postgres ↔
  Neon ↔ Supabase) は引き続き `KindMismatch` で拒否。
  `keyring_refs_in` は共有アーム `ConnectionKind::Postgres |
  ConnectionKind::Neon | ConnectionKind::Supabase` に集約。
  `store.rs` に Supabase parse / serialize、`admin.rs` に
  Supabase add / update (set + keep) / delete / cross-kind-rejection
  の 5 新規テスト。
- **`dbboard-server` リゾルバ**: `BackendConfig::Supabase { url:
  String }` variant を追加 (`Debug` で `Supabase(<redacted>)`)、
  `DBBOARD_SUPABASE_URL` env var を **Neon の下 / PG の上** に配置
  (alphabetical tiebreaker、両方 pg-wire flavored なので順序は規約)。
  `entry_to_backend` は `ConnectionKind::Supabase` を
  `BackendConfig::Supabase` へ、`backend.rs::connect_adapter` は
  `BackendConfig::Supabase` を `PostgresAdapter::connect_supabase`
  でディスパッチ (direct `:5432` / pooler `:6543` 両方この経路で
  受ける — URL 自体が選択)。`label_for` は env path で
  `"env:supabase"`。新規テスト 6 件: env precedence (Supabase vs
  PG / Supabase vs Neon)、entry → Supabase backend、label_for
  env:supabase、Neon > Supabase の tiebreaker、Debug 漏洩防止
  (Supabase URL の `supa-pw` を含まない)。
- **`dbboard-ui` Connections フォーム**: `KindSelector::Supabase` /
  `AddFormState::supabase_url` / `EditKindState::Supabase` を追加、
  Add フォームの kind dropdown に "Supabase" を追加、Edit フォームは
  Neon と同じ `replace_url` / `new_url` UI を再利用 (Fluent key
  `connections-field-pg-url` を共有して 11 locale の同期コストゼロ)。
  新規 UI テスト 3 (Supabase add 経路 / Supabase edit prefill /
  replace_url=true 上書き)。`render_edit_form` の Postgres | Neon
  パターンを Postgres | Neon | Supabase に拡張。
- **docs**: `connections.md` の Resolution order に
  `DBBOARD_SUPABASE_URL` を Neon の下に追記、TOML schema 例に
  `kind = "supabase"` のエントリを追加 (direct/pooler 両 URL OK の
  注釈付き)、`kind` の説明に Supabase を追加。`compatibility.md`
  の Postgres-wire テーブルで Supabase 行を Tier 1 に昇格
  (Postgres 17/16/15、`DBBOARD_SUPABASE_URL` gated、TLS required、
  direct/pooler 両エンドポイント covered)、REST 系 (PostgREST /
  GoTrue / Storage / Realtime) は本 ADR では out of scope と
  明記。`roadmap.md` Phase 3 を ✅ done (2026-06-04) に、Supabase
  bullet + adapter-specific quirks documented チェックを追加、exit
  criteria 達成文 (Neon + Supabase + 汎用 Postgres を 1 セッションで
  切り替え可能) に更新。`crates/dbboard-postgres/README.md` の
  flavor table に Supabase 行 (direct/pooler URL 注記)、TLS hardening
  note を `connect_supabase` に拡張、live test 例コマンドに
  Supabase バリエーション追加、ADR-0019 リンク追加。
- **live test gate**: `tests/pg_roundtrip.rs` に
  `supabase_round_trip_reports_supabase_flavor` を追加、
  `DBBOARD_SUPABASE_URL` set 時のみ実行 (未 set なら skip)。
  `adapter.id() == "supabase"` を end-to-end でアサート。既存の
  `DBBOARD_PG_URL` / `DBBOARD_NEON_URL` gated test は不変なので、
  1 マシンで 3 エンドポイントに向けて並行実行可能。

検証状況 (本セッション末、close-out commit 直前):

- `cargo fmt --all -- --check`: pass
- `cargo clippy --all-targets --all-features -- -D warnings`: pass
- `cargo test --all-features`: **全クレート green** (dbboard-config
  49 + 4 + 8 / dbboard-server 35 + 10 / dbboard-postgres 10 + 6
  (pg_roundtrip に Supabase 1 件追加) / dbboard-ui 77 (前回 74 から
  Supabase UI 3 件追加) / 他はそのまま)
- pre-commit hook (cargo-husky) は 4 機能 commit すべて
  fmt/clippy/check/test green でブロック通過。

次のステップ (人間担当):

1. `git push -u origin feature/supabase-adapter-kind`
2. GitHub で PR open: base = `develop`, head =
   `feature/supabase-adapter-kind`, title 例
   `feat: Supabase as flavored kind over dbboard-postgres (ADR-0019)`。
   本文は 5 commit と ADR-0019 をリンク、scope 確定経緯
   (AskUserQuestion で「pg-wire flavored kind のみ」採択、REST
   hybrid は future ADR) も書く。
3. merge 後にローカルの feature ブランチを `git branch -d`、`develop`
   を fast-forward sync、次のセッション開始時に本ファイルを更新。
4. **Phase 3 はこの PR を持って完了**: Neon (ADR-0018) + Supabase
   (ADR-0019) 双方シップ、roadmap.md の Phase 3 が ✅ done。
   次セッションでは Phase 4 (AI integration, optional layer) 着手か、
   web 側 Claude が `0003-web-history-schema-mirror` を pickup した
   場合の cross-repo フォローアップ対応に分岐。

web 側への影響:

- **HTTP contract: 変更なし**。`/capabilities` レスポンス shape も
  error category も触れていない (Supabase capability flags はすべて
  default-false のまま) ので web 側 mirror は不要。
- **history per-record JSON schema: 変更なし**。ADR-0017 の `conn`
  field は接続 id (例 `"supabase-prod"`) なのでアダプタ id とは
  独立、既存テストにも影響なし。
- 次セッションで `dbboard-web-state.md` memory を更新する際に
  「ADR-0019 は web 側 mirror 不要」を ADR-0013 / 0015 / 0016 /
  0018 と同じカテゴリに追記する。

### Phase 3 Neon adapter (本セッション / 2026-06-04)

`develop` (= `7555c58`) から `feature/neon-adapter-kind` を切って
4 commit、workspace tests 全 green。ユーザ確認済の scope は
**「Neon を first-class kind に (推奨)」** — docs-only でも別
クレートでもない、`dbboard-postgres` への flavor 注入。

積んだ commit (古い順):

- `8b0a72a` `docs: ADR-0018 Neon as a flavored kind over dbboard-postgres`
- `45ffe2b` `feat(postgres): add flavor field, connect_neon constructor (ADR-0018)`
- `6936902` `feat(neon): wire ConnectionKind::Neon through config, resolver, and UI (ADR-0018)`
- `0385aaf` `docs: document Neon flavor + add DBBOARD_NEON_URL live test gate (ADR-0018)`

実装の要点:

- **ADR-0018 起票**: `docs/decisions.md` に Accepted で append。ADR-0008
  (汎用 pg-wire 採用) を refine、Phase 3 「Connection picker recognises
  adapter kind」と `architecture.md` の stable-id 不変条件を discharge。
  rejected alternatives は URL inference / v=2 bump / `dbboard-neon`
  クレート / `NEON_URL` を `PG_URL` の下に置く案、の 4 件。
- **`dbboard-postgres` flavor 化**: `FLAVOR_POSTGRES = "postgres"` /
  `FLAVOR_NEON = "neon"` を pub const として公開。`PostgresAdapter`
  に `flavor: &'static str` フィールドを追加し `id()` から返却。
  既存の `connect(config)` は `FLAVOR_POSTGRES`、新規 `connect_neon
  (config)` は `FLAVOR_NEON` で内部 `connect_with_flavor` に委譲。
  wire / SQL / TLS hardening (Prefer → Require) は完全に同一。新規
  unit test 1 (`flavor_constants_are_stable_and_distinct`)。
- **`dbboard-config` 追加変種**: `ConnectionKind::Neon { keyring_url_ref
  }` を additive v=1 として追加 (schema bump なし)。`ConnectionAdmin`
  add / update / delete は Postgres のミラー実装、kind 変更
  (Postgres ↔ Neon) は引き続き `KindMismatch` で拒否 (ADR-0016 §3
  の rollback story を保つため)。`store.rs` に Neon parse / serialize
  ラウンドトリップテスト、`admin.rs` に Neon add / update (set + keep) /
  delete / cross-kind-rejection の 5 新規テスト。
- **`dbboard-server` リゾルバ**: `BackendConfig::Neon { url: String }`
  variant を追加 (`Debug` で redacted)、`DBBOARD_NEON_URL` env var を
  `DBBOARD_PG_URL` の **上** に配置 (より具体的なラベルを優先)。
  `entry_to_backend` は `ConnectionKind::Neon` を `BackendConfig::Neon`
  へ、`backend.rs::connect_adapter` は `BackendConfig::Neon` を
  `PostgresAdapter::connect_neon` でディスパッチ。`label_for` は
  env path で `"env:neon"`、file-store path はエントリ id を返却。
  新規テスト: env precedence (neon vs pg)、entry → Neon backend、
  label_for env:neon、Debug 漏洩防止。
- **`dbboard-ui` Connections フォーム**: `KindSelector` /
  `AddFormState` / `EditKindState` に Neon 行を追加、Add フォームの
  kind dropdown に "Neon" を追加、Edit フォームは Postgres と同じ
  `replace_url` / `new_url` UI を再利用 (Fluent key を共有して
  11 locale の同期コストゼロ)。新規 UI テスト 3 (Neon add 経路 /
  Neon edit prefill / replace_url=true 上書き)。
- **docs**: `connections.md` の Resolution order に `DBBOARD_NEON_URL`
  を最上位として追記、TOML schema 例に `kind = "neon"` のエントリを
  追加し `kind` の説明に Neon を追加。`compatibility.md` の
  PostgreSQL-wire テーブルで Neon 行に「ADR-0018: id() == "neon"、
  live test gated on `DBBOARD_NEON_URL`、TLS required」を明記。
  `roadmap.md` Phase 3 の Neon と Connection picker recognises adapter
  kind の 2 行を done に。新規 `crates/dbboard-postgres/README.md`
  で flavor pattern / TLS hardening / dynamic decoding / row cap /
  live test の走らせ方を解説。
- **live test gate**: `tests/pg_roundtrip.rs` に
  `neon_round_trip_reports_neon_flavor` を追加、`DBBOARD_NEON_URL`
  set 時のみ実行 (未 set なら skip)。`adapter.id() == "neon"` を
  end-to-end でアサート。既存の `DBBOARD_PG_URL` gated test は
  不変なので、1 マシンで CockroachDB / 生 Postgres と Neon を別
  endpoint に向けて並行実行可能。

検証状況 (本セッション末):

- `cargo fmt --all -- --check`: pass
- `cargo clippy --all-targets --all-features -- -D warnings`: pass
- `cargo test --all-features`: **全クレート green** (dbboard-config
  43 + 4 + 8 / dbboard-server 45 / dbboard-postgres 21 / dbboard-ui
  76 (前回 74 から Neon UI 3 件追加) / 他はそのまま)
- pre-commit hook (cargo-husky) は 4 commit すべて fmt/clippy/check
  /test green でブロック通過。

次のステップ (人間担当):

1. `git push -u origin feature/neon-adapter-kind`
2. GitHub で PR open: base = `develop`, head = `feature/neon-adapter
   -kind`, title 例 `feat: Neon as flavored kind over dbboard-postgres
   (ADR-0018)`。本文は 4 commit と ADR-0018 をリンク、scope 確定
   経緯 (AskUserQuestion で「first-class kind」採択) も書く。
3. merge 後にローカルの feature ブランチを `git branch -d`、`develop`
   を fast-forward sync、次のセッション開始時に本ファイルを更新。

web 側への影響:

- **HTTP contract: 変更なし**。`/capabilities` レスポンス shape も
  error category も触れていないので web 側 mirror は不要。
- **history per-record JSON schema: 変更なし**。ADR-0017 の `conn`
  field は接続 id (例 `"neon-prod"`) なのでアダプタ id とは独立、
  既存テストにも影響なし。
- 次セッションで `dbboard-web-state.md` memory を更新する際に
  「ADR-0018 は web 側 mirror 不要」を ADR-0013 / 0015 / 0016 と
  同じカテゴリに追記する。

### Phase 2 PR #10 マージクローズ (前セッション末 / 2026-06-04)

### Phase 2 PR #10 マージクローズ (本セッション末 / 2026-06-04)

- PR #10 (`feature/query-history-persistence` → `develop`) マージ済
  = `ca6ca93` (GitHub 上で merge commit、`mergedAt`
  2026-06-04T03:57:54Z)。
- 取り込まれた 7 commits: `62ed834` (ADR) / `b4c1c1c` (path fix) /
  `c023eba` (default_history_path) / `c3bfcb5` (persistence layer) /
  `72cb165` (app wiring + server label helper) / `c7aac22` (web
  handoff brief) / `ae86627` (closeout)。
- リモート `feature/query-history-persistence` は merge 時に削除済
  (`git fetch --prune` で `[deleted] (none) -> origin/feature/
  query-history-persistence`)、ローカル feature ブランチも
  `git branch -d` 済。
- ローカル `develop` は `origin/develop` (= `ca6ca93`) と fast-forward
  sync 済。
- web 側への handoff: `.claude/issues/0003-web-history-schema-mirror.md`
  が `develop` 上で読める状態。web 側 Claude が pickup する時のアンカー
  commit は `ca6ca93` (PR コメントには `72cb165` を引用しているが、
  実体は merge 後の `ca6ca93` から参照可)。HTTP contract には触らない
  ので desktop 側の追加作業なしに並行可。

### 本セッション (2026-06-04) で landed したもの

- `feature/query-history-persistence` ブランチを `develop` から切り出し
  (commit `7180407` 起点)。
- **ADR-0017 起票** (`62ed834`): `docs/decisions.md` に append。JSON
  Lines / record schema / storage / rotation / secret handling / 8 項目
  + cross-repo coordination policy を採択。Stage 1 ADR-0014 の「Stage 2
  ADR」プレースホルダを realise。
- **`default_history_path()` 追加** (`c023eba`): `dbboard-config::store`
  に `history.jsonl` 解決 helper を追加 (`default_path()` と対称)。
- **persistence layer 実装** (`c3bfcb5`): `crates/dbboard-ui/src/history.rs`
  に `PersistentHistoryStore`、JSON envelope (v/ts/conn/actor/sql/status/
  duration_ms/rows/rows_affected/error)、`load_tail` (起動時末尾 N 行
  hydrate、malformed line / unknown v / unknown status は count して skip)、
  startup-only rotation (50 MiB or 100k 行で `.jsonl.1` overwrite)、
  `O_APPEND` 1 record 1 line atomic write。Stage 1 の `HistoryStore`
  公開 API は不変。
- **app wiring + server label helper** (`72cb165`): `record_submit`
  (in-memory、submit-time、即時 UX) と `record_completion` (disk、
  reply-time、rich record) を分離 (Option D)。`dbboard-server` に
  `resolved_connection_label()` を追加し ADR-0017 `conn` field をスタンプ。
  `apps/dbboard` で `time` crate (`formatting` + `std` only) 経由の
  RFC 3339 clock を `RfcClock = fn() -> String` として inject (UI 本体は
  date crate non-dependent)。
- **handoff brief 起票** (`c7aac22`): `.claude/issues/0003-web-history
  -schema-mirror.md`。`0001`/`0002` と異なり HTTP wire contract mirror
  ではなく **per-record JSON schema mirror**。web 側 ADR は「desktop
  ADR-0017 と同一 schema」だけ書けば済む。secret handling delta
  (verbatim logging は共有サーバ上で意味が変わる) を flag。
- **roadmap.md 更新**: Phase 2 Query history 行を「in-memory (Stage 1)」
  + 「persistent JSON Lines (Stage 2, ADR-0017)」の 2 行に分割。

### 検証状況 (本セッション最終)

- `cargo fmt --all -- --check`: pass
- `cargo clippy --all-targets --all-features -- -D warnings`: pass
- `cargo test --all-features`: **266 tests passed**
  (history persistence layer + UI completion path + server label helper
  の新規テストを合算)
- pre-commit hook (cargo-husky) は各 commit で fmt/clippy/check/test
  をすべて green でブロック通過。

### 次セッション開始タスク: Phase 3 着手 (Neon / Supabase アダプタ)

- 主目的は trait の証明 — UI / core / contract を変えずに新アダプタを
  追加できることを示す。
- 候補順: Neon (pg-wire なので `dbboard-postgres` を再利用、Phase 1.7
  実装をそのまま流用) → Supabase (REST + sqlx ハイブリッド、新クレート
  `dbboard-supabase` 起票が必要)。
- 開始前にやること: roadmap.md Phase 3 の項目を再読、Neon 接続文字列の
  形式と Postgres URL parser の互換性を確認 (`dbboard-postgres` の
  `sslmode=require` 昇格周りが Neon 推奨と合うか)、ADR-0011 tiered
  support 上の位置付けを確認。
- 先んじて Phase 2 の追記候補がもしあれば (capability 周りの flag を
  Postgres 側で立てる等)、Phase 3 着手前に切り出す。

### Phase 2 PR #9 マージクローズ (前々セッション末 / 2026-06-03)

- PR #9 (`feature/connection-admin-ui` → `develop`) マージ済 = `88d0f45`
  (GitHub 上で merge commit、squash ではない)。
- ローカル `feature/connection-admin-ui` 削除済 (`git branch -d`、
  `263d9b1` was)。リモート側 branch は人間が削除済 (確認:
  `git fetch --prune` で `[deleted] (none) -> origin/feature/
  connection-admin-ui`)。
- ローカル `develop` は `origin/develop` (= `88d0f45`) と sync 済。
- memory 更新済: `dbboard-web-state.md` で desktop@88d0f45 snapshot
  反映 + ADR-0016 を「contract change ではない (UI / config のみ)
  ので mirror 不要」リストに追加、`MEMORY.md` index も対応更新。

### Phase 2 接続管理 UI (本セッション / 2026-06-03)

`develop` から `feature/connection-admin-ui` を切って 6 commit、全
workspace tests green (dbboard-config 12 → 17、dbboard-ui 30 → 46、
他は据え置き)。Push は人間担当。

積んだ commit (古い順):

- `720516a` `docs: ADR-0016 connection management UI (HeidiSQL model, Stage 1)`
- `5a07728` `feat(config): add ConnectionAdmin use-case (ADR-0016)`
- `c8e4099` `feat(ui): add ConnectionsView for connection management (ADR-0016)`
- `2541ef7` `feat(app): wire ConnectionAdmin and the Connections window (ADR-0016)`
- `05aaf93` `i18n(connections): translate connections window for tiers 1+2 locales`
- (本 commit) `docs: tick Phase 2 connection management UI in roadmap and status`

実装の要点:

- ADR-0016 起票: Stage 1 は add / edit / delete + 「次回起動時に有効」
  リスタートヒントのみ。ホット切替 / active selector / kind 変更は
  Stage 2 以降。HeidiSQL のように **複数の dbboard プロセスを上げて
  別接続を扱う** ユーザ動線を一次サポート (ユーザ確認済)。
- `dbboard-config::ConnectionAdmin` 新設: `ConnectionFile`
  + `Arc<dyn SecretStore>` を抱え、`add(draft) / update(draft) /
  delete(id)` の 3 メソッドを公開。`ConnectionDraft` は kind 別
  enum で、Edit 側は token / pg URL の差し替え意志を `Option`
  で表現 (write-only secret)。失敗時 `ConfigError` を返し、TOML と
  keyring の両方を確実に rollback (path 単位の atomic rename + 失敗
  時 secret 削除)。新規 unit test 5 + integration test 0 (file-IO は
  既存 `tests/secrets.rs` でカバー済)。
- `dbboard-ui::ConnectionsView` 新設: `Mode { List, Add(form),
  Edit { id, form }, ConfirmDelete { id, name } }` の小さな state
  machine。`render_*` を method 分割し、submit ロジックは
  `InMemorySecretStore` + `tempfile` で純粋関数として 16 件
  ユニットテスト化 (form→draft / submit 成否 / 空白 base_url の
  None 化 / kind 切替の per-field buffer 保持)。`#[derive(Default)]`
  + clippy `-D warnings` クリア。
- Add form は kind dropdown で `Turso | D1 | Postgres` を切替。
  各 kind 専用フィールドを独立 buffer で抱えるので、kind を flip
  しても入力が失われない。Edit form は kind を locked 表示 (Stage 2
  で再考)、secret は `Replace token` / `Replace URL` チェック ON で
  のみ新 buffer を送信。default OFF なので名前だけ直す編集が安全。
- `apps/dbboard/main.rs` を `DesktopApp` ラッパに刷新:
  `Arc<dyn SecretStore>` を server 解決と runtime admin で共有 →
  UI から追加した token がそのまま再読み取り可能。Top menu bar に
  `Connections` ボタンを追加 (admin が None = headless / CI fallback
  時のみ非表示)。`egui::Panel::top` + `egui::MenuBar::new().ui(...)` +
  `Window::open(&mut bool).show(ctx, ...)` の 0.34 API を使用。
- i18n: `connections-*` キーを 21 件追加 (window-title / restart-hint
  / list-empty / add/edit/delete/save/cancel / confirm-delete /
  field-{id,name,kind,turso-path,d1-account,d1-database,d1-base-url,
  d1-token,pg-url} / replace-token / replace-url)。en を source of
  truth として 11 locale すべて翻訳済 (ja/ko/zh-CN/zh-TW/de/fr/es/
  pt-BR/ru/it)。pt-BR 「Conexões」 / fr 「Connexions」 / de
  「Verbindungen」 / ru 「Подключения」 等、ダイアクリティカル
  正しく記述。
- HTTP contract (`docs/api-contract.md`) / `dbboard-server` /
  `dbboard-core` / adapter 各種に変更ゼロ → web 側 mirror 不要
  (Phase 2 admin UI は presentation + config-layer only、contract に
  touch しないため `dbboard-web-state.md` は更新不要)。

## 次の Phase 2 PR (human action)

- ローカル commit を push: `git push -u origin
  feature/connection-admin-ui` (Norton で release build が遅く
  なる可能性あり、`env-windows-norton.md` 参照)。
- `develop` 向けに PR を出す。タイトル例: `feat: Phase 2 — connection
  management UI (ADR-0016, Stage 1)`。
- PR body には上記 6 commit の役割と「**HTTP contract は touch せず、
  config + UI レイヤのみで完結。HeidiSQL multi-process モデル**」
  点を明記。
- マージ後の残 Phase 2 タスクは history 永続化 (Stage 2 ADR 待ち) のみ。
  Phase 2 を closing にする前に、(1) history Stage 2 ADR、(2) Stage 2
  ADR 実装、いずれかを次セッションで判断。

### Phase 2.5 PR #8 マージクローズ (本セッション末 / 2026-06-03)

- PR #8 (`feature/i18n-locales` → `develop`) マージ済 = `c36d1b4`
  (GitHub 上で merge commit、squash ではない)。
- ローカル `feature/i18n-locales` 削除済 (`git branch -d`、`f6f5107` was)。
  リモート側 branch は人間が削除済 (確認: `git fetch --prune` で
  `[deleted] (none) -> origin/feature/i18n-locales`)。
- ローカル `develop` は `origin/develop` (= `c36d1b4`) と sync 済。
- memory 更新済: `dbboard-web-state.md` で desktop@c36d1b4 snapshot 反映
  + ADR-0015 を「contract change ではない (DbError 本文は English 維持)
  ので mirror 不要」リストに追加、`MEMORY.md` index も対応更新。

### Phase 2.5 多言語化 (本セッション / 2026-06-03)

`develop` から `feature/i18n-locales` を切って ADR + skeleton + wiring を
分割 commit、全 workspace 175 unit tests + 2 doctests green (dbboard-i18n
8 + dbboard-ui 30、他 137)。Push は人間担当。

積んだ commit の構成 (古い順):

- `6a804fe` `feat(i18n): add dbboard-i18n crate with 11-locale Fluent loader (ADR-0015)`
- (本セッション後半) `feat(i18n): wire dbboard-ui labels and apps/dbboard startup`
- (本セッション後半) `docs: tick Phase 2.5 multilingual UI roadmap entry`

実装の要点:

- ADR-0015 起票: locale 11 件 (Tier 1 + Tier 2)。ar/hi は RTL / shaping
  考慮で Stage 2 送り。framework は fluent-rs (gettext より plural rule
  柔軟)。font 戦略は Latin/Cyrillic を egui の Ubuntu-Light に任せ、
  CJK は `apps/dbboard` 起動時に OS フォント探索。
- `crates/dbboard-i18n` 新設: `rust-embed` で `.ftl` を build-time
  embed、`fluent_language_loader!()` を `OnceLock` で global 化 (MSRV 1.75
  に合わせ `LazyLock` 1.80 は不可)。`t!()` / `t_args!()` は最終的に
  `i18n-embed-fl` proc-macro を **drop**。fl!() は caller crate の
  `CARGO_MANIFEST_DIR` に対し `<crate-name>.ftl` を探すため、consumer
  crate 毎に `i18n.toml` 複製が必要になる。代わりに runtime で直接
  `loader().get(id)` / `loader().get_args_concrete(id, HashMap)` を呼ぶ
  簡潔な macro に差し替え。
- `crates/dbboard-i18n/i18n/<tag>/dbboard-i18n.ftl` を 11 言語ぶん作成。
  ファイル名は crate 名と一致させる (i18n-embed の規約)。
- `dbboard-ui`: literal UI 文字列を全て `t!()` 化。`DbError` 本文は
  ADR-0009 (HTTP contract) の都合で English のまま、UI 側で
  `category()` をスイッチして翻訳した prefix を付与する
  `error_display(&DbError) -> String` を導入。test 2 件追加
  (`error_display_prefixes_translated_category_to_raw_message` /
  `error_display_covers_every_db_error_category`)。
- `apps/dbboard`: `main()` 先頭で `dbboard_i18n::init(None)` を呼ぶ。
  失敗は non-fatal (eprintln + en fallback) — 将来 locale 追加で .ftl
  に typo が出ても起動を壊さないため。`install_cjk_font(&ctx)` を
  eframe creator で実行、Windows / macOS / Linux 別に候補パスを順に
  `std::fs::read` し、最初に読めた font を `FontFamily::{Proportional,
  Monospace}` に **append** (replace ではない、Latin glyph は Ubuntu-
  Light のまま)。
- HTTP contract / dbboard-server / dbboard-core / adapter 各種に変更
  ゼロ → web 側 mirror 不要 (memory `dbboard-web-state.md` も触らない、
  Phase 2.5 は presentation-only)。

### Phase 2 query history (in-memory, Stage 1) — 本セッション / 2026-06-03

`develop` から `feature/query-history-in-memory` を切って 4 commit、
156 → 168 tests green (dbboard-ui 28、history 8 + lib 13 + client 7)。
Push は人間担当。

積んだ commit (古い順):

- `992f7a5` `docs: add ADR-0014 for in-memory query history`
- `1356c6e` `feat(ui): in-memory query history store (ADR-0014)`
- `8b2eefb` `feat(ui): wire query history into editor with click-to-restore`
- `fbb1fa7` `docs(roadmap): tick Phase 2 in-memory query history (Stage 1)`

実装の要点:

- ADR-0014 起票: in-memory を Stage 1、永続化は connection 管理 UI 後に
  Stage 2 ADR で扱う。理由は history の storage shape が connection-
  management 設計を引っ張らないようにするため。HTTP contract は touch
  しない (history は純粋 UI 関心事)。
- `crates/dbboard-ui/src/history.rs` 新設。`HistoryStore` = bounded
  `VecDeque<HistoryEntry>` (cap 100, `DEFAULT_CAPACITY`)、`push` は
  trim 後 empty を ignore + 隣接 dedup + cap 超過で oldest drop。
  `iter` は newest-first (`push_front` で蓄積)。zero capacity は 1 に
  clamp (footgun 防止)。
- `DbboardApp` に `history: HistoryStore` フィールド追加、`run_sql` の
  guard 通過後 / busy=true 前で `push` (busy ガード時は呼ばれないので
  履歴汚染なし)。public accessor `history(&self) -> &HistoryStore` で
  test 容易性を確保。
- UI: SQL TextEdit 直下、Result の上に `CollapsingHeader("History (N)")`。
  default_open=false で初期は折りたたみ。`ScrollArea::vertical()
  .max_height(160.0)` 内に `small_button` で各 entry。クリックで
  `restore: Option<String>` に拾い、iter() borrow を抜けてから
  `self.sql = sql` 代入。ボタンラベルは `history_button_label` で
  first line + 80 chars truncation + ellipsis。
- 新規 test 5 件:
  `new_app_has_empty_history` / `run_sql_pushes_to_history` /
  `run_sql_empty_input_does_not_push_to_history` /
  `run_sql_consecutive_duplicates_collapse_in_history` /
  `run_sql_while_busy_does_not_push_to_history`。
- `docs/roadmap.md` Phase 2 の query history bullet を `[x]
  Query history — in-memory (ADR-0014, Stage 1). Persistence is
  deferred to a Stage 2 ADR landing after connection-management UI.`
  に更新。
- HTTP contract / dbboard-server / dbboard-core / adapter 各種に変更
  ゼロ → web 側 mirror 不要 (memory `dbboard-web-state.md` も別途
  反映済み — PR #3 で /capabilities ミラー完了、PR #4 で PWA Phase
  1.5 shell シップ済みを記録)。

## 次の Phase 2 PR (human action)

- ローカル commit を push: `git push -u origin
  feature/query-history-in-memory` (Norton で release build が遅く
  なる可能性あり、`env-windows-norton.md` 参照)。
- `develop` 向けに PR を出す。タイトル例: `feat(ui): Phase 2 — in-memory
  query history (ADR-0014, Stage 1)`。
- PR body には上記 4 commit の役割と「**HTTP contract は touch せず、
  dbboard-ui 内のみで完結**」点を明記。
- マージ後の残 Phase 2 タスクは connection 管理 UI のみ
  (history 永続化は別 ADR で後続化を ADR-0014 で明示済)。

### Phase 2 config 層 PR #6 マージクローズ (本セッション末 / 2026-06-03)

### Phase 2 config 層 PR #6 マージクローズ (本セッション末 / 2026-06-03)

- PR #6 (`feature/config-store` → `develop`) マージ済 = `00756d7`
  (GitHub 上で merge commit、squash ではない)。
- ローカル `feature/config-store` 削除済 (`git branch -d`、`42871db` was)。
  リモート側 branch 削除は人間担当。
- ローカル `develop` は `origin/develop` (= `00756d7`) と sync 済。
- memory 更新済: `dbboard-web-state.md` で desktop@00756d7 snapshot
  反映 (ADR-0012 + ADR-0013 の双方が contract 層に追加された旨)。

### Phase 2 config 層 (本セッション / 2026-06-03)

`develop` から `feature/config-store` を切って 5 commit 積み終え、156
tests green (1 ignored = live keyring)。Push は人間担当。

積んだ commit (古い順):

- `<adr>` ADR-0013 起票 (`docs/decisions.md` 末尾追記)。
- `<skel>` `crates/dbboard-config` skeleton + schema (serde-only, `kind`
  discriminator で Turso/D1/Postgres、CONFIG_VERSION=1)。
- `d7bc17c` `feat(config): load and persist connections.toml via the directories crate`。
- `76f22f9` `feat(config): keyring-backed SecretStore with in-memory fallback`。
- `<wire>` `apps/dbboard` 配線 + `docs/connections.md` 新設 + roadmap
  Phase 2 checkbox 更新。

実装の要点:

- `dbboard-core` の「no I/O」は保持。新クレート `dbboard-config` が
  TOML + keyring を抱え込む。
- `connections.toml` schema: `version=1`、`[[connections]]` per entry。
  D1/Postgres entry は **secret material を持たない** — `keyring_*_ref`
  でキーチェーン参照のみ。`tests/secrets.rs` で TOML 内に raw token /
  postgres URL が含まれないことを回帰テスト化。
- 保存は `*.tmp` → `fs::rename` で atomic、Unix のみ mode 0o600。
- `KeyringStore` = `keyring` crate v3.6.3 ラッパー、service 名は
  定数 `"dbboard"`。`InMemorySecretStore` を test/CI fallback として
  併設。live keyring test は `#[ignore]` (CLAUDE.md 必須 `cargo test
  --all-features` を緑保つため)。
- `dbboard-server::config` に `resolve_backend(env, file, secrets)`
  純粋関数 + `backend_config_from_env_and_store()` ラッパーを追加。
  既存 `backend_config_from_env()` は env-only として温存。
- 解決順: PG_URL > D1 trio > TURSO_PATH > `DBBOARD_CONNECTION=<id>`
  > 単一 entry 自動選択 > Turso `:memory:`。missing id は **silent
  fallback せず** ConfigError で startup 中断。
- `apps/dbboard/main.rs` を `load_or_empty + KeyringStore +
  backend_config_from_env_and_store` フローに刷新。`default_path()`
  失敗時は `ConnectionFile::empty()` で best-effort 続行 (CI/headless
  対応)。
- README に解決順サマリ追加、新規 `docs/connections.md` に schema /
  ファイル位置 / OS 別 secret seed 手順を記載。

### Phase 2 PR #5 マージクローズ (本セッション末 / 2026-05-27)

- PR #5 (`feature/adapter-trait-capability` → `develop`) マージ済 = `7f463ef`。
  GitHub 上で squash ではなく merge commit (CHANGELOG への影響なし、Phase 2
  は未 release)。
- ローカル + リモート `feature/adapter-trait-capability` 削除済。pre-push
  hook が release build + 132 tests を実行してから削除を通した。
- memory 更新済:
  - `dbboard-web-state.md` → desktop@7f463ef snapshot、delta-mirror waiting
    on web の状態を反映。
  - `MEMORY.md` index の dbboard-web エントリ description 更新。
- ローカル `develop` は `origin/develop` (= `7f463ef`) と sync 済。

### Phase 2 ブランチ実装完了 (本セッション後半 / 2026-05-27)

`develop` から分岐した `feature/adapter-trait-capability` 上に 5 commit
ぶんの Phase 2 を積み終え、132 tests green。Push は人間担当。

積んだ commit (古い順):

- `0dc9e17` `feat(core): introduce Capabilities discovery struct (ADR-0012)`
- `17e8a84` `feat(core): define DatabaseAdapter trait and capability markers`
- `5e46e99` `refactor(adapters): implement DatabaseAdapter trait and dispatch via Arc<dyn>`
- `1c350f6` `feat(server): add GET /capabilities and the capability error category`
- `f59107b` `docs: document GET /capabilities and queue web mirror brief`

Phase 2 実装の要点:

- `Capabilities` は flat snake_case (`has_views` / `has_functions` / `has_auth` /
  `has_storage` / `has_realtime`) で `Copy + serde`。
- `DatabaseAdapter` trait は `async-trait` で `Arc<dyn ...>` 共有可能。必須面は
  `id() -> &'static str` / `capabilities()` / `ping()` / `list_tables()` /
  `query()`。capability 用 `Option<&dyn ...>` accessor は **未配線** (Phase 2
  では capability 実装ゼロ、shape のみ定義の方針)。
- `dbboard-server::AppState` は `Arc<dyn DatabaseAdapter>` を 1 本だけ持つ。
  `Backend` enum 完全廃止、`backend.rs` は `connect_adapter` 1 関数のみ。
- `GET /capabilities` → `{ "id": "<adapter>", "capabilities": Capabilities }`。
  全アダプタ Phase 2 では全 flag `false`。
- `DbError::Capability(String)` を新設、HTTP 404 にマップ。`category()` /
  `message()` / `from_parts()` を更新済。
- UI scrub (#7) は **no-op で完了**: `dbboard-ui` / `apps/dbboard` を
  `Turso|D1|Postgres|Neon|Supabase|libsql` で grep → 0 件。Phase 1.5 の
  HTTP indirection で既に達成済だった。
- `docs/api-contract.md` に `GET /capabilities` セクション、`Capabilities`
  データ形状、`capability` エラーカテゴリ行を追記。
- `.claude/issues/0002-web-capabilities-mirror.md` を 0001 と同形式で作成
  (Phase 2 contract 追加分を dbboard-web に mirror する handoff)。

## 次の Phase 2 PR (human action)

- ローカル commit を push: `git push -u origin feature/adapter-trait-capability`
  (Norton で release build が遅くなる可能性あり、`env-windows-norton.md` 参照)。
- `develop` 向けに PR を出す。タイトル例: `feat: Phase 2 — DatabaseAdapter trait
  + Capability discovery (ADR-0012)`。
- PR body には上記 5 commit の役割と「**Phase 1 surface はゼロ変更、Phase 2
  は純粋に additive**」点を明記。Conformance test 範囲は変えていない。
- マージ後の sibling 作業は `.claude/issues/0002-web-capabilities-mirror.md`
  を起点に web リポへ持ち込む。

### v0.1.0 出荷完了 (本セッション前半)

- PR #3 (`feature/dev-hardening-husky-deny` → `develop`) マージ済 = `9de9f67`。
- PR #4 (`develop` → `main`, release for v0.1.0) マージ済 = `84c08be`。
- `v0.1.0` git tag 作成 + push 済。CHANGELOG.md の `[0.1.0]` リンクは resolve 済。
- 旧 feature branch (`feature/dev-hardening-husky-deny`) は local 削除済。
  remote の削除は人間にお任せ (GitHub 上で stale branch クリーンアップ)。

## Phase 2 タスク (本ブランチで一括 PR — 実装完了)

ADR-0012 に従い 1 PR にまとめた。実装はすべて完了、push 待ち。

1. ✅ status / memory 同期 (`v0.1.0` 出荷反映)。
2. ✅ `Capabilities` struct 定義 (`0dc9e17`)。
3. ✅ `DatabaseAdapter` trait 定義 (`17e8a84`)。
4. ✅ 3 アダプタを trait に migration + `Backend` enum 解体 (`5e46e99`)。
   元の task 4/5 は compile-time に分離不能 (循環) と判明し 1 commit に統合。
6. ✅ `GET /capabilities` + `DbError::Capability(404)` (`1c350f6`)。
7. ✅ UI scrub (no-op で完了; Phase 1.5 ですでに達成済)。
8. ✅ `docs/api-contract.md` 改訂 + `.claude/issues/0002-web-capabilities-mirror.md`
   起票 (`f59107b`)。

## 直近の作業 (前セッション後半 / 2026-05-26)

- **環境復旧**: Norton と推測される AV が `C:\Users\syste\AppData\Roaming\npm\
  node_modules\@anthropic-ai\claude-code\bin\claude.exe` を `.old.<timestamp>` に
  リネーム → web 側で `claude` CLI が起動しなくなった。`bin/claude.exe` に
  リネームし直して復旧 (タイムスタンプから推測: 2026-05-04 頃)。
- **進捗確認 + ステータス整合性チェック**: 既存 `.claude/project-status.md`
  が「Option 1 シーケンス未実行」前提で書かれていたが、実 git log で見ると
  v0.1.0 出荷は既に完了 (PR #3 / #4 マージ済、tag 済) と判明 → 本ファイルを
  全面リライト。
- **dbboard-web 側に PWA pivot brief 発見**: `dbboard-web/.claude/handoff/
  2026-05-26-pwa-pivot-incoming.md` が未追跡で存在。「`dbboard-web` を PWA 化し
  ambient mobile 需要を吸収、native アプリは作らない」方針。**この brief は
  HTTP contract に依存しないため、desktop Phase 2 と並行で web 側 Claude が
  独立に進める**。desktop の Phase 2 タスクには影響なし。
- **Phase 2 ブランチ作成**: `develop` を sync → 旧 feature branch を local 削除
  → `feature/adapter-trait-capability` を develop から作成。

## 過去の作業 (参考)

### v0.1.0 出荷 (本セッション前半 / 前セッションからの繰り越し)

- `feature/dev-hardening-husky-deny` 上に積んでいた以下を `develop` → `main`
  経由で出荷:
  - `chore(security)`: `cargo-deny` を `deny.toml` で設定し pre-push に組込 (`6ae8652`)。
  - `chore(husky)`: 削除のみの push では release build/test をスキップ (`8b4ebe7`)。
  - `docs(policy)`: ADR-0011 で SemVer + tiered DB support を採択、
    `docs/compatibility.md` 新設 (`bad80e0`)。
  - `chore(release)`: ワークスペース版を `0.1.0` に bump、`CHANGELOG.md` 新設、
    roadmap.md Phase 1/1.5/1.6/1.7 に ✅ done (`456045f` `99ff580`)。
  - `docs(adapter)`: ADR-0012 で Capability パターンを採択 — 必須最小面 +
    `Option<&dyn ...>` でぶら下げる任意 capability。HTTP は `/views` `/auth` などで
    階層化、新エラーカテゴリ `capability` (`46d1d16`)。
  - `docs`: README / architecture.md を 0.1.0 実態に同期 (`264d68e`)。
  - `chore(handoff)`: dbboard-web Phase 1 contract-mirror brief (`939fe22`)。

### 結果セット行数上限 (security HIGH 解消、Phase 1.7)

- `dbboard-core::limits::MAX_RESULT_ROWS = 10_000`。超過時は `DbError::Query` で
  エラー (切り捨てない)。3 アダプタ全てに反映、`docs/api-contract.md` に明文化。

### Phase 1.6 (Cloudflare D1) / Phase 1.7 (PostgreSQL/CockroachDB)

- `dbboard-d1`: REST `/raw` ベースの HTTP クライアント (rustls、https-only)。
- `dbboard-postgres`: pg-wire 汎用アダプタ (sqlx 0.8 + tls-rustls-ring)。
  ADR-0002 を ADR-0008 で修正、pg-wire 互換 DB は単一アダプタ共有方針。
  TLS `sslmode=Prefer` → `Require` 昇格で平文フォールバック防止。

### Phase 1.5 (ローカル HTTP backend)

- `dbboard-server` (axum 0.8) 新設。`dbboard-ui` が HTTP クライアント (reqwest) を保持。
- `dbboard-core` に serde derive 常時付与 (Value 手書き、`Blob` は `{"$blob":"<base64>"}`)。

## 注意点・既知の問題

- `develop` がデフォルトブランチ。Phase 2 完了時は `feature/adapter-trait-capability`
  → `develop` の PR を出す。release タグ運用は v0.1.0 で確立済 (`develop` → `main`
  release PR → tag push)。
- WEB 版 (`meta-taro/dbboard-web`) と同時並行で進めない、というルールは
  **「同じ contract layer」に限定**して運用する。今回の PWA pivot は contract に
  触らないため、desktop Phase 2 と並行可 (web 側 Claude が独立に担当)。
- Push は人間が実行する。エージェントは commit までで止めること。
- **Norton AV が claude.exe を quarantine するパターン**: pre-push の release build
  だけでなく、`@anthropic-ai/claude-code` の bin/claude.exe 本体も `.old.<timestamp>`
  にリネームされる事例を確認 (本セッション)。再発したら同じ手順 (リネームし戻し →
  ダメなら `npm i -g @anthropic-ai/claude-code` 再インストール)。Norton の例外設定
  追加も検討余地あり。memory `env-windows-norton.md` 更新候補。
- **GitHub Desktop の push が `remote: fatal error in commit_refs` で失敗するケース**:
  PowerShell `git push -v origin <branch>` でリトライすると通る。原因は GitHub Desktop
  と git CLI の細かい挙動差 or タイミング起因と推測。

## 開発ペースに関するメモ

- 二つのリポジトリを同時に同じ contract layer で進めない (Roadmap の Pacing Note 参照)。
- contract (アダプタ shape、エラー区分、スキーマスナップショット形状) の変更は
  両 repo の `docs/decisions.md` に ADR を書いてから着手する。
- 機能パリティは目標であって強制ではない。desktop 側で先に新アダプタを実装し、
  必要に応じて web 側に展開するリズムで進める想定。
- ただし **contract に触らない strategic な変更** (PWA pivot 等) は両 repo 並行で
  進めて OK。web 側の判断と進捗は web 側 Claude セッションに委譲。
