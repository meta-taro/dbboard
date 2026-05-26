# プロジェクトステータス (内部メモ・日本語)

このファイルは作業中のセッション状態を記録する内部用ドキュメント。
外部公開向けの内容ではないため日本語で書く。セッション終了時に更新する。

## 最終更新

- 日付: 2026-05-26
- ブランチ: `feature/dev-hardening-husky-deny` (`develop` から分岐、現在 develop の 11
  コミット先行、main の 22 コミット先行 = `main` は initial commit のみで release 未受領)
- 現在の Phase: **Phase 1 / 1.5 / 1.6 / 1.7 完了 + 0.1.0 workspace bump 済。
  `dbboard-web` 側も Phase 1 (pnpm + Nuxt 4 + NestJS 11 + `/health` smoke) 完了し
  baton が desktop に戻った。branch 戦略は Option 1 を確定 — 現ブランチを develop に PR、
  続けて develop → main の release PR を切り、main で `v0.1.0` をタグ付けして
  CHANGELOG リンクを resolve したうえで、`feature/adapter-trait-capability` を
  develop から新規に切って Phase 2 に着手する。本ファイル下「次のステップ
  (Option 1 シーケンス)」参照。**

## 次のステップ (Option 1 シーケンス)

ユーザの選択は「develop と main を両方進めて v0.1.0 を切る」。Phase 2 着手前に
長期化した `feature/dev-hardening-husky-deny` を完全に着地させる。

1. **未 push の 4 コミット (`264d68e` `0b68aad` `1ac67e9` + 本ステータス更新) を push**
   *(人間担当 — GitHub Desktop 経由が落ちる場合は PowerShell から
   `git push -v origin feature/dev-hardening-husky-deny`)*。
2. **PR #3: `feature/dev-hardening-husky-deny` → `develop`** を作成。本文ドラフトは
   下記「PR ドラフト #3」を参照。push 後、エージェントが `gh pr create` で開く想定。
   レビュー → マージは人間。
3. **PR #4: `develop` → `main` (release PR for v0.1.0)** を作成。PR タイトルは
   `release: v0.1.0`。本文は CHANGELOG `[0.1.0]` セクションの転記 + 0.1.0 で出荷した
   3 アダプタ + ADR-0011 (SemVer) + ADR-0012 (Capability) の参照。マージは人間。
4. **main で `git tag -a v0.1.0 -m "v0.1.0" <merge-sha>` + `git push origin v0.1.0`**
   *(人間担当)*。`CHANGELOG.md` の `[0.1.0]: .../releases/tag/v0.1.0` リンクがこれで resolve。
   必要なら `gh release create v0.1.0 --notes-from-tag` で GitHub Release も発行。
5. **`develop` 最新で `feature/adapter-trait-capability` を新規に切る** → Phase 2 着手。
   最初のチケットは `crates/dbboard-core` への `DatabaseAdapter` トレイト + `Capabilities` 型定義。
   `/capabilities` 実装が完了したら `docs/api-contract.md` を改訂し、`939fe22` 形式の
   handoff brief を `.claude/issues/0002-*.md` に起こして dbboard-web に baton を渡す。

### PR ドラフト #3 (`feature/dev-hardening-husky-deny` → `develop`)

- **Title**: `chore: dev hardening, 0.1.0 release, and ADR-0012 capability pattern`
- **Summary 要点** (本文 HEREDOC 化):
  - `chore(security)`: `cargo-deny` を `deny.toml` で設定し pre-push に組み込み (`6ae8652`)。
  - `chore(husky)`: 削除のみの push では release build/test をスキップ (`8b4ebe7`)。
  - `docs(policy)`: ADR-0011 で SemVer + tiered DB support を採択、
    `docs/compatibility.md` を新設 (`bad80e0`)。
  - `chore(release)`: ワークスペース版を `0.1.0` に bump、`CHANGELOG.md` 新設、
    roadmap.md Phase 1/1.5/1.6/1.7 に ✅ done (`456045f` `99ff580`)。
  - `docs(adapter)`: ADR-0012 で Capability パターンを採択 — `DatabaseAdapter` 必須
    最小面 + `Option<&dyn ...>` でぶら下げる任意 capability。HTTP は `/views` `/auth`
    などで階層化、新エラーカテゴリ `capability` (`46d1d16`)。
  - `docs`: README / architecture.md を 0.1.0 実態に同期 (`264d68e`)。
  - `chore(status)` / `chore(handoff)`: dbboard-web Phase 1 contract-mirror brief
    (`939fe22`)、セッション status 更新 (`075a879` `89b7c70` `0b68aad` `1ac67e9`)。
