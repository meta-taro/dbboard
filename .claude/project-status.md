# プロジェクトステータス (内部メモ・日本語)

このファイルは作業中のセッション状態を記録する内部用ドキュメント。
外部公開向けの内容ではないため日本語で書く。セッション終了時に更新する。

## 最終更新

- 日付: 2026-06-03 (本セッション末、Phase 2 接続管理 UI ADR-0016 Stage 1
  PR #9 マージ済 + 次セッション ADR-0017 方針合意済)
- ブランチ: `develop` (ローカル 1 ahead = `d9a7ba2`、push は人間担当)
- 現在の Phase: **Phase 2 接続管理 UI (ADR-0016 Stage 1) シップ完了。
  次セッションは ADR-0017 (query history persistence, Stage 2) の起票
  から開始。形式は JSON Lines (ndjson) で、record schema を desktop と
  web で共有 (HTTP contract には触れない、on-disk schema のみ揃える)
  方針までユーザと合意済。**

## 次セッション開始タスク: ADR-0017 起票

ユーザと合意済の前提:

- **形式**: JSON Lines (`.jsonl`、1 行 1 JSON、改行 LF)。動機は
  `jq` / `tail -F` / grep がそのまま使えること。HeidiSQL (registry) /
  DBeaver (SQLite) / DataGrip (text) / TablePlus (SQLite) のどれも
  jq-friendly ではないため、これが差別化点になる。
- **共有のスコープ**: **record の schema (field 名・型・semantics)
  だけ** を desktop と web で揃える。保存場所・ローテーション戦略・
  書き込み実装は各 repo 裁量。
- **HTTP contract には触らない**: `GET /history` は追加しない (ユーザ
  確認済)。理由は (1) endpoint 化すると web の access control 設計が
  contract に染み出す、(2) jq でファイル直読みできることが UX の核、
  (3) Stage 3 で necessary になれば additive に追加可能。
- **共有の置き場所**: ADR-0017 本文内に schema を single source of
  truth として置く。`docs/api-contract.md` ミラーは作らない (これは
  HTTP layer 専用)。web 側 sibling ADR は「desktop ADR-0017 と同一
  schema」とだけ書く形。

### 合意済 record schema (両 repo 共通)

```jsonc
{
  "v": 1,                              // schema version (forward-compat)
  "ts": "2026-06-03T14:22:01.123Z",   // RFC 3339 UTC、ローカル変換は jq 側
  "conn": "prod-pg",                   // connection id (TOML primary key)
  "actor": null,                       // desktop null 固定、web は session/user id
  "sql": "SELECT * FROM users LIMIT 10",
  "status": "ok" | "error",
  "duration_ms": 42,
  "rows": 10,                          // SELECT のとき、それ以外 null
  "rows_affected": null,               // DML のとき、SELECT は null
  "error": null                        // status=error のとき {category, message}
}
```

- `"v":1` で forward-compat 確保 (ADR で「field rename は breaking
  change」と明記する)。
- `"actor"` は desktop 側 day-1 から null 固定で書き込む (後付けすると
  過去ログとの不整合が出る)。
- `"error"` は ADR-0009 / `DbError` の 5 カテゴリ
  (`connection` / `query` / `schema` / `type_conversion` /
  `capability`) を category として持つ。

### ADR-0017 で詰める論点

- **保存場所 (desktop)**: `directories` crate で
  `$XDG_DATA_HOME/dbboard/history.jsonl` (Linux) /
  `~/Library/Application Support/dbboard/history.jsonl` (macOS) /
  `%APPDATA%\dbboard\history.jsonl` (Windows)。`dbboard-config` の
  `default_path` と同居させるか、別 module に切るか要検討。
- **ローテーション**: 50MB or 100k 行で `history.jsonl.1` に rotate
  (`tracing-appender` 等の既存実装に乗せる)。
- **書き込みタイミング**: クエリ実行直後の async append (UI を
  block しない、最悪 1 件失っても致命的ではない)。
- **secret を含むクエリの扱い**: 完全保存 (ローカルファイル、暗号化
  なし)。README に「ローカル DB ファイルと同等の扱い」と明記。web 側
  は team 利用文脈で別判断 (web ADR 裁量)。
- **読み込みポリシー**: 起動時に末尾 N 行 (`DEFAULT_CAPACITY=100`) を
  tail して in-memory `HistoryStore` に注入 → UI からは Stage 1 と
  同じ API で見える (= UI コード変更ゼロ)。
- **migration**: `"v"` を見て古い行を捨てずに済むよう、reader 側で
  unknown field を ignore する Stage 1 から書いておく。

### 次セッションでの段取り

1. `feature/query-history-persistence` を `develop` から切る。
2. ADR-0017 を `docs/decisions.md` に append (上記 schema + 残論点の
   決着)。1 commit。
3. `crates/dbboard-ui::history::HistoryStore` に persistence layer を
   追加 (新規 module、Stage 1 の API は不変)。1〜2 commit。
4. `apps/dbboard` 配線 (起動時に load、`run_sql` 後に append)。
   1 commit。
5. `.claude/issues/0003-web-history-schema-mirror.md` を `0001`/`0002`
   と同形式で起票 → web 側へ。1 commit。
6. roadmap.md Phase 2 の history 行を「Stage 2 = persisted via
   ADR-0017」に更新、project-status.md を closeout。1 commit。
7. PR を `develop` へ。タイトル案: `feat: Phase 2 — query history
   persistence (ADR-0017, Stage 2)`。



### Phase 2 PR #9 マージクローズ (本セッション末 / 2026-06-03)

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
