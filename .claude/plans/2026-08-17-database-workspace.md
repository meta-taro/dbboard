# dbboard Database Operations / Migration Rehearsal / Topology 拡張企画

> Status: Draft
> Scope: Local-first / Desktop-first
> Purpose: 各DB固有機能、運用、移行リハーサル、性能検証、冗長構成監視を一連で扱えるDatabase Workspaceへdbboardを拡張する。

## 1. 企画の核

dbboardの価値を「何種類のDBに接続できるか」だけで評価しない。

> 接続したDBを、そのDBらしく理解し、安全に運用・検証できること。

Migrationでは、

> 移行できることではなく、何分かかり、何が変わり、性能がどうなり、問題時に何分で戻せ、本当に元へ戻ったかまで証明する。

## 2. 全体レイヤー

- Universal DB: Tables / Query / Schema / Data
- Native DB: Procedures / Functions / Triggers / Events / Extensions / PRAGMA / RLS / FTS / Replication
- Compatibility: Version-aware案内 / Cross-DB近似機能 / Migration互換性
- Operations: Scheduler / Backup / Restore / Maintenance / Health Check
- Storage: Local / S3 / Cloudflare R2 / MinIO
- Database Lab: Load Test / Migration / Validation / Performance Compare / Rehearsal
- Topology: Replication / Cluster / Lag / Health / Failover
- Agent: AI / MCP
- Audit: Activity Timeline / Execution History / Markdown・JSON Report

## 3. DB固有機能

### MySQL / MariaDB
Views、Stored Procedures、Functions、Triggers、Event Scheduler、EXPLAIN、Replication、Binlog、Variables/Status、Users/Privileges。

### PostgreSQL
Views、Materialized Views、Functions、Procedures、Triggers、Extensions、RLS/Policies、Sequences、LISTEN/NOTIFY、Streaming/Logical Replication、EXPLAIN ANALYZE。

### SQLite
PRAGMA、WAL、VACUUM、ANALYZE、ATTACH DATABASE、FTS5、JSON、Triggers、Backup/Snapshot、Integrity Check。

Adapterはsupports_replication、supports_backup、supports_native_scheduler等のCapabilityを宣言し、UIを動的に構成する。

## 4. Compatibility Advisor

接続先のEngine、Version、Edition、Extension、有効機能を認識する。巨大なマニュアル閲覧画面ではなく、AI grounding、Feature availability、Deprecated警告、Version upgrade、Migration互換性判定に使う。

Cross-Database Feature Translatorを用意し、たとえばMySQL EVENTからPostgreSQLのpg_cronまたはdbboard Scheduler、AUTO_INCREMENTからIDENTITY/SEQUENCE、SQLiteのFULLTEXTからFTS5など、近似・代替機能を案内する。

「存在しない」で終わらず、Native機能、Extension、dbboard機能の順で代替候補を示す。

## 5. Scheduler / Automation

原則は `Native if available, dbboard automation if not.`

Scheduled Query、Scheduled Backup、Export、VACUUM/ANALYZE、Health Check、Integrity Check、Schema Snapshot、Row Count、Storage Upload、Notificationを扱う。

## 6. Backup / Restore / Verification

DBごとに適切な方式を利用する。MySQL/MariaDBはdump、PostgreSQLはpg_dump/pg_restore、SQLiteはBackup API/Snapshot等。

成功判定を終了コードだけに依存しない。File存在、Size、SHA-256、Manifest、Schema fingerprint、Row Count、必要ならTest Restoreまで検証する。

## 7. Storage Explorer

WindowsへのS3マウントだけに依存せず、dbboard自身がS3互換APIを扱う。

- Local
- AWS S3
- Cloudflare R2
- MinIO

Explorer形式でBucket/Prefix/Objectを閲覧し、DBとの関連を持たせる。Backup now、Restore、Verify、Download、Delete、Open in Storage Explorerを提供する。

DuckDB等との連携でS3上のCSV/TSV/JSON/Parquetを直接Queryする拡張も検討する。

## 8. Database Lab / Load Test

既存Benchmark toolを活用し、dbboardは統一UI・計測・比較・履歴を担当する。

Metrics:
- TPS / Queries/sec / Rows/sec
- Average latency / p50 / p95 / p99
- Error rate / Connections
- CPU / Memory
- Disk Read/Write
- Network
- DB Size

Time × CPU、Memory、Disk I/O、Network、TPS、Latency、Rows/secをグラフ化する。

Index追加、Version upgrade、Engine migration等についてBefore/After比較を保存する。

## 9. Migration Pre-flight

