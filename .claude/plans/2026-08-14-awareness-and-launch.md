# dbboard 認知拡大施策
## エージェント共有用実行草案

## 1. 目的

dbboard の海外を含む認知度を上げる。

中心メッセージ：

> Local database client with a built-in MCP server for AI agents.

単なるデータベースGUIではなく、ローカルDB・デスクトップ・MCP・Claude Code等のAI Agent・Read-only運用・OSSを組み合わせた開発者向けツールとして認知を取る。

---

## 2. 基本方針

広告的な言い方は避ける。

悪い例：

```text
The ultimate next-generation AI database tool!
```

推奨：

```text
I wanted Claude Code to inspect my local database
without putting the database behind a cloud proxy,
so I built a desktop DB client with an MCP server built in.
```

「何を作ったか」より、なぜ作ったか・何が不便だったか・なぜローカルなのか・なぜMCPなのかを中心に書く。

---

## 3. 優先施策

### Priority A: Show HN

タイトル案：

```text
Show HN: dbboard – A local database client with an MCP server for AI agents
```

本文に含める内容：

- Claude Code等からローカルDBを調査できる
- MCP Serverを内蔵
- クラウドDB Proxyを不要にした
- Read-only運用が可能
- OSS
- GitHub URL
- 対応DB
- 30秒程度のデモGIF

避ける：

- マーケティング用語
- 絵文字多用
- 過剰な優位性主張
- 長すぎる製品紹介

### Priority A: Reddit / r/ClaudeAI

タイトル案：

```text
I built an open-source local DB client that lets Claude query databases through MCP
```

本文構成：

```text
Why
↓
What I built
↓
How MCP is used
↓
Why local
↓
Read-only / safety
↓
GitHub
```

訴求ポイント：

- Local-first
- MCP built in
- AI Agent can inspect/query DB
- No need to expose DB through a new cloud service
- OSS

### Priority A: Reddit / r/opensource

OSSとして紹介する。

内容：

- 問題意識
- アーキテクチャ
- ライセンス
- 対応OS
- 対応DB
- GitHub
- contributors welcome

「宣伝」より「こんなものを作った」という技術共有として投稿する。

---

## 4. MCP Directory 登録

現在：

```text
MCP Market
```

への掲載確認済み。

追加候補：

```text
mcpservers.org
awesome-mcp-servers
best-of-mcp-servers
その他 MCP Directory
```

作業：

- [ ] 各Directoryの登録条件確認
- [ ] README説明を短く整理
- [ ] MCP Tool一覧を明記
- [ ] Installation手順を明記
- [ ] GitHub Topics追加
- [ ] Demo GIF追加
- [ ] Directory登録PR / Form送信

---

## 5. GitHub README改善

README冒頭で3秒以内に理解できる状態を作る。

```text
dbboard

Local database client with MCP for AI agents.

[GIF]

- Local-first
- MCP built in
- Read-only mode
- MySQL / PostgreSQL / SQLite / ...
- Open source

Quick Start
...
```

READMEに欲しいもの：

- [ ] 30秒GIF
- [ ] Architecture図
- [ ] MCP Tool例
- [ ] Claude Codeでの利用例
- [ ] Read-only説明
- [ ] 対応DB
- [ ] 対応OS
- [ ] Install
- [ ] Security / Privacy
- [ ] License

---

## 6. Demo GIF

30秒程度。

```text
Claude Code
↓
dbboard MCP
↓
Database
↓
Query / Schema確認
↓
Claudeへ結果返却
```

見せたいポイント：

1. dbboard起動
2. DB接続
3. Claude Codeから質問
4. MCP Tool実行
5. DB結果取得
6. Claudeが回答

README、Reddit、HN、DEV.toなどで共通利用する。

---

## 7. GitHub Topics

```text
mcp
model-context-protocol
claude
claude-code
ai-agent
database
database-client
sqlite
postgresql
mysql
developer-tools
local-first
```

---

## 8. 英語記事

DEV.to 等へ技術記事を出す。

タイトル案：

```text
Giving Claude Code Safe Access to a Local Database with MCP
```

または：

```text
Why I Built a Local Database Client with an MCP Server
```

記事構成：

1. 問題
2. Cloud Proxyを作りたくなかった理由
3. Local-first設計
4. MCP構成
5. Read-only
6. Claude Code利用例
7. OSS / GitHub

---

## 9. Product Hunt

優先度は Reddit / HN / MCP Directory より低い。

以下が整ってから実施：

- README
- 英語説明
- Demo GIF
- アイコン
- スクリーンショット
- Landing Page

---

## 10. 投稿順序

```text
1. README改善
2. Demo GIF
3. GitHub Topics
4. MCP Directory
5. r/ClaudeAI
6. r/opensource
7. Show HN
8. DEV.to
9. Product Hunt
```

Reddit/HN投稿前にREADMEとGIFを整える。

---

## 11. 投稿後

確認項目：

- GitHub Stars
- GitHub Issues
- Fork
- External mentions
- Directory掲載
- Search engine indexing
- README流入
- Release downloads

反応があった質問はREADME FAQへ反映する。

---

## 12. 継続施策

新機能ごとに記事を作る。

例：

```text
Claude CodeからSchemaを確認する
Read-only MCP
複数DB対応
MCP Tool追加
DB Schema visualization
Query safety
```

「dbboardを更新しました」ではなく、

```text
How to inspect PostgreSQL schema from Claude Code
```

のように利用者の課題をタイトルにする。

---

## 13. KPI

```text
GitHub Stars
Unique visitors
Clone
Release download
Directory掲載数
External mention数
Reddit/HNコメント数
Issue / PR
```

短期のバズより、第三者サイトへの掲載と継続的な自然流入を重視する。

---

## 14. 企画の核

dbboard の認知拡大では、

> MCP対応DBクライアントを宣伝する

のではなく、

> AI Agent にローカルDBを安全に扱わせる方法を提示する

ことを中心にする。

このテーマで GitHub / MCP Directory / Reddit / Hacker News / DEV.to へ横展開する。