- **Test plan**:
  - [x] pre-commit (`cargo fmt --check` / `clippy -D warnings` / `check` / `test`) green per commit。
  - [x] pre-push (`cargo build --release` / `cargo test --release`) green at push time。
  - [x] `cargo deny check` green (license + advisory).
  - [ ] CI on PR for develop branch green (待ち)。

## 直近の作業 (このセッション)

- **dbboard-web Phase 1 完了報告を受領 (2026-05-26)**
  - web 側で実施された内容 (ユーザ報告):
    1. **Phase 1 close**: pnpm workspace + Nuxt 4 + NestJS 11 monorepo scaffold +
       smoke `GET /health`、PR #1 で `develop` にマージ済 (merge commit `1c204ed`)、
       feature branch は local/remote 両方削除。
    2. **Contract mirror complete**: `dbboard-web/docs/api-contract.md` が
       `dbboard@89b7c70` (= 本リポジトリの最新時点) と byte-content-identical。
       最終 contract 変更は `3f114e4` (10,000 行 cap)。web 側 `.prettierignore` で
       再フォーマットから保護。
    3. **3 つの policy ADR が web 側に着地**: ① desktop HTTP API contract を Phase 1 入力として
       採用、② branch 方針 `feature/<slug>` → PR → `develop` (desktop ADR-0005 と一致)、
       ③ self-host-only OSS 配布 (Docker Compose + ghcr.io、メンテナ自身がホストする SaaS は
       提供しない)。
    4. **Web は待機状態**: 残 issue `0003` (NestJS HTTP surface)、`0004` (Postgres adapter)、
       `0005` (row cap + body limit + conformance tests) はすべて open のままで、desktop が
       次の contract 変更を publish するまで未着手。
  - desktop 側で必要な action: ① roadmap.md の Pacing Note を「web Phase 1 完了、baton 復帰」に
    更新 (済: 本コミットで反映)、② branch 戦略の決定 (`feature/dev-hardening-husky-deny` は
    main 比 49 ファイル/+10,778 行に膨らんでおり、Phase 2 をここに乗せると更に長期化する)、
    ③ Phase 2 着手 (`/capabilities` 実装後は `docs/api-contract.md` を改訂 + web 側へ
    `939fe22` 形式の handoff brief を出す)。

- **進捗監査とドキュメント実態同期 (2026-05-26)**
  - 監査結果: `README.md` と `docs/architecture.md` が 0.1.0 実態より遅れていた。
    - README L36-40: 「Turso adapter ships first, followed by Cloudflare D1...」と
      逐次出荷の未来形のまま。実態は Turso/D1/Postgres 全て 0.1.0 で shipped。
    - architecture.md L34-38: 「Phase 1 ships dbboard-core/turso/ui」「dbboard-server
      lands in Phase 1.5」「Adapter crates beyond Turso land in Phase 3」と shipped 済
      を未来形で記述。
    - architecture.md L72-93: 「The exact signature evolves as Phase 1 progresses」の
      トレイト sketch が ADR-0012 (Capability 拡張) を反映していなかった。
  - 修正: README Status 節は「Pre-1.0; workspace at 0.1.0 with Phase 1 closed」に書換え、
    CHANGELOG リンクを追加。Supported Databases 節は実態 (Turso/D1/Postgres shipped、
    Neon は同 Postgres adapter で動作、Supabase/Neon picker は Phase 2 以降) に書換え。
    architecture.md は phase 状況パラグラフを書換え、core trait sketch を ADR-0012 の
    `Capabilities` + Optional accessor 形に更新。
  - **commit** (`264d68e`): `docs: sync README and architecture.md to the 0.1.0 reality`。
  - その他確認: `.env.example` は最新で OK。`docs/decisions.md` の ADR 番号は
    0001-0009, 0011, 0012 (0010 はスキップ済、append-only 尊重)。orphan ADR-0010 参照は
    本ファイルの meta-note 1 件のみで、これは「リネームしたこと」を記録する
    意図的な記述。`docs/compatibility.md` の `Phase 3` 言及は Neon picker 残作業の
    正しい future 記述。
  - **既知のギャップ (未対応)**: `git tag --list` 空。`CHANGELOG.md` の
    `[0.1.0]: ...releases/tag/v0.1.0` は GitHub release tag を前提だが、CLAUDE.md の
    GitFlow (develop → PR → main → tag) では release PR が main にマージされてから
    タグを切る運用なので、現時点で tag を作るのは早い。`feature/dev-hardening-husky-deny`
    → `develop` → `main` 経由で 0.1.0 を切る段で `git tag v0.1.0` + tag push する想定。
    詳細は「注意点・既知の問題」参照。

