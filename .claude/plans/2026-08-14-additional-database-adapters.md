# dbboard 追加データベース対応 企画草案
## Agent共有用

## 1. 目的

dbboard の対応データベースを拡張し、単なるSQLクライアントではなく、

> Local-first database client for humans and AI agents

として利用可能なデータソースを広げる。

既に対応済みの MongoDB / Firestore は本企画の対象外とする。

今回の追加候補：

```text
Priority 1
├─ DuckDB
├─ Microsoft SQL Server / Azure SQL
└─ Redis / Valkey

Priority 2
├─ ClickHouse
└─ Elasticsearch / OpenSearch

Demand-driven
└─ Oracle Database
```

---

## 2. 基本方針

対応DBを単純に増やすだけではなく、以下の価値が明確なDBを優先する。

- Desktop GUI
- MCP
- AI Agent
- Read-only / Safe mode
- Schema / Structure inspection
- Query
- Troubleshooting
- SSH Tunnel
- Cloud DB access

特に、

> AI Agent が人間の代わりにデータを調査する価値が高いか

を採用基準にする。

---

# 3. Priority 1: DuckDB

## 3.1 位置付け

最優先候補。

DuckDB はサーバー型DBだけでなく、ローカルファイル分析用途に強い。

dbboard の Local-first / MCP という思想と非常に相性が良い。

## 3.2 想定用途

```text
CSV
TSV
Parquet
DuckDB file
↓
dbboard
↓
MCP
↓
AI Agent
```

Agentへの指示例：

```text
このParquetで一番売上が高い商品を調べて
このCSVの異常値を探して
過去30日のログをendpoint別に集計して
このデータセットのカラム構造を説明して
```

## 3.3 UI案

```text
New Connection

DuckDB

○ DuckDB Database File
○ CSV
○ TSV
○ Parquet

File:
[ Select File ]
```

ローカルファイルを直接開けるUXを重視する。

## 3.4 MCP Tool候補

```text
duckdb.open_file
duckdb.list_tables
duckdb.describe
duckdb.query
duckdb.preview
duckdb.profile
```

既存dbboard共通MCPへ統合できるなら、DB固有Toolを増やしすぎず共通抽象化する。

## 3.5 企画上の価値

DuckDBを追加するとdbboardを、

> DBサーバーへ接続するアプリ

から、

> ローカルデータファイルをAI Agentに分析させるアプリ

へ拡張できる。

---

# 4. Priority 1: Microsoft SQL Server / Azure SQL

## 4.1 位置付け

企業案件向けの重要DB。

MySQL / PostgreSQL系だけでなく SQL Server を追加することで、業務システム・エンタープライズ領域の対応力を高める。

## 4.2 対象

```text
Microsoft SQL Server
Azure SQL Database
Azure SQL Managed Instance
```

可能な範囲で同一Connector系として扱う。

## 4.3 必須候補

- Database一覧
- Schema一覧
- Table / View
- Column
- Index
- Primary Key / Foreign Key
- Stored Procedure
- Function
- Query
- Explain / Execution Plan
- Read-only
- TLS
- SSH / network tunnel

## 4.4 SQL Server固有検討

- T-SQL
- IDENTITY
- TOP
- NVARCHAR
- uniqueidentifier
- datetime2
- Stored Procedure
- schema ownership
- Windows Authentication
- Entra ID / Azure authentication

初期版では全機能を網羅せず、通常のSQL認証＋基本Schema inspectionから開始してよい。

## 4.5 MCP用途

```text
このDBのテーブル構造を説明して
このStored Procedureが参照しているテーブルを調べて
usersテーブルに関連するFKを一覧にして
このSQLが遅い原因候補を調査して
```

---

# 5. Priority 1: Redis / Valkey

## 5.1 位置付け

Redis対応は単なるDB追加より、

> AI Agentによる本番環境トラブル調査

の価値が高い。

Valkeyも同一系統として対応候補にする。

## 5.2 対象

```text
Generic Redis
Generic Valkey

AWS
├─ Amazon ElastiCache for Redis OSS / Valkey
└─ Amazon MemoryDB
```

クラウド別にDriverを完全分離するのではなく、Redis/Valkey Connectorに接続Presetを追加する方向を優先する。

## 5.3 Connection UI案

```text
New Connection

Redis / Valkey

Preset:
○ Generic Redis / Valkey
○ AWS ElastiCache
○ AWS MemoryDB

Host:
Port:

TLS:
[ ]

Authentication:
○ None
○ Username / Password
○ AUTH Token

Mode:
○ Standalone
○ Cluster

Network:
○ Direct
○ SSH Tunnel
```

## 5.4 AWS接続

ElastiCache / MemoryDB はVPC内に配置されるケースが中心なので、ローカルPCから接続する場合はネットワーク経路が必要になる。

dbboard側では既存または将来のSSH Tunnelと組み合わせる。

```text
dbboard
↓ SSH Tunnel
EC2 / Bastion
↓ VPC
ElastiCache / MemoryDB
```

またはVPN経由。

## 5.5 Redis UI

