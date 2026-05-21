# プロジェクトステータス (内部メモ・日本語)

このファイルは作業中のセッション状態を記録する内部用ドキュメント。
外部公開向けの内容ではないため日本語で書く。セッション終了時に更新する。

## 最終更新

- 日付: 2026-05-21
- ブランチ: `feature/turso-vertical-slice`
- 現在の Phase: Phase 1.6 (Cloudflare D1 アダプター) 実装完了。未コミット。

## 直近の作業

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

1. cargo workspace 骨格を作成
   - `Cargo.toml` (workspace)
   - `crates/dbboard-core/` (空の lib)
   - `apps/dbboard/` (空の binary, cargo-husky 配置先)
2. `cargo-husky` を導入し pre-commit / pre-push フック設定
3. `develop` ブランチに切り替えてから commit する運用に移行
4. Phase 1 開始: Turso 接続の最小スライス実装

## 注意点・既知の問題

- `develop` がデフォルトブランチ。今後の機能実装は `feature/...` を切ってから
  `develop` に PR でマージする運用に揃える (ADR-0005 参照)。
- WEB 版 (`meta-taro/dbboard-web`) と同時並行で進めない。スプリント単位で交互に進める。
- Push は人間が実行する。エージェントは commit までで止めること。
- **Rust 未インストール**: ローカル環境に cargo がない状態でブートストラップを commit した。
  Rust toolchain をインストールしたら `cargo test` を 1 回走らせて cargo-husky の
  git hooks を `.git/hooks/` にインストールすること (それまで pre-commit / pre-push は無効)。

## 開発ペースに関するメモ

- 二つのリポジトリを同時に同じ層で進めない (Roadmap の Pacing Note 参照)。
- 契約 (アダプタ shape、エラー区分、スキーマスナップショット形状) の変更は
  両 repo の `docs/decisions.md` に ADR を書いてから着手する。
- 機能パリティは目標であって強制ではない。デスクトップ側で先に新アダプタを
  実装し、必要に応じて WEB 側に展開するというリズムで進める想定。
