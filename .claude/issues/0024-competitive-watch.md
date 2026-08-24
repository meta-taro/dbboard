# 0024 — 競合ウォッチ・市場チェックの仕組み

- 状態: 未着手（企画確定。着手は v1.0 の後）
- 出典: 共有された企画草案「dbboard 競合ウォッチ・定期市場チェック」。原文は
  `.claude/plans/2026-08-14-competitive-watch.md` に全文のまま置いてある。
  下は要約なので、判断に迷ったら原文を見る。
- 関連: baseline §14（時間ベースの報告ノルマを設けない）、baseline §31（棚卸しは
  時間トリガにしない）、0023（追加アダプター）

## 目的

競合を真似るためではない。

> 市場がどこへ向かっているかを継続的に把握し、dbboard 独自の強みを失わないよう
> ロードマップを調整する。

## 草案からの変更点 2 つ

企画草案をそのまま採らず、次の 2 点を変えて起こす。

### 変更 1 — 週次 / 月次 / 四半期をやめ、差分起点にする

草案は Weekly / Monthly / Quarterly の定期調査だった。これは baseline と衝突する。

- baseline §14: 「定例・日次メール・週次報告のような**時間ベースの報告ノルマは
  設けない**」
- baseline §31: 時間トリガにすると「やることがない週にも形式的な更新を強い、
  質の悪い記録を増やす」

**カレンダーで起動せず、差分で起動する。**

- 競合の release / changelog / README に**変化が観測されたときだけ**書く
- 変化が無ければ 1 行も増えない。空の週が続くこと自体は異常ではない
- これは草案 §25 の「毎回全 Web を再調査せず、前回状態との差分を基本とする」
  という趣旨と一致する。起動条件を趣旨に合わせただけ

四半期のポジショニング再評価だけは、差分では起動しない性質のもの。これは
**リリースを切るタイミング**（v1.0、v1.1 …）に紐付ける。

### 変更 2 — 対抗的なラベルを使わない

dbboard は public リポジトリ。`competitive/` をリポに置くなら、**書いた内容は
競合からも読める**。

`Threat: High` のような対抗的なラベルは使わない。書くのは次の 2 つだけ。

- **観測した事実**（いつ、どの製品が、何を出したか。出典 URL 付き）
- **dbboard 側の判断**（追う / 追わない / 別解を採る、とその理由）

公開されても困らない書き方にすれば、置き場所を悩む必要がなくなる。評価の中身は
落とさない — 落とすのは相手を敵として名指す枠組みだけ。

## ウォッチ対象

### 直接競合

Tabularis / DBeaver / DataGrip / DBHub（Bytebase）/ Beekeeper Studio

確認するもの: 対応 DB、MCP の有無と形、AI エージェント連携、読み取り専用と承認、
クラウド接続、価格と OSS 方針、リリース頻度

### 周辺（各 DB 固有の UX 参考）

TablePlus / HeidiSQL / pgAdmin / MongoDB Compass / Redis Insight /
Supabase Studio / Firebase Console / OpenSearch Dashboards / DuckDB 周辺

### 新規競合の探索

固定リストだけを見ない。次のようなテーマで検索し、見つかったら
`competitive/competitors/<name>.md` を起こす。

```
database MCP client / database client AI agent / desktop MCP database
local database AI agent / open source database MCP / AI database GUI
```

## 保存構造

```
competitive/
├─ README.md                    運用の説明（差分起点であること）
├─ competitors/<name>.md         競合ごとの現状（frontmatter に last_checked）
└─ reports/YYYY-MM-<slug>.md     変化を観測したときだけ作る
```

草案の `weekly/` `monthly/` `quarterly/` は作らない（変更 1 のとおり）。

競合ファイルの frontmatter 例:

```yaml
---
name: Tabularis
category: database-client
mcp: true
agent: true
oss: true
last_checked: 2026-08-16
---
```

Git 管理する。**差分そのものが市場の変化の記録になる**ため、毎回全文を書き直さず
変わった箇所だけ直す。

## 比較の軸

- **対応 DB** — 0023 の候補（DuckDB / SQL Server / Redis / ClickHouse /
  OpenSearch）が競合でどう扱われているかは、優先度を見直す材料になる
- **MCP / エージェント** — 内蔵か外部か、接続を人間と共有するか、承認の有無
- **安全性** — 読み取り専用、破壊的クエリの検出、行数上限、タイムアウト、
  監査ログ、資格情報の扱い。**dbboard の主軸なのでここは独立して見る**
- **クラウド** — AWS / Azure / GCP の各マネージド DB への接続方式

## Issue 化の条件

競合が実装したことは、それだけでは着手理由にならない。次を全部満たすときだけ
Issue を起こす。

- dbboard の利用者に価値がある
- dbboard の思想に合う
- エージェントからの用途が明確
- 安全性を維持できる
- 実装負債が許容範囲

「競合がやったから」「Star が多いから」「流行しているから」だけでは起こさない。

## 段階

```
段階 1  competitive/ を作る。主要 5 競合の現状ファイルを 1 回書く。
        報告のテンプレートと、差分起点であることを README に書く
段階 2  新規競合の探索。GitHub 指標の記録
段階 3  自動化（変化の検知 → 報告の下書き → Issue 候補）
```

**着手は v1.0 の後**。v1.0 の完了条件（issue 0021）には含めない。

## 完了条件（段階 1）

- `competitive/README.md` に、差分起点であること（カレンダーで動かないこと）が
  書かれている
- 主要 5 競合のファイルが `last_checked` 付きで存在する
- 対抗的なラベルが 1 つも使われていない
- 最初の報告が 1 本ある（変化を観測した場合のみ。無ければ報告は 0 本でよい）