移行前に以下をBaselineとして保存する。

- Schema Snapshot
- Row Count
- Checksums
- DB Size
- Indexes / Constraints
- Native Features
- Replication state
- Performance baseline
- Backup

## 10. Migration計測

Started、Completed、Duration、Transferred bytes、Transferred rows、Rows/sec、Errors、Retriesを保存する。

テストデータ量と所要時間から本番移行時間を推定できるようにするが、単純比例による断定はせず推定値として扱う。

Migration中もCPU、Memory、Disk I/O、Network、Connections、Throughput、Latency、Error rateを継続計測・グラフ化する。

## 11. Migration Validation / Data Compare

Migration完了だけでは成功としない。

Schema、Row Count、Checksum、Sampling、Constraints、Indexes、Views、Native Features、User-defined verification SQLを検証する。

比較レベル:
1. Row Count / Schema fingerprint
2. Sampling hash / Key range
3. FullまたはChunk checksum

巨大DBでは検証コストを選択可能にする。

## 12. Migration Rehearsal

ローカル環境を中心に以下を一連で実行する。

1. Pre-flight
2. Backup
3. Compatibility Scan
4. Test Environment準備
5. Migration
6. Data Validation
7. Load Test
8. Performance Compare
9. Topology Validation
10. Report
11. Rollback Test
12. Rollback Verification

Dockerとの連携を重視し、`Existing DB → Docker Target → Migration → Validation → Benchmark → Rollback → Destroy` を実現する。

目的は「本番移行前に何回でも安全に失敗できる場所」。

## 13. Migration Session History

RehearsalをRun単位で保存する。

- #001 Failed
- #002 Readiness 74%
- #003 Readiness 91%
- #004 Readiness 100%

Run間でData/Schema差分、Duration、Performance、Resource usage、Errors、Compatibility、Rollback結果を比較する。

## 14. Migration Readiness

BLOCKER / WARNING / SAFEと根拠を表示する。単一Scoreだけで安全性を断定しない。

例:
- incompatible procedure
- view差分
- index差分
- p95悪化
- table verification率
- row match率

## 15. Rollback Test

Rollbackを正式なRehearsal工程として扱う。

方式候補:
- Backup Restore
- Snapshot Restore
- File replacement
- Transaction rollback（可能なケース）
- Container reset

Rollback Started / Completed / Duration / Restore bytes / Errorsを記録し、「何分で戻せたか」を測定する。

## 16. Rollback Verification

Restore成功とRollback成功を分離する。

Rollback後にPre-flight BaselineとSchema fingerprint、Row Counts、Checksums、Sample hashes、Indexes、Constraints、Verification SQL、Backup hashを比較する。

`Rollback completed: YES / Rollback verified: YES / Match: 100% / Duration: 5m14s`

のように表示する。不一致テーブルがあれば明示する。

## 17. Database / Replication Topology

冗長構成を図として表示する。

例:

    Primary
    ├─ Replica A  lag 320ms  healthy
    ├─ Replica B  lag 2.4s   warning
    └─ Replica C             disconnected

Multi-hopも表現する。

    Primary → Replica A → Reporting DB

各Edgeにlag等を持たせ、どのReplication Pathがボトルネックか視覚化する。

## 18. Topology Metrics

Node/EdgeごとにRole、Health、Replication state、Replication lag、WAL/binlog position、Last heartbeat、Connections、CPU、Memory、Disk I/O、Transaction rate、Network、Uptimeを取得可能な範囲で表示する。

対象:
- MySQL/MariaDB Primary/Replica
- PostgreSQL Streaming/Logical Replication
- Redis/Valkey Replication/Sentinel/Cluster
- MongoDB Replica Set
- CockroachDB Nodes/Regions
- Aurora等のCluster topology

## 19. Replication Lag Graph

Time × Replication Lag、WAL distance、CPU、Disk I/O、Transactionsをグラフ化する。

ThresholdをProject/DB単位で設定可能にする。AIは「lag増加とDisk Write Latency上昇が同時発生」等を原因候補として提示し、断定はしない。

## 20. Failover Visibility

Current Primary、Replica candidates、Failover state、Last failover、Election status、Healthを表示する。

初期は自動Failover操作よりRead-only visibilityを優先する。

## 21. Migration Topology Validation

Migration前後でNode数、Role、Replication path、Health、Lag、Failover configurationを比較し、冗長構成が正しく再現されたか検証する。

## 22. Activity Timeline / Execution History

Human / AI / MCP / Automation / Systemを区別して、SQL、AI操作、MCP call、Backup、Restore、Migration、Validation、Load Test、Topology event、Failover、Storage operationを時系列一本で記録する。

