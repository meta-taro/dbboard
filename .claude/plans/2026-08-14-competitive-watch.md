# dbboard 競合ウォッチ・定期市場チェック 企画草案
## Agent共有用

## 1. 目的

dbboard の競合製品・周辺技術・MCP / AI Agent 市場を定期的に調査し、

- 競合が追加した新機能
- 対応データベース
- MCP / Agent機能
- 安全設計
- Cloud対応
- UI / UX
- Pricing / OSS方針
- GitHubの成長
- 新規競合
- 市場トレンド

を継続的に記録する。

調査結果は Markdown として保存し、Git で差分管理する。

目的は単なる競合監視ではなく、

> dbboard がどこで差別化できるかを継続的に判断するための材料を残す

こと。

---

# 2. 基本コンセプト

```text
Competitor / Market
↓
定期調査
↓
Agent
↓
差分抽出
↓
評価
↓
competitive/YYYY-MM/*.md
↓
Git
↓
Roadmap / Issueへ反映
```

---

# 3. 初期ウォッチ対象

## Direct Competitors

### Tabularis

重点監視対象。

確認項目：

- Built-in MCP
- 対応DB
- Read-only
- Approval
- EXPLAIN / Safety Gate
- Plugin Driver
- Agent integration
- Desktop UX
- Release頻度
- OSS状況

---

### DBeaver

確認項目：

- MCP Server
- dbvr
- AI Chat
- 外部MCP連携
- 対応DB
- Enterprise機能
- Cloud接続
- Agent操作
- Safety / Approval

---

### DataGrip

確認項目：

- MCP Tools
- Agent Skills
- Claude Agent
- Codex
- Database connection management
- Text-to-SQL
- Query approval
- Query history
- JDBC driver対応

---

### DBHub / Bytebase

確認項目：

- MCP Server
- Workbench
- 対応DB
- Read-only
- Row Limit
- Query Timeout
- Multi-DB
- Policy
- Team / Governance
- Web UI

---

### Beekeeper Studio

確認項目：

- AI Shell
- Bring Your Own Agent
- 対応DB
- Redis
- DuckDB
- ClickHouse
- MongoDB
- SQL Server
- Plugin
- Desktop UX

---

# 4. 周辺ウォッチ対象

必要に応じて追加する。

```text
TablePlus
HeidiSQL
pgAdmin
MongoDB Compass
Redis Insight
JetBrains Database Tools
Supabase Studio
Firebase Console
ClickHouse Play / Clients
Elastic / OpenSearch Dashboards
DuckDB ecosystem
```

直接競合ではないが、各DB固有UXの参考になる。

---

# 5. 新規競合の探索

固定リストだけではなく、新しい競合を検索する。

検索テーマ例：

```text
database MCP client
database client AI agent
desktop MCP database
Claude database MCP
Codex database MCP
local database AI agent
open source database MCP
AI database GUI
```

追加候補が見つかった場合：

```text
competitive/competitors/<name>.md
```

を作成する。

---

# 6. 調査頻度

推奨：

```text
Weekly
├─ Release / Changelog
├─ GitHub activity
└─ 重大アップデート

Monthly
├─ 全競合比較
├─ 対応DB比較
├─ MCP比較
├─ Safety比較
└─ Roadmap提案

Quarterly
├─ 市場ポジショニング再評価
├─ 新規競合
├─ AI Agent市場
└─ dbboard戦略見直し
```

---

# 7. Weekly Check

毎週は軽量にする。

確認：

- 新Release
- Changelog
- GitHub Releases
- README変更
- MCP追加
- 新DB対応
- Safety変更
- Agent integration
- Pricing変更
- OSS License変更

成果物：

```text
competitive/weekly/
└─ 2026-W33.md
```

---

# 8. Monthly Report

月次では競合を横断比較する。

成果物：

```text
competitive/monthly/
└─ 2026-08.md
```

フォーマット：

```markdown
# Competitive Report 2026-08

## Executive Summary

## Major Changes

## Competitor Updates

### Tabularis
### DBeaver
### DataGrip
### DBHub
### Beekeeper Studio

## Database Support Matrix

## MCP / Agent Matrix

## Safety Matrix

## Cloud Matrix

## dbboardとの差分

## Threats

## Opportunities

## Recommended Actions

## Proposed Issues
```

---

