# プロジェクトステータス (内部メモ・日本語)

このファイルは作業中のセッション状態を記録する内部用ドキュメント。
外部公開向けの内容ではないため日本語で書く。セッション終了時に更新する。

## 最終更新

- 日付: 2026-05-21
- ブランチ: `feature/turso-vertical-slice`
- 現在の Phase: Phase 1.7 (CockroachDB / 汎用 dbboard-postgres アダプター) 実装完了。未コミット。

## 直近の作業

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

1. Phase 1.7 の作業を commit する (英語のコンベンショナルコミット。push は人間)。
2. Phase 1.5 (ローカル HTTP バックエンド / ADR-0006) または Phase 2 (アダプタトレイト
   抽出) のどちらを先に進めるか判断する。具象アダプターが 3 つ (Turso/D1/Postgres)
   揃ったので Phase 2 のトレイト設計の入力は十分。
3. 実 CockroachDB での手動 E2E (`cargo run -p dbboard` でサイドバー/SELECT/DML 表示)。

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