- **dbboard-web ハンドオフ準備 (`.claude/issues/0001-web-contract-mirror.md`)**
  - 方針判断: 当初は「web は desktop API を真似るから待ち」だったが、contract が
    `0.1.0` で安定し ADR-0011 が 1.0.0 リリース条件として web 相互運用検証を要求している
    ため、もう web を稼働させるべきタイミングと判断。ADR-0012 (`/capabilities`) は
    既存 3 エンドポイントを壊さない追加なので、今 web が現行 contract を実装しても
    Phase 2 完了後に追加実装するだけで作り直しは発生しない (forward-compatible)。
  - 起草物 `.claude/issues/0001-web-contract-mirror.md` (140 行): scope (現行 3 エンドポイント
    + Value/QueryResult/Column/TableInfo + エラー envelope + request-level rejection
    + 10,000 行 cap) / out of scope の明示 (`/capabilities` と `capability` カテゴリ
    は desktop 未実装ゆえ除外) / Acceptance (NestJS + Postgres 1 アダプタ +
    contract-conformance test) / Tech 推奨 (class-validator で 422/400 切り分け、
    body limit 64 KiB、TLS ハードニング)。
  - `.claude/issues/` の慣行 (NNNN-kebab-slug.md, status header, テンプレート遵守) に従う。
  - **commit** (`939fe22`): `chore(handoff): seed the dbboard-web Phase 1 contract-mirror brief`。
  - 本ファイル `.claude/project-status.md` も「次のステップ」を「push → web ミラー → Phase 2」に
    書き直し、commit `075a879` (`chore(status): record ADR-0012 and queue the contract mirror`)
    で記録。

- **ADR-0012: Capability パターンによる per-DB 拡張性のドラフト**
  - 論点: Phase 2 のトレイト抽出に入る前に「PostgreSQL ビュー / Supabase auth /
    Storage / Realtime のような DB 固有機能を後付け追加できる設計」を確定する必要があった。
  - 決定 (3 点):
    1. **必須コア + 任意 Capability** の二層トレイト構造。`DatabaseAdapter` には id /
       capabilities / ping / introspect / query のみ必須。`ViewIntrospection` /
       `FunctionIntrospection` / `AuthAdmin` / `StorageAdmin` / `RealtimeChannels` は
       `Option<&dyn ...>` を返すアクセサ経由で公開し、デフォルト実装は `None`。
       未対応 DB のコードは一切変更不要。
    2. **HTTP も同じ層構造に揃える**。能力ごとに URL プレフィックス
       (`/views/...` `/functions/...` `/auth/...` `/storage/...` `/realtime/...`) を割り当て、
       未対応エンドポイントは新カテゴリ `capability` (HTTP 404) で拒否。
       新エンドポイント `GET /capabilities` でクライアントが事前に能力フラグを取得できる。
    3. **`Backend` enum を `Arc<dyn DatabaseAdapter>` に縮退**。アダプタ追加は
       `BackendConfig::connect` の一箇所だけ触ればよくなる。`async-trait` クレートを
       採用 (AFIT は dyn 互換性が未成熟なため)。
  - 命名: 「Capability」はオブジェクト指向領域の業界共通語で、Spring Data / DataFusion /
    PostgreSQL FDW でも同名で用いられている。DDD の「能力」訳語より誤解が少ない。
  - **commit** (`46d1d16`): `docs(adapter): adopt capability pattern for per-DB extensibility`
    (`docs/decisions.md` に ADR-0012 を append + `docs/roadmap.md` Phase 2 参照を ADR-0012 へ更新)。
  - 既存の `ADR-0010 予定` 等の参照は ADR-0012 にリネーム済 (append-only ルール尊重)。
  - 検証: pre-commit フック (fmt/clippy/check/test) 緑、130 テスト緑。