例:

    10:02 Human      Rehearsal started
    10:03 Automation Backup started
    10:05 Storage    S3 upload verified
    10:06 DB         Migration started
    10:18 DB         Migration completed
    10:23 AI         Compatibility issue analyzed
    10:31 Human      Rollback requested
    10:36 DB         Restore completed
    10:39 Validator  Rollback verified

## 23. Reports

Migration、Benchmark、Backup、Topology、Rollbackの結果をMarkdown / JSONへ出力しGit管理しやすくする。

Migration ReportにはSource/Target/Version、Dataset size、Duration、Resource summary、Compatibility、Schema/Data validation、Performance comparison、Topology、Rollback duration/verification、Readiness、Errors、Recommended actionsを含める。

## 24. AI / MCP

AI例:
- 「このMySQLをPostgreSQLへ移行した場合の問題を調べて」
- 「今回のRehearsalが前回より遅い理由を分析して」
- 「Replica Bのlagが増えている原因候補を調べて」
- 「このRollbackは本当に元の状態に戻っている？」

MCP候補:
- get_database_capabilities
- get_database_version
- get_topology
- get_replication_status
- get_replication_lag
- run_health_check
- create_backup / verify_backup / restore_backup
- run_migration_test
- run_validation
- run_load_test
- compare_runs
- run_rollback_test
- get_activity_history
- generate_report

Write / Restore / Migration等はApproval必須を基本とする。

## 25. Safety

Read-only mode、Explicit approval、Dry-run、Backup before write、Target environment warning、Local rehearsal mode、Timeout、Row/Size limit、Activity log、Destructive operation detectionを設ける。

## 26. Local-first

初期段階で巨大なクラウドMigration Serviceを目指さない。

Local DB、Docker DB、User-controlled remote DB、Local Backup、User-owned S3/R2/MinIOを中心とし、dbboard運営側がユーザーDBデータを預からない構成を基本とする。

## 27. UI案

Connection:
- Data
- Schema
- Native
- Query
- Topology
- Performance
- Automations
- Backups
- Database Lab
- Activity

Database Lab:
- Benchmark
- Migration
- Validation
- Rehearsals
- Reports

DashboardではVersion、Connections、Replication Health、Max Lag、CPU、Memory、Last Backup、Backup Verified、Last Rehearsal等を一画面に出す。

## 28. 実装Phase

### Phase 1
Activity Timeline / Execution History / Metrics collector / Charts / Adapter Capability API

### Phase 2
MySQL・PostgreSQL・SQLite Native Features

### Phase 3
Backup / Restore / Verification / S3 / R2 / MinIO / Storage Explorer

### Phase 4
Scheduler / Scheduled Query / Backup schedule / Maintenance / Retention

### Phase 5
Topology / MySQL replication / PostgreSQL replication / Lag graph / Health

### Phase 6
Load Test / Performance Metrics / Before-After / Reports

### Phase 7
Migration Pre-flight / Compatibility / Migration / Validation / Data Compare / Performance Compare / Rollback / Rollback Verification / Readiness

### Phase 8
AI Compatibility Advisor / Cross-DB Translator / Metrics・Migration・Topology analysis / MCP高度化

## 29. 差別化

個々の機能にはDBクライアント、Migration Tool、Monitoring Tool、Benchmark Tool等の競合が存在する。

dbboardでは、

`Modern Multi-DB Client + Native DB Features + AI/MCP + Automation + Backup/Storage + Migration Rehearsal + Performance Testing + Topology/Replication + Activity Timeline + 100% Free OSS`

を一つのLocal-first Desktop UXで接続することを差別化の中心とする。

## 30. 製品哲学

- 共通化しすぎず、各DBの個性を残す。
- DBコマンドを隠すだけではなく、その意味・関連性を可視化する。
- Backup / Migration / Rollbackは「実行成功」と「検証成功」を分ける。
- AIの推論と計測された事実を区別する。
- 本番前に何度でも失敗できる場所を作る。

## 31. 最終製品像

> DBを理解し、監視し、バックアップし、移行を試し、性能を測り、安全に元へ戻せるLocal-first Database Workspace。

理想フロー:

`Connect → Topology/Health/Lag → Backup → Verify → Migration Pre-flight → Compatibility Scan → Migration Rehearsal → Data Validation → Load Test → Before/After Graph → Problem Detection → Rollback → Rollback Verification → Activity Timeline → Markdown Report`

この一連をGUI・AI・MCPのいずれからも扱えることを長期ゴールとする。