RDBとはUIを分ける。

```text
Redis
├─ Keys
├─ Type
├─ TTL
├─ Value
├─ Memory
└─ Namespace / Prefix
```

対応Type候補：

- String
- Hash
- List
- Set
- Sorted Set
- Stream
- JSON（利用可能な場合）

## 5.6 Agent用途

```text
session:* のKey数を調べて
TTLが1時間未満のKeyを調査して
このユーザーのSessionが残っているか確認して
Memoryを大量消費しているKeyを探して
Queueが詰まっていないか調べて
特定prefixだけ確認して
```

## 5.7 安全設計

Redisは削除・flushが非常に危険。

標準MCPでは以下を禁止または強制確認。

```text
FLUSHALL
FLUSHDB
DEL
UNLINK
CONFIG SET
SCRIPT
MODULE
```

基本はRead-only。

書き込みを許可する場合も既存Write Policyへ統合する。

---

# 6. Priority 2: ClickHouse

## 6.1 位置付け

Analytics / Logs / Observability向け。

AI Agentによる大量データの集計・調査と非常に相性が良い。

## 6.2 想定用途

```text
Application Logs
Event Logs
Analytics
Metrics
Observability
Large datasets
```

Agent例：

```text
昨日のHTTP 500をendpoint別に集計して
過去30日のp95 response timeを調べて
エラーが急増した時刻を特定して
このイベントテーブルの異常値を探して
```

## 6.3 UI

SQL型なので既存dbboard UIを比較的流用しやすい。

考慮対象：

- Database
- Table
- View
- Materialized View
- Engine
- Partition
- Order By
- TTL
- Column compression
- Distributed table

## 6.4 MCP

共通SQL Toolを基本とする。

追加候補：

```text
clickhouse.table_engine
clickhouse.partition_info
clickhouse.system_query
```

---

# 7. Priority 2: Elasticsearch / OpenSearch

## 7.1 位置付け

検索・ログ・Observability・Vector用途。

MongoDB対応でNoSQL抽象化の経験があるため、その延長として検討する。

## 7.2 対象

```text
Elasticsearch
OpenSearch
Amazon OpenSearch Service
```

## 7.3 UI案

```text
Cluster
├─ Indices
│  ├─ Mapping
│  ├─ Settings
│  ├─ Documents
│  └─ Stats
├─ Aliases
├─ Templates
└─ Nodes
```

## 7.4 Agent用途

```text
error levelのログを直近1時間で集計して
このIndexのmappingを説明して
特定trace_idを追跡して
検索結果が0件になる原因をmappingから調べて
Vector fieldの設定を確認して
```

## 7.5 Query

SQLではなく、

- Query DSL
- REST API
- OpenSearch DSL

が中心になる。

AI Agentとの親和性は高いが、dbboard共通SQL抽象化から外れるためPriority 2とする。

---

# 8. Demand-driven: Oracle Database

## 8.1 位置付け

企業需要は大きいが、実装・検証・配布面のコストが高い可能性がある。

利用要望が出てから対応でもよい。

## 8.2 必須候補

- Schema
- Table
- View
- Sequence
- Trigger
- Procedure
- Package
- Index
- Constraint
- Explain Plan

## 8.3 Agent用途

```text
このPackageの依存関係を調べて
このテーブルに関連するTriggerを確認して
遅いSQLのExecution Planを説明して
```

---

# 9. 現時点で追加しないもの

既に対応済み：

```text
MongoDB
Firestore
```

本企画では追加対象から除外する。

---

# 10. 推奨優先順位

```text
P1
1. DuckDB
2. SQL Server / Azure SQL
3. Redis / Valkey
   ├─ Generic
   ├─ AWS ElastiCache
   └─ AWS MemoryDB

P2
4. ClickHouse
5. Elasticsearch / OpenSearch
   └─ Amazon OpenSearch Service

Demand-driven
6. Oracle Database
```

---

# 11. 優先順位の理由

## DuckDB

最小の追加でdbboardの用途を大きく広げられる。

```text
CSV / Parquet / Local Data
↓
DuckDB
↓
MCP
↓
AI Analysis
```

「Local-first AI Database Client」というブランドに最も合う。

## SQL Server

一般DBクライアントとして対応範囲を大きく広げる。

企業ユーザーへの説得力が高い。

## Redis / Valkey

AIデバッグ・本番トラブル調査という明確な用途がある。

AWS ElastiCache / MemoryDBへ展開可能。

## ClickHouse

AIによるログ・大量データ分析との相性が強い。

## Elasticsearch / OpenSearch

検索・ログ・Vector時代のデータソースとして重要。

ただしRDBからUI/Queryモデルが離れる。

---

# 12. Connector Architecture

DBごとの実装を可能な限り分離する。

例：

```text
crates/
├─ dbboard-core
├─ dbboard-mysql
├─ dbboard-postgres
├─ dbboard-mongodb
├─ dbboard-firestore
├─ dbboard-duckdb
├─ dbboard-sqlserver
├─ dbboard-redis
├─ dbboard-clickhouse
└─ dbboard-opensearch
```