- **バージョニング & DB サポート方針の確立 (ADR-0011) + Phase 1 クローズアウト**
  - 論点 2 つを ADR-0011 にまとめた:
    1. dbboard 本体のバージョニング: **SemVer**。公開 API は HTTP contract のみ
       (`docs/api-contract.md`)。内部クレートは `publish = false` のままで SemVer 対象外。
       `0.1.0` を Phase 1 クローズと同じ commit で切る。`1.0.0` は HTTP contract が
       `dbboard-web` と相互運用検証済み + Capability モデル (ADR-0012) 完成が条件。
    2. DB サポート: **Tier 制**。Tier 1 (CI/ローカル live test 緑) / Tier 2 (互換だが未自動化) /
       Best effort。サーバ系 DB はメジャー N と N-1。マネージド系はベンダ最新 API + 固定 client crate。
  - 新規ファイル `docs/compatibility.md` をサポート行列の正本に。README からリンク。
  - **commit 1** (`bad80e0`): `docs(policy): adopt SemVer and tiered DB version support`
    (ADR-0011 追記 + compatibility.md 新設 + README リンク追加)。push 済 (人間)。
  - **ロードマップ実態同期 + 0.1.0 bump + CHANGELOG 新設** (本セッション 2 つ目の commit):
    - `docs/roadmap.md`: Phase 1 の 6 項目を `[x]`、Phase 1 / 1.5 / 1.6 / 1.7 に ✅ done と
      日付を付与、`*(current)*` マーカーを Phase 2 へ移動。Pacing Note の "Right now" を
      2026-05-25 に更新。
    - `Cargo.toml`: `version = "0.0.0"` → `"0.1.0"` (ADR-0011 の約束どおり Phase 1 クローズ
      commit で bump)。
    - `CHANGELOG.md` 新設: Keep a Changelog 形式。`[0.1.0]` セクションに Added / Security /
      Documentation で 0.1.0 スコープを retrospective に記録。
    - `.claude/project-status.md` を本セッション内容で更新 (このファイル)。
  - 検証: 全テスト緑 (130 件)。pre-commit フックは fmt / clippy / check / test を緑で通過。

## 過去の作業 (参考)