# 9. 対応DB比較

例：

| Database | dbboard | Tabularis | DBeaver | DataGrip | DBHub | Beekeeper |
|---|---:|---:|---:|---:|---:|---:|
| MySQL | ✓ |  |  |  |  |  |
| PostgreSQL | ✓ |  |  |  |  |  |
| SQLite | ✓ |  |  |  |  |  |
| MongoDB | ✓ |  |  |  |  |  |
| Firestore | ✓ |  |  |  |  |  |
| DuckDB | Planned |  |  |  |  |  |
| Redis / Valkey | Planned |  |  |  |  |  |
| SQL Server | Planned |  |  |  |  |  |
| ClickHouse | Planned |  |  |  |  |  |
| OpenSearch | Planned |  |  |  |  |  |

Agentが最新情報で更新する。

---

# 10. MCP / Agent比較

比較項目：

```text
Built-in MCP
External MCP
Claude Code
Claude Agent
Codex
Cursor
Windsurf
Gemini CLI
Agent Skills
Connection reuse
Schema inspection
Query execution
Write operation
Approval
Read-only
Audit log
```

---

# 11. Safety比較

dbboardにとって重要。

比較する：

- Read-only
- Query allow / deny
- Destructive query detection
- Approval
- Row limit
- Timeout
- Backup before write
- Write Policy
- Audit Log
- Secret storage
- Local-only
- Credential handling
- SSH Tunnel
- TLS

単に機能数ではなく、

> AI AgentにDBを触らせる際の安全性

を独立評価する。

---

# 12. Cloud比較

確認対象：

```text
AWS
├─ RDS
├─ Aurora
├─ ElastiCache
├─ MemoryDB
└─ OpenSearch

Azure
├─ Azure SQL
├─ PostgreSQL
└─ Redis系

GCP
├─ Cloud SQL
├─ Firestore
└─ AlloyDB
```

比較する：

- Direct connection
- TLS
- IAM/Auth
- SSH / Bastion
- VPN
- Cloud preset
- Managed DB固有機能

---

# 13. GitHub指標

OSSの場合はGitHubも観測する。

記録候補：

```text
Stars
Forks
Issues
PR
Contributors
Release cadence
Last commit
Open issues
Discussions
```

ただしStar数だけで優劣を判断しない。

見るべきもの：

- 成長速度
- Release継続性
- Contributor増加
- 新Featureの方向
- Issue内容
- User request

---

# 14. 差分評価

各競合について毎回全文を書き直さない。

前回との差分を中心にする。

例：

```text
Tabularis

Previous:
PostgreSQL / MySQL / SQLite

Current:
+ DuckDB plugin
+ Agent approval improvements

Impact:
Medium

dbboard:
DuckDB implementationを前倒し検討
```

---

# 15. Threat評価

評価レベル：

```text
Low
Medium
High
Critical
```

例：

```text
Threat: High

競合が
- MongoDB
- Firestore
- Redis
- MCP
- Read-only
- Desktop
を同時に実装した
```

「似た機能がある」だけでThreatを高くしない。

dbboardの差別化軸を直接侵食するかで判断する。

---

# 16. Opportunity評価

競合が対応していない領域を見つける。

候補：

```text
Firestore
Multi-model DB
Redis / Valkey
DuckDB / local files
Cross-DB Agent interface
Local-first
Unified MCP
Write Policy
Backup / Restore
SSH Tunnel
Cloud presets
Agent investigation
Database + Log investigation
```

---

# 17. dbboardの差別化軸

定期的に評価する。

現時点の候補：

```text
1. Multi-database

2. SQL + NoSQL + Cache + Analytics

3. Human GUI + AI Agent

4. Built-in MCP

5. Local-first

6. Read-only / Write Policy

7. Backup / Restore

8. SSH / Cloud connectivity

9. Agentが同じConnectionを利用

10. Open Source
```

---

# 18. 市場ポジショニング

競合チェック時にdbboardの説明文も見直す。

候補：

```text
Database client with MCP
```

↓

```text
Local-first database client for humans and AI agents
```

さらに対応データソースが増えた場合：

```text
Universal local data workspace for AI agents
```

必要に応じてポジションを更新する。

---

# 19. Roadmap連携

調査結果から直接Issue候補を作る。

例：

