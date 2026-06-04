# プロジェクトステータス (内部メモ・日本語)

このファイルは作業中のセッション状態を記録する内部用ドキュメント。
外部公開向けの内容ではないため日本語で書く。セッション終了時に更新する。

## 最終更新

- 日付: 2026-06-04 (本セッション末、Phase 3 Neon ADR-0018 PR #11
  マージ完了、ローカル feature ブランチ削除済)
- ブランチ: `develop` (= `origin/develop` = `c249bc4`、sync 済)
- 現在の Phase: **Phase 3 Neon adapter (flavored kind over
  `dbboard-postgres`) シップ完了。PR #11 マージ済 (`c249bc4`、
  merge commit `c249bc4`)、ローカル `feature/neon-adapter-kind`
  ブランチ削除済。次セッションは Phase 3 残り = Supabase アダプタ
  (REST + sqlx hybrid) 着手、あるいは web 側 Claude が
  `0003-web-history-schema-mirror` を pickup した場合の
  cross-repo フォローアップ対応。**

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