- **結果セット行数上限を全アダプター共通で導入 (security HIGH の解消) + 関連 MEDIUM/LOW**
  - 設計判断 3 点 (ユーザー確認済): ① 超過時は `DbError::Query` でエラー (切り捨てない)、
    ② 上限 10,000 行、③ `dbboard-core` の定数として共通化。
  - 5 コミット (各コミットで `cargo fmt --check` / `clippy -D warnings` / `check` / `test`
    の pre-commit フックが緑):
    1. `feat(core)`: `dbboard-core::limits` モジュールを追加。`MAX_RESULT_ROWS = 10_000`
       と `too_many_rows_error()` ヘルパを公開。+ 2 ユニットテスト。
    2. `feat(turso)`: `run_select` で 1 行ずつ push する前に上限チェック。あわせて
       (a) `connect_local` のエラーをパスでスクラブする `redact_path` ヘルパ、
       (b) `is_row_returning` のブロックコメント (`/* ... */`) バイパス修正 (`first_token`
       を導入して `--` 行コメント・ブロックコメント・空白を反復スキップ)。
       in_memory.rs に at-cap / over-cap / path 漏洩なし の 3 統合テストを追加。
    3. `feat(d1)`: REST `/raw` のレスポンスを `QueryResult` に変換する直前に
       envelope の長さで上限を弾く。あわせて `transport_error` ヘルパで
       `reqwest::Error::without_url()` 経由で URL を除去 (account_id / database_id /
       D1 ホスト名がエラー envelope に漏れない)。`rest_roundtrip.rs` に
       `https_only(true)` 拒否で URL/account_id/database_id 非漏洩を確認する
       統合テスト 1 件を追加。
    4. `feat(postgres)`: `sqlx::raw_sql` のストリーミングループ内、`row_to_values`
       を呼ぶ前に上限チェック。`pg_roundtrip.rs` に `generate_series(1, N)` で
       at-cap / over-cap の 2 統合テストを追加 (`DBBOARD_PG_URL` 未設定時は self-skip)。
    5. `docs(api-contract)`: `docs/api-contract.md` の Transport 節に上限ルール
       (10,000 行 / 超過時は HTTP 400 `query` カテゴリ / Phase 2 で streaming or pagination 検討)
       を明文化。dbboard-web へミラーする際の根拠資料。
  - 共通方針: 上限ヒットは「サイレントに切り詰めない」。`UI` が見えない truncate に
    気付かないリスクを避けるため、必ず `DbError::Query` → HTTP 400 で返す。
    エラーメッセージは「`add a LIMIT clause to narrow it`」を含めユーザーに行動を示唆。
  - 検証: 全クレート緑 (合計 120 テスト: core 35 / d1 21+3 / postgres 9+4 /
    server 3+9 / turso 13+8 / ui 15)。CockroachDB ライブ統合テストも
    `DBBOARD_PG_URL` 設定で実行・合格。
  - 残課題: 前 PR から繰り延べた「dbboard-web へ api-contract ミラー」は引き続き
    人間担当 (今回追記した上限ルールを web 側にも反映する)。

- **ローカル HTTP バックエンドを導入 (Phase 1.5 / ADR-0006・ADR-0009)** — PR #1 にて
  `develop` にマージ済 (2026-05-23)。設計判断 3 点 (ユーザー確認済): ① dbboard-ui
  が HTTP クライアントを所有 (worker + reqwest を ui へ移設、egui は同期なので
  Command/Reply チャンネルは存続)、② dbboard-core に serde derive を常時付与
  (serde は I/O ではないので core の no-I/O 維持)、③ ブランチ
  `feature/local-http-backend`。6 コミット: `chore` deps → `feat(core)` serde derive
  (`Value` 手書き Serialize/Deserialize、Blob は `{"$blob":"<base64>"}`、`DbError`
  に `category()`/`message()`/`from_parts()`) → `feat(server)` `crates/dbboard-server`
  新設 (axum 0.8 / DefaultBodyLimit 64KiB / graceful shutdown) → `feat(ui)` worker を
  ui へ移設し HTTP 化 → `refactor(app)` main.rs を `serve()` + `connect()` に書換え
  → `docs` ADR-0009 + `docs/api-contract.md` + roadmap/architecture/README 更新。
  ランタイム 2 つ (サーバー=multi-thread、UI worker=current-thread の別スレッド)。
  詳細はマージ済 PR #1 の commit history 参照。