共通インターフェース：

```text
Connection
Schema / Structure
Browse
Query
Metadata
Read Policy
Write Policy
MCP
```

DB固有機能はExtensionとして追加する。

---

# 13. MCP設計

基本MCPはDBごとにバラバラにしすぎない。

可能なら共通Tool：

```text
connections.list
db.describe
db.list_objects
db.get_object
db.query
db.preview
db.explain
```

DB固有機能のみ追加：

```text
redis.ttl
redis.memory
clickhouse.partition_info
opensearch.mapping
sqlserver.stored_procedure
```

AgentがDB種類を過剰に意識しなくても調査できる設計を目指す。

---

# 14. Read-only を優先する

AI Agent用途では安全性が重要。

新Connectorは最初にRead-onlyを完成させる。

```text
Phase 1
Read / Inspect / Query

Phase 2
Safe Write

Phase 3
Advanced DB-specific operation
```

既存Write Policyへ統合し、

- DROP
- DELETE
- TRUNCATE
- FLUSH
- destructive command

は明示確認または禁止する。

---

# 15. Cloud DB対応

「DB種類」と「Cloud Provider」を分ける。

```text
Redis / Valkey Connector

Preset
├─ Generic
├─ AWS ElastiCache
└─ AWS MemoryDB
```

```text
SQL Server Connector

Preset
├─ Generic SQL Server
├─ Azure SQL
└─ Azure SQL Managed Instance
```

```text
OpenSearch Connector

Preset
├─ Generic OpenSearch
└─ Amazon OpenSearch Service
```

クラウド名ごとに別DB実装を作らない。

---

# 16. Agentへ依頼する初期調査

各Connectorの実装前にAgentは以下を調査する。

- Rust Driver候補
- License
- TLS
- Authentication
- SSH Tunnel適合性
- Cloud接続方式
- Schema Metadata取得
- Query API
- Read-only強制方法
- Write Policy
- MCP Toolへのマッピング
- Windows/macOS build
- CIでのIntegration Test方法
- DockerでのTest fixture可否

---

# 17. 実装順序案

## Phase 1: DuckDB

- [ ] Driver選定
- [ ] `.duckdb`接続
- [ ] CSV / TSV / Parquet直接参照
- [ ] Schema
- [ ] Query
- [ ] MCP
- [ ] Read-only
- [ ] Desktop file picker

## Phase 2: SQL Server

- [ ] Driver
- [ ] SQL Authentication
- [ ] TLS
- [ ] Schema
- [ ] Table / View
- [ ] Stored Procedure
- [ ] Query
- [ ] MCP
- [ ] Azure SQL検証

## Phase 3: Redis / Valkey

- [ ] Standalone
- [ ] TLS
- [ ] AUTH
- [ ] Key browser
- [ ] TTL
- [ ] Type表示
- [ ] MCP
- [ ] Read-only
- [ ] Cluster
- [ ] SSH Tunnel
- [ ] AWS ElastiCache検証
- [ ] AWS MemoryDB検証

## Phase 4: ClickHouse

- [ ] Driver
- [ ] Metadata
- [ ] Query
- [ ] Table Engine
- [ ] Partition
- [ ] MCP

## Phase 5: Elasticsearch / OpenSearch

- [ ] REST Connector
- [ ] Index
- [ ] Mapping
- [ ] Documents
- [ ] Search
- [ ] MCP
- [ ] Amazon OpenSearch検証

## Phase 6: Oracle

要望が出た場合に着手。

---

# 18. 認知拡大との連携

追加DBはそれぞれ記事・README・Directory露出の材料になる。

例：

```text
Using Claude Code to analyze Parquet files with DuckDB and MCP
```

```text
Inspect AWS ElastiCache safely from Claude Code with MCP
```

```text
Query ClickHouse logs from an AI agent
```

```text
Explore SQL Server schemas with Claude Code through MCP
```

単なる、

```text
dbboard now supports Redis
```

ではなく、用途ベースの記事にする。

---

# 19. 最終的な製品像

```text
dbboard

Relational
├─ MySQL / MariaDB
├─ PostgreSQL
├─ SQL Server
└─ Oracle

Local Analytics
├─ SQLite
└─ DuckDB

Document / Cloud
├─ MongoDB
└─ Firestore

Cache / Realtime
└─ Redis / Valkey

Analytics / Logs
└─ ClickHouse

Search / Observability
└─ Elasticsearch / OpenSearch
```

これらを、

```text
Desktop GUI
+
MCP
+
AI Agent
+
Read-only Safety
+
SSH / Cloud Connectivity
```

で統一して扱う。

---

# 20. 企画の核

対応DB数を競うことが目的ではない。

> 人間がGUIで確認でき、AI AgentがMCP経由で安全に調査できるデータソースを増やす。

これを基準に拡張する。

現時点の最優先：

```text
DuckDB
SQL Server
Redis / Valkey
```

次点：

```text
ClickHouse
Elasticsearch / OpenSearch
```

Oracleは需要ベースで判断する。
