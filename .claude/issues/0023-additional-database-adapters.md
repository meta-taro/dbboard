# 0023 — 追加データベースアダプターの企画

- 状態: 未着手（企画確定・着手順まで決定、実装は未）
- 出典: 2026-08-16 に共有された企画草案「dbboard 追加データベース対応」
- 関連: ADR-0018（Postgres 系のフレーバー方式）、0019 Firestore、0020 MongoDB、
  `docs/roadmap.md` Phase 6

## 採用基準

対応 DB の数を競わない。基準は 1 つだけ。

> 人間が GUI で確認でき、AI エージェントが MCP 経由で安全に調査できるデータソースを増やす。

## 候補と優先度

```
P1
1. DuckDB
2. Microsoft SQL Server / Azure SQL
3. Redis / Valkey（Generic / AWS ElastiCache / AWS MemoryDB）

P2
4. ClickHouse
5. Elasticsearch / OpenSearch（Amazon OpenSearch Service 含む）

需要ベース
6. Oracle Database
```

MongoDB・Firestore は対応済みのため対象外。PlanetScale は MySQL 互換で
`dbboard-mysql` から届くため新規アダプター不要（roadmap に記載済み）。

## 契約の確認結果（着手前に済ませた調査）

企画共有時に「Redis や OpenSearch は SQL 形でないので、先に HTTP 契約の変更が
必要ではないか」という懸念を出したが、**コードを読んで確認した結果これは誤りだった**。
記録として残す。

- 契約は `Adapter::query(&self, sql: &str) -> DbResult<QueryResult>`
  （`crates/dbboard-core/src/adapter.rs:40`）。**引数の中身が SQL である必要はない。**
  MongoDB は JSON のコマンドドキュメントをこの文字列として渡している
  （`crates/dbboard-mongodb/src/adapter.rs:205`）
- 読み取り専用の強制も、既定の Postgres パーサを使わず
  `query_read_only` をアダプター側で上書きできる（同 223 行）。
  **Redis はコマンド名の許可リストで足りるため、SQL の構文解析より確実に守れる**
- 値の表現も、Hash / List / Set / Sorted Set は Firestore / MongoDB 対応で入れた
  nested value（issue 0018）がそのまま使える

したがって Redis / OpenSearch は「契約を変える新種」ではなく、**すでに払った
コストを再利用する側**。`dbboard-web` への契約ミラー（v1.0 ゲート 2、現在延期中）
とも衝突しない。着手順を入れ替える理由は無い。

新しい `ConnectionKind` の追加は、Aurora DSQL のときと同じ **v=1 への追加バリアント**
として扱う（ADR-0018 / ADR-0019 の前例どおり）。

## Redis の必須要件 — `KEYS` を通さない

**企画草案の禁止リスト（FLUSHALL / DEL / CONFIG SET 等）だけでは足りない。**
破壊系より先に、**走査系**を止める必要がある。

`KEYS session:*` は Redis をシングルスレッドで全走査するため、本番でキーが
数百万あると**その間サーバー全体が応答を止める**。データは壊れないが、サービスは
落ちる。企画が想定している「冗長構成のセッションを本番で調査する」場面が、
まさにこれを踏む条件そのもの。

- `KEYS` は**アダプターが拒否する**。運用ルールでも UI の注意書きでもなく、実装で塞ぐ
- 代わりに `SCAN`（カーソル + `COUNT` 上限）だけを通す
- クラスタ構成では全シャードに対して SCAN を回す必要がある。カーソルはシャードごと
- 同じ理由で `SMEMBERS` / `HGETALL` / `LRANGE 0 -1` のような**要素数無制限の取得**も
  上限付きに置き換える

**AI が組み立てた文字列を人が見ないまま実行する MCP 経路がある以上、
「危険なコマンドを人が避ける」設計にはできない。**

## アダプターごとの着手前調査

各アダプターの実装前に、次を調べて `docs/decisions.md` に ADR として残す。

- Rust ドライバ候補とライセンス
- ビルド影響（**DuckDB は本体を同梱するドライバになる可能性がある。
  現在の exe が約 40 MB なので、増分は無視できない。要調査**）
- TLS / 認証方式（SQL Server は Windows 認証・Entra ID、Redis は AUTH トークン）
- SSH トンネル適合性（既存 `dbboard-tunnel` に乗るか）
- クラウド接続方式（ElastiCache / MemoryDB は VPC 内が中心）
- スキーマメタデータの取得方法（Redis / OpenSearch には宣言されたスキーマが無い。
  MongoDB と同じ**標本からの推論**として、推論であることが見て分かる形で出す）
- 読み取り専用の強制方法（エンジン側で効かせられるか、許可リストか）
- MCP ツールへの対応付け
- Windows / macOS ビルド、CI の統合テスト方法、Docker のテスト用フィクスチャ

## クラウドは「DB 種別」と分ける

クラウド名ごとに別実装を作らない。接続プリセットとして持つ。

```
Redis / Valkey  → Generic / AWS ElastiCache / AWS MemoryDB
SQL Server      → Generic / Azure SQL / Azure SQL Managed Instance
OpenSearch      → Generic / Amazon OpenSearch Service
```

Postgres 系で Neon / Supabase / Aurora DSQL を 1 つのアダプターのフレーバーとして
扱ったのと同じ方式（ADR-0018）。

## MCP は共通ツールを基本にする

エージェントが DB 種別を過剰に意識せず調査できることを優先する。

```
共通: connections.list / db.describe / db.list_objects / db.query / db.preview
固有: redis.ttl / redis.memory / clickhouse.partition_info / opensearch.mapping
```

固有ツールは、共通で表現できないものだけ足す。

## 各アダプターの段階

どのアダプターも読み取りを完成させてから書き込みに進む。

```
段階 1  接続 / 一覧 / 構造 / 参照 / 読み取り専用 / MCP
段階 2  既存 Write Policy への統合（安全な書き込み）
段階 3  DB 固有の操作
```

## 実装順

1. **DuckDB** — `.duckdb` 接続、CSV / TSV / Parquet の直接参照、ファイル選択 UI
2. **SQL Server** — SQL 認証 + TLS + 基本の構造参照から。T-SQL 固有機能は後段
3. **Redis / Valkey** — Standalone → キーブラウザ（SCAN 強制）→ TTL / 型 → MCP →
   クラスタ → SSH トンネル → ElastiCache / MemoryDB 検証
4. **ClickHouse**
5. **Elasticsearch / OpenSearch**
6. **Oracle** — 要望が出てから

## 完了条件（アダプター 1 つあたり）

- 接続 / テーブル（相当）一覧 / 構造 / 参照 / クエリが GUI で通る
- `query_read_only` がそのエンジンに適した方法で強制されている
- **Redis は `KEYS` と無制限取得を拒否することがテストで固定されている**
- MCP から同じ接続で調査できる
- Docker でテスト用フィクスチャが立ち、CI の統合テストが回る
- ADR が `docs/decisions.md` に 1 本ある
- `docs/test-specs/` に検証シートが 1 枚ある

## 補足 — 製品像の表記

企画草案 §19 の最終形で SQLite が Local Analytics に挙がっている。dbboard は
ローカルの SQLite ファイルを **`dbboard-turso`（libSQL）経由で開ける**ので実態としては
正しいが、独立した SQLite アダプターがあるわけではない。対外的な表記を作るときは
この区別を落とさないこと。