- **CockroachDB 対応を追加 (Phase 1.7 / ADR-0008)**
  - 新クレート `crates/dbboard-postgres`: PostgreSQL ワイヤープロトコル汎用アダプター。
    CockroachDB が最初の接続先で、Neon/Supabase の SQL 経路も同クレートを再利用予定
    (ADR-0002 の「DB ごとに1クレート」を ADR-0008 で「pg-wire 互換 DB は単一アダプター
    共有」に修正)。`sqlx` 0.8 + `tls-rustls-ring` (OpenSSL 非依存)。
  - 動的デコード: `sqlx::raw_sql` (simple query protocol) で全列をテキスト表現で受け取り
    `Value::Text`、NULL は `Value::Null`。`dbboard-core` の Value 型 (5 変種) は無変更。
    追加 decode features 不要で uuid/numeric/jsonb/array/custom 型まで全カバー。
  - イントロスペクションは `information_schema.tables` (`pg_catalog`/`information_schema`/
    `crdb_internal` 除外) で `schema.table` (`TableInfo::qualified`)。
  - `apps/dbboard/src/main.rs`: `Backend`/`BackendConfig` に Postgres 変種を追加。env 駆動
    で `DBBOARD_PG_URL` を最優先 → D1 → ローカル Turso `:memory:`。UI は無変更。
  - security-reviewer / rust-reviewer を実行し指摘を反映:
    - **TLS ハードニング** (security HIGH): sqlx 既定の `sslmode=Prefer` は平文フォール
      バックでパスワード平文送信のリスク。`connect` で URL を parse し `Prefer`→`Require`
      に昇格 (明示的な `disable` 等は尊重)。`harden_ssl_mode` を 2 件のユニットテストで担保。
    - **decode_cell の不変条件ガード** (rust HIGH): テキストフォーマット前提を `debug_assert`
      で明示し、将来の binary プロトコル混入を検知。
    - `query` のドキュメントに single-statement 前提を明記。
    - URL/パスワード非漏洩は実装時から担保済 (pool のみ保持、`Configuration` エラーは
      固定文字列化)。`configuration_error_hides_the_url` テストで検証。
  - 純粋関数 (エラー分類・SSL モード・introspection マッピング・truncate) を 9 件の
    ユニットテストでカバー。実 CockroachDB への疎通テスト `pg_roundtrip.rs` は
    `DBBOARD_PG_URL` 未設定時スキップ (今回は設定済で 2 件とも実行・合格)。
  - `docs/decisions.md` (ADR-0008)、`docs/roadmap.md` (Phase 1.7)、`README.md`、
    `.env.example` を更新。
  - 検証: `cargo fmt --check` / `clippy -D warnings` / `check` / `test` 全て緑。
    (`cargo-audit` はローカル未インストールのためスキップ)
- **Cloudflare D1 アダプターを追加 (Phase 1.6 / ADR-0007)**
  - 新クレート `crates/dbboard-d1`: D1 は外部からは REST API 経由でしか触れない
    ため、`reqwest` (rustls, https-only) で `/raw` エンドポイントを叩く HTTP
    クライアント実装。`connect`/`ping`/`list_tables`/`query` で `TursoAdapter`
    のメソッド面をミラー (トレイト抽出は ADR-0003 に従い Phase 2 へ繰り延べ)。
  - `apps/dbboard/src/main.rs`: `Backend { Turso, D1 }` enum を導入し env 駆動で
    バックエンド選択 (`DBBOARD_D1_ACCOUNT_ID`/`_DATABASE_ID`/`_TOKEN` が揃えば D1、
    無ければ従来どおりローカル Turso `:memory:`)。UI は無変更。
  - 純粋関数 (envelope→QueryResult, JSON→Value, エラー分類) を 19 件のユニット
    テストでカバー。実 D1 への疎通テストは `DBBOARD_D1_*` 未設定時スキップ。
  - security-reviewer / rust-reviewer を実行し指摘を反映: https-only + rustls 明示、
    空トークン即エラー、429/5xx は Connection 分類、エラー文字列の上限長切り詰め、
    未使用 `thiserror` 依存削除、未テスト分岐の追加。
  - `docs/decisions.md` (ADR-0007)、`docs/roadmap.md` (Phase 1.6)、`README.md`、
    `.env.example` を更新。
  - 検証: `cargo fmt --check` / `clippy -D warnings` / `check` / `test` 全て緑。
- WEB 版 (`dbboard-web`) との関係性を整理し、独立コードベース + 概念共有という方針を ADR-0004 に記録

## 次のステップ

1. ~~未 push のコミットを push (人間)~~ **完了** (2026-05-25): `bad80e0..939fe22` の
   6 コミット全て origin に到達済。`git push -v` で通った (GitHub Desktop の通常 push は
   `commit_refs` エラーで失敗していたが、CLI verbose 経由でリトライしたら成功。
   詳細は「注意点・既知の問題」参照)。
2. **`docs/api-contract.md` を `dbboard-web` にミラー (人間担当・別リポジトリ)** — Pacing Note の
   交互スプリント順序に従い、デスクトップ側の Phase 2 着手前に web 側で同契約を反映する。
   今回ミラーすべき差分は ① 10,000 行上限ルール (Phase 1.7 追記)、② エラー envelope の
   `message` が category prefix を含まない仕様、③ Request-level rejection の HTTP code 表、
   の 3 点。ADR-0012 で新設予定の `/capabilities` および `capability` エラーカテゴリは
   **まだコードに無いのでミラー対象外**。Phase 2 が contract を実装した段階で改めて
   contract 改訂 → web ミラーの流れで進める。詳細ブリーフは
   `.claude/issues/0001-web-contract-mirror.md` (commit `939fe22` で push 済)。
   `dbboard-web` 側のセッションを起こす際は `dbboard/.claude/issues/0001-web-contract-mirror.md`
   と `dbboard/docs/api-contract.md` (snapshot at `939fe22` 以降) をコンテキストに渡す。
