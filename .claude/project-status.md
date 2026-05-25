# プロジェクトステータス (内部メモ・日本語)

このファイルは作業中のセッション状態を記録する内部用ドキュメント。
外部公開向けの内容ではないため日本語で書く。セッション終了時に更新する。

## 最終更新

- 日付: 2026-05-25
- ブランチ: `feature/dev-hardening-husky-deny` (`develop` から分岐)
- 現在の Phase: **Phase 1 / 1.5 / 1.6 / 1.7 完了。workspace を `0.1.0` に bump し、
  Phase 2 (アダプタトレイト + Capability) のドラフトに入る直前。**

## 直近の作業 (このセッション)

- **バージョニング & DB サポート方針の確立 (ADR-0011) + Phase 1 クローズアウト**
  - 論点 2 つを ADR-0011 にまとめた:
    1. dbboard 本体のバージョニング: **SemVer**。公開 API は HTTP contract のみ
       (`docs/api-contract.md`)。内部クレートは `publish = false` のままで SemVer 対象外。
       `0.1.0` を Phase 1 クローズと同じ commit で切る。`1.0.0` は HTTP contract が
       `dbboard-web` と相互運用検証済み + Capability モデル (ADR-0010 予定) 完成が条件。
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

1. 本セッションの 2 コミット (`docs(policy)` と本コミット) を push (人間)。
2. **ADR-0010 (Capability パターン) のドラフト** — Phase 2 のトレイト抽出と一体で設計する
   ため、コードに触る前に決め切る。前回会話で骨子は提示済 (`DatabaseAdapter` 必須メソッド
   + `views()`/`auth()`/`storage()` 等の能力アップキャスト + `/capabilities` エンドポイント)。
3. `docs/api-contract.md` の 10,000 行上限ルール (Phase 1.7 で追記) を `dbboard-web` に
   ミラー (人間)。
4. Phase 2 着手: `dbboard-core` に `DatabaseAdapter` + Capability トレイト群を定義し、
   `dbboard-server::Backend` enum を `Arc<dyn DatabaseAdapter>` に置換。UI から `Turso`
   ワードを完全に消す (Phase 2 exit criteria)。
5. 上限を緩める検討は Phase 2 後半 (ストリーミング / ページネーション API)。UI 側の
   「LIMIT 自動付与」「ページめくり」UX は別 ADR 候補。

## 注意点・既知の問題

- `develop` がデフォルトブランチ。今後の機能実装は `feature/...` を切ってから
  `develop` に PR でマージする運用に揃える (ADR-0005 参照)。
- WEB 版 (`meta-taro/dbboard-web`) と同時並行で進めない。スプリント単位で交互に進める。
- Push は人間が実行する。エージェントは commit までで止めること。
- Rust toolchain はインストール済 (cargo 1.95.0)。cargo-husky の git hooks も導入済。

## 開発ペースに関するメモ

- 二つのリポジトリを同時に同じ層で進めない (Roadmap の Pacing Note 参照)。
- 契約 (アダプタ shape、エラー区分、スキーマスナップショット形状) の変更は
  両 repo の `docs/decisions.md` に ADR を書いてから着手する。
- 機能パリティは目標であって強制ではない。デスクトップ側で先に新アダプタを
  実装し、必要に応じて WEB 側に展開するというリズムで進める想定。