```text
Competitive Finding:
Tabularis introduced X

Assessment:
Useful / High demand

dbboard Gap:
Missing

Proposal:
Implement X

Priority:
P2
```

---

# 20. Issue化ルール

競合が実装したから即コピーしない。

Issue化条件：

```text
○ dbboardユーザーに価値がある
○ dbboardの思想に合う
○ Agent用途が明確
○ Safetyを維持できる
○ 実装負債が許容範囲
```

以下だけならIssue化しない：

```text
× 競合がやったから
× Starが多いから
× 流行しているから
```

---

# 21. Agent Prompt例

```text
dbboardの競合を定期調査してください。

対象：
- Tabularis
- DBeaver
- DataGrip
- DBHub / Bytebase
- Beekeeper Studio

確認：
- Release / Changelog
- GitHub
- MCP
- AI Agent
- 対応DB
- Safety
- Cloud
- Pricing / License

前回のcompetitive reportとの差分だけを重点的に確認してください。

新しい競合があれば追加してください。

最後に、
- Threat
- Opportunity
- dbboardとの差分
- 推奨Issue
をまとめてください。

結果はMarkdownで保存してください。
```

---

# 22. 保存構造

```text
competitive/
├─ README.md
│
├─ competitors/
│  ├─ tabularis.md
│  ├─ dbeaver.md
│  ├─ datagrip.md
│  ├─ dbhub.md
│  └─ beekeeper-studio.md
│
├─ weekly/
│  └─ 2026-W33.md
│
├─ monthly/
│  └─ 2026-08.md
│
└─ quarterly/
   └─ 2026-Q3.md
```

---

# 23. competitor master

各競合の基本情報は個別ファイルにする。

例：

```yaml
---
name: Tabularis
category: database-client
mcp: true
agent: true
oss: true
watch: high
last_checked: 2026-08-14
---
```

本文：

```text
Position
Features
Supported Databases
MCP
Safety
Cloud
Pricing
Links
Notes
```

---

# 24. Gitによる履歴

競合情報そのものもGit管理する。

これにより、

```text
2026-08
Tabularis supports A/B/C

2026-10
+ Redis

2027-01
+ MongoDB
```

のように市場変化を追える。

AI AgentはGit diffを使って、

> 前回から何が変わったか

を優先的に調査する。

---

# 25. 自動化案

将来的には定期実行できるようにする。

```text
Scheduler
↓
Competitive Watch Agent
↓
Web / GitHub / Release
↓
Previous Report
↓
Diff
↓
Markdown
↓
Git
```

重要：

毎回全Webを再調査するのではなく、

```text
Previous State
+
Latest Release / Changelog / GitHub
↓
Difference
```

を基本とする。

---

# 26. 通知条件

毎回通知する必要はない。

通知対象：

```text
- 競合が新DB対応
- MCP大幅変更
- Agent機能追加
- Safety機能追加
- OSS→商用 / License変更
- 急速なGitHub成長
- 新規直接競合
- dbboardの主要差別化を競合が実装
```

軽微な変更は月次レポートのみ。

---

# 27. 認知拡大にも利用

競合調査は製品開発だけでなく記事ネタにも使う。

例：

```text
Database clients with MCP in 2026
```

```text
How database tools are adapting to AI agents
```

```text
MCP database clients compared
```

dbboardだけを宣伝するのではなく、市場全体を比較した記事を作る。

その中でdbboardを自然に紹介する。

---

# 28. 実装優先度

## Phase 1

- [ ] `competitive/` 作成
- [ ] 主要5競合master作成
- [ ] Monthly Report template
- [ ] Agent Prompt
- [ ] Git diff運用

## Phase 2

- [ ] Weekly lightweight check
- [ ] GitHub metrics
- [ ] New competitor discovery
- [ ] Threat / Opportunity score

## Phase 3

- [ ] Scheduler
- [ ] 自動Report
- [ ] 条件通知
- [ ] Roadmap / Issue候補生成

---

# 29. 最終目的

競合をコピーすることではない。

> 市場がどこへ向かっているかを継続的に把握し、
> dbboard独自の強みを失わないようにRoadmapを調整する。

競合チェックを一度きりの調査ではなく、

```text
Observe
↓
Diff
↓
Evaluate
↓
Decide
↓
Issue
↓
Implement / Ignore
```

という継続的な製品開発プロセスとして扱う。