3. Phase 2 着手 (上記 2 完了後):
   - `dbboard-core` に `DatabaseAdapter` トレイトと `Capabilities` 構造体、5 つの
     Capability トレイト (`ViewIntrospection` / `FunctionIntrospection` / `AuthAdmin` /
     `StorageAdmin` / `RealtimeChannels`) を ADR-0012 に従って定義。
   - `dbboard-server::Backend` enum を `Arc<dyn DatabaseAdapter>` に置換し、
     `BackendConfig::connect` に分岐を集約。
   - 既存 3 アダプタ (Turso/D1/Postgres) を新トレイトの impl 形に書き換える。
     既存メソッド面はそのまま流用できる設計なので、機能変更ではなく差し替え。
   - UI から `Turso` ワードを完全に消す (Phase 2 exit criteria)。
   - `GET /capabilities` 実装と `capability` エラー (404) を追加 → contract ドキュメント改訂 →
     web 側にミラー依頼。
4. Phase 2 以降の余地:
   - Connection management UI (add / edit / delete)、TOML config + OS keychain、
     Query history (Phase 2 完了基準ではないが Phase 2 に含めるか別 Phase に切るか要判断)。
   - 10,000 行上限の緩和 (ストリーミング / ページネーション) は Phase 2 後半 or 別 Phase。
     UI 側の「LIMIT 自動付与」「ページめくり」UX は別 ADR 候補。

## 注意点・既知の問題

- `develop` がデフォルトブランチ。今後の機能実装は `feature/...` を切ってから
  `develop` に PR でマージする運用に揃える (ADR-0005 参照)。
- WEB 版 (`meta-taro/dbboard-web`) と同時並行で進めない。スプリント単位で交互に進める。
- Push は人間が実行する。エージェントは commit までで止めること。
- Rust toolchain はインストール済 (cargo 1.95.0)。cargo-husky の git hooks も導入済。
- **GitHub Desktop の push が `remote: fatal error in commit_refs` で失敗するケース**
  (2026-05-25 セッションで発生): GitHub status は all green、他リポジトリの push は通る、
  pre-push フックも `[pre-push] ok` で抜けるが、最後の ref 更新だけ落ちる。secret pattern や
  サイズ問題は無し。**回避策: PowerShell から `git push -v origin <branch>` でリトライ**で
  通った。原因は GitHub Desktop と git CLI の細かい挙動差 or タイミング起因と推測。
  再現したら CLI で `-v` 付き push が最短手段。
- **`v0.1.0` git tag は未作成 (意図的)**: CHANGELOG.md は GitHub release URL
  (`...releases/tag/v0.1.0`) を前提に link を張っているが、CLAUDE.md の GitFlow
  (`develop` → release PR → `main` → tag) に従うと release commit を `main` に乗せて
  から tag するのが正しい順序。現在 `feature/dev-hardening-husky-deny` ブランチ上に
  release commit (`456045f`) があり、まだ `develop` にも `main` にもマージされていない。
  CHANGELOG link は `develop` → `main` 経由で 0.1.0 を切る段で `git tag v0.1.0 <sha>` +
  `git push origin v0.1.0` を実行した時点で resolve される予定。それまで link は壊れた
  状態だが GitFlow 上は正常。

## 開発ペースに関するメモ

- 二つのリポジトリを同時に同じ層で進めない (Roadmap の Pacing Note 参照)。
- 契約 (アダプタ shape、エラー区分、スキーマスナップショット形状) の変更は
  両 repo の `docs/decisions.md` に ADR を書いてから着手する。
- 機能パリティは目標であって強制ではない。デスクトップ側で先に新アダプタを
  実装し、必要に応じて WEB 側に展開するというリズムで進める想定。
