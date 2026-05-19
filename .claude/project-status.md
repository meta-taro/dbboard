# プロジェクトステータス (内部メモ・日本語)

このファイルは作業中のセッション状態を記録する内部用ドキュメント。
外部公開向けの内容ではないため日本語で書く。セッション終了時に更新する。

## 最終更新

- 日付: 2026-05-19
- ブランチ: `develop` (リモートデフォルト・追跡中)
- 現在の Phase: Phase 1 (Turso 縦割り) を開始する前段階

## 直近の作業

- ルール・規約・ドキュメント整備の初期セットアップを実施
  - `CLAUDE.md` (英語、OSS 向け統合ルール)
  - `README.md` `DESIGN.md` `docs/architecture.md` `docs/roadmap.md` `docs/decisions.md` を作成
  - `.gitignore` (日本語コメント)
  - `.claude/issues/` のテンプレ整備
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
