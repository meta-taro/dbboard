---
title: "Claude Code に自分の DB を触らせる — dbboard の MCP サーバー"
emoji: "🗄️"
type: "tech"
topics: ["mcp", "claudecode", "database", "rust", "mysql"]
published: true
---

<!--
Published at https://zenn.dev/dokokade/articles/46b8c608715963
Zenn builds from a separate repository, where this file carries the slug
as its name. This copy is the source of record: edit here first, then
copy across, so the article and the docs it describes move together.
-->

Claude Code に「このテーブルの構造どうなってる？」と聞くために、スキーマを
コピペして貼り付けている人向けの記事です。

[dbboard](https://github.com/meta-taro/dbboard) は Rust + Tauri 製のデスク
トップ DB クライアントで、v0.5 から **MCP サーバー**（`dbboard-mcp`）が同梱
されています。これを Claude Code に登録すると、エージェントが自分でテーブル
一覧を引き、スキーマを読み、`SELECT` を投げられるようになります。

この記事は「作りました」の紹介ではなく、**その場で導入して動かすまでの手順**
です。以下に出てくるコマンドと出力は、v0.5.1 のバイナリを実際に登録して叩いた
結果をそのまま貼っています。

## 何が嬉しいか、先に結論

- スキーマのコピペが要らなくなる。`describe_table` と `search_schema` を
  エージェントが自分で呼ぶ。
- **読み取りは DB エンジン側で read-only が強制される**。文字列の前方一致で
  `SELECT` かどうかを見ている、みたいな実装ではない（後述）。
- **書き込みは既定でオフ**。接続ごとに人間が opt-in して初めて開く。しかも
  `GRANT` / `TRUNCATE` / `DROP` はどう設定しても開かない。
- **接続情報はエージェントに一切渡らない**。URL もトークンも OS のキーチェーン
  に置かれたままで、MCP のレスポンスには id / 名前 / 種別しか乗らない。

## 対応している DB

MCP サーバーはデスクトップアプリと同じアダプタ層を使うので、アプリで繋がる
ものはそのままエージェントからも触れます。

| 種別 | 対象 |
|---|---|
| `turso` | Turso / libSQL（ローカルの SQLite ファイルを含む） |
| `d1` | Cloudflare D1（REST API 経由） |
| `postgres` | CockroachDB / セルフホスト PostgreSQL |
| `neon` | Neon |
| `supabase` | Supabase（Postgres wire。PostgREST 等は対象外） |
| `aurora-dsql` | Amazon Aurora DSQL（IAM トークン自動発行の派生あり） |
| `mysql` | MySQL 8.x / MariaDB |

Postgres 系は 4 種類とも同じアダプタですが、接続の「種別」としては別扱いに
なっています（ADR-0018 / 0019 / 0021）。MySQL だけは方言が違うので別アダプタ
です（ADR-0068）。バージョンごとのサポート状況は
[`docs/compatibility.md`](https://github.com/meta-taro/dbboard/blob/develop/docs/compatibility.md)
にあります。

## 導入

### 1. バイナリを取る

[latest release](https://github.com/meta-taro/dbboard/releases/latest) から
自分の OS のものを 1 つ落とします。ランタイム依存のない単体実行ファイルです。

| OS | 資産名 |
|---|---|
| Windows x64 | `dbboard-mcp-windows-x86_64.exe` |
| macOS (universal) | `dbboard-mcp-macos-universal` |

**デスクトップアプリのインストーラには入っていません。** 別ダウンロードです
（逆に、MCP サーバーだけ使うならアプリは要りません）。

### 2. 未署名なので、初回だけ OS に怒られる

コード署名をしていないので、そのままだと止められます。

- **Windows**: SmartScreen が「WindowsによってPCが保護されました」を出します。
  *詳細情報* → *実行* で通します。
- **macOS**: Gatekeeper に弾かれます。notarize もしていないので、quarantine
  属性を外します。

```sh
xattr -d com.apple.quarantine ~/.local/bin/dbboard-mcp
```

ウイルス対策ソフトが未署名バイナリを一般論として警告することもあります。気持ち
悪ければ、リリースの `SHA256SUMS.txt` とハッシュを突き合わせてください。

### 3. 置き場所を決める（`target/` から直接登録しない）

自分でビルドした場合、**`target/release/` の中を直接登録しないでください**。
Windows は実行中の exe を置き換えられないので、次に `cargo build --release`
したときに

```
failed to remove file ... (os error 5)
```

でビルドが落ちます。どこかにコピーしてから登録します。

### 4. Claude Code に登録する

```sh
# macOS / Linux
claude mcp add dbboard --scope user -- "$HOME/.local/bin/dbboard-mcp"
```

```powershell
# Windows (PowerShell) — パス区切りは / か、\\ にする
claude mcp add dbboard --scope user -- "$env:LOCALAPPDATA/dbboard/dbboard-mcp.exe"
```

確認します。

```sh
$ claude mcp list
Checking MCP server health…

dbboard: C:/.../dbboard-mcp.exe - ✔ Connected
```

`✔ Connected` が出れば、9 個のツールが Claude Code から見えています。

:::message
設定を変えたら **クライアントを再起動**します。MCP サーバーはクライアントごと
に 1 プロセス起動され、セッション中は握られたままなので、`connections.toml` を
書き換えても起動済みのプロセスには届きません。
:::

### 5. 接続を用意する

MCP サーバーは接続を**作れません**。読むだけです。作るのは人間の仕事で、
デスクトップアプリの接続フォームか、`connections.toml` を直接書きます。

| OS | 既定のパス |
|---|---|
| Windows | `%APPDATA%\dbboard\dbboard\config\connections.toml` |
| macOS | `~/Library/Application Support/dev.dbboard.dbboard/connections.toml` |
| Linux | `$XDG_CONFIG_HOME/dbboard/connections.toml` |

ローカルの SQLite ファイルなら、これだけで足ります。

```toml
[[connections]]
id = "scratch"
name = "scratch"
kind = "turso"
path = "C:/work/scratch.db"
```

パスワードや接続 URL を伴う種別は、値そのものを TOML に書かず、OS キーチェーン
への参照（`keyring_url_ref` など）を書きます。アプリのフォームから作れば自動で
そうなります。

別のファイルを使わせたいときは `--config` か `DBBOARD_CONFIG` で指定します。

```jsonc
{
  "mcpServers": {
    "dbboard": {
      "type": "stdio",
      "command": "C:/Users/<you>/AppData/Local/dbboard/dbboard-mcp.exe",
      "env": { "DBBOARD_CONFIG": "C:/work/agent-connections.toml" }
    }
  }
}
```

:::message alert
`DBBOARD_MYSQL_URL` のような「接続そのものを環境変数で渡す」変数は
`dbboard-server`（ヘッドレス HTTP サーバー）側の仕組みで、**MCP サーバーは
読みません**。MCP のツール呼び出しは `connection_id` を名前で解決するので、
`connections.toml` に実体が要ります。（このドキュメントの誤りは、この記事を
書く過程で見つけて直しました。）
:::

## 使えるツール（9 個）

読み取り 7 個、書き込み 1 個、バックアップ 1 個です。

| ツール | 返すもの |
|---|---|
| `list_connections` | 設定済み接続を `{ id, name, kind }` で列挙 |
| `list_tables` | テーブル一覧 |
| `describe_table` | 1 テーブルの列（型・NULL 可否・PK・順序）と主キー |
| `search_schema` | 名前に部分一致するテーブル / 列を横断検索（最大 200 件） |
| `list_relationships` | 外部キーを有向辺として返す（最大 500 件） |
| `run_read_query` | read-only な SQL 1 文の結果（既定 200 行 / 上限 1000 行） |
| `get_annotations` | dbboard 側で付けたテーブル・列のメモ |
| `run_write` | 書き込み 1 文。接続ごとの opt-in が必要 |
| `dump_database` | 論理ダンプをファイルに書き出す |

`search_schema` は「メールアドレスってどのテーブルにある？」を、全テーブルに
`describe_table` を撃たずに 1 回で終わらせるためのものです。行データではなく
**識別子**を検索します。

実際に投げた結果はこうなります（`run_read_query`）。

```json
{
  "columns": [
    { "name": "id", "declared_type": "INTEGER" },
    { "name": "email", "declared_type": "TEXT" }
  ],
  "rows": [
    [1, "a@example.com"],
    [2, "b@example.com"]
  ],
  "row_count": 2,
  "truncated": false
}
```

`max_rows` を超えた分は切り捨てられ、`truncated: true` が立ちます。上限は
1000 でクランプされます。読み取りは偵察用であって一括エクスポート用ではない、
という線引きです。

## read-only は「文字列判定」ではない

ここが一番の勘所です。`run_read_query` は SQL の先頭が `SELECT` かどうかを見て
判断していません。**DB エンジン側の read-only モードの中で実行**します。

| エンジン | 強制のしかた |
|---|---|
| Postgres 系（Neon / Supabase / CockroachDB / Aurora DSQL） | `SET TRANSACTION READ ONLY` のトランザクション内で実行（あわせて `statement_timeout` も設定） |
| MySQL / MariaDB | `SET TRANSACTION READ ONLY`（次のトランザクションのみに効く指定。SESSION / GLOBAL を汚さない） |
| libSQL / Turso | `PRAGMA query_only` を立てて実行し、必ず戻す |
| Cloudflare D1 | AST で分類（サーバ側に read-only モードが無いため、ここだけは「分類」であることが ADR-0046 に明記されている） |

なぜ前方一致ではダメなのか。Postgres アダプタは複数文をまとめて実行できる経路
を使っているので、`SELECT 1; DROP TABLE t;` は構文エラーにならず**両方走り
ます**。さらに、

- `WITH x AS (DELETE FROM t RETURNING *) SELECT * FROM x` は `WITH` で始まる
- `SELECT ... FOR UPDATE` は行ロックを取る
- `SELECT nextval('s')` はシーケンスを進める
- `EXPLAIN ANALYZE <DML>` は本当に実行する

いずれも「`SELECT` で始まっていれば安全」を破ります。

実際に投げると、こう返ります。

```
DELETE FROM users WHERE id = 1
→ query failed: not a single read-only statement:
  only read-only SELECT / WITH / EXPLAIN statements are allowed

SELECT 1; DROP TABLE users
→ query failed: not a single read-only statement:
  expected a single statement, found 2
```

## 書き込みは 3 段階のゲート

「読めるだけだと結局使えない」という話は当然あるので、v0.5 で `run_write` が
入りました。ただし全開ではなく、3 段とも通らないと実行されません（ADR-0087）。

### 1 段目: 接続ごとの opt-in

`connections.toml` の `mcp_write = true`、またはアプリの *Connections → Edit →
AI agent access* のトグル。**キーが無ければ `false`** なので、既存の接続は
アップグレードしても読み取り専用のままです。

opt-in していない接続に書こうとすると、こうなります。

```
connection "scratch-ro" is not enabled for writes over MCP;
a human must set mcp_write = true on it in connections.toml
```

「設定を変えれば通る」ことが、エージェントに分かる文面になっています。

### 2 段目: AST による allowlist

SQL をパースして、以下だけを受け付けます。

- **データ**: `INSERT` / `UPDATE` / `DELETE` / `MERGE`
- **スキーマ**: `CREATE TABLE` / `CREATE VIEW` / `CREATE INDEX` /
  `CREATE SCHEMA` / `ALTER TABLE`

それ以外は、危険かどうかに関係なく**リストに無いので拒否**されます
（`COMMENT ON` すら通りません）。分類できないものも拒否。fail closed です。

通ったときの返りはこれだけです。

```json
{ "statement": "schema", "rows_affected": 0 }   // CREATE TABLE
{ "statement": "data",   "rows_affected": 2 }   // INSERT ... 2 行
```

### 3 段目: どう設定しても開かないもの

`mcp_write = true` にしても、**永久に**通らないものがあります。

- `GRANT` / `REVOKE` / `DENY`
- ユーザー / ロールの DDL、`SET PASSWORD`
- `TRUNCATE`
- `DROP`（**インデックスを含めて全部**。`CREATE INDEX` は通るが `DROP INDEX`
  は通らない）

拒否メッセージは「一時的な拒否」と区別できる文面になっていて、設定を探しに
行っても無駄だと分かります。

```
DROP TABLE users
→ refused permanently: DROP — it destroys an object rather than its contents
  — dbboard never runs this through an agent; use the desktop app's SQL editor

GRANT ALL ON users TO app
→ refused permanently: a privilege change (GRANT / REVOKE / DENY) — ...

TRUNCATE TABLE users
→ refused permanently: TRUNCATE — it cannot be rolled back the way a DELETE can
  — ...
```

`DELETE` は通るのに `TRUNCATE` は通らないのは、言葉の印象の問題ではありません。
`DELETE` は `WHERE` があって行単位でログに残りトランザクションで巻き戻せる。
`TRUNCATE` / `DROP` は DDL で、MySQL では暗黙コミットされ、戻すものが残らない。
`GRANT` を閉じているのはもう少し直接的で、**権限を配れるエージェントは、渡され
た接続の範囲を自分で広げられてしまう**ので、1 段目の opt-in が無意味になります。

なお `dump_database` は書き込みゲートの**外**にあります。バックアップを取る
行為は DB を変更しないからで、`run_write` の前に呼べるようになっています
（既存ファイルは上書きせず、`create_new` で作ります）。

接続の追加・編集・削除も MCP からはできません。認証情報を扱うことになるためで、
リストア機能も同じ理由で閉じています。

## 秘密情報はレスポンスに乗らない

`list_connections` が返すのは `{ id, name, kind }` だけです。解決済みの接続
URL、トークン、キーチェーンの参照キーは、ツールの結果にもエラーメッセージにも
入りません。エージェントが見るのは「`shop-db` という名前の `mysql` 接続がある」
までで、それがどこの何かは分かりません。

接続名そのものを見せたくない場合は `mcp_alias` を設定すると、エージェント側には
別名だけが見え、本来の id はハンドルとして受け付けられなくなります（ADR-0088）。

## ハマりどころ

- **設定変更後に再起動していない。** 前述のとおり、プロセスはセッション中
  握られたままです。
- **`expected to read 4 bytes, got 0 bytes at EOF`。** 接続が死んでいます。
  v0.5.1 で直しました。SSH バスティオン経由のトンネルが keepalive を送っておらず
  （russh は既定で送らない）、アイドルのセッションが向こう側で回収されていました。
  いまはキャッシュ済み接続が 30 秒アイドルだったら使う前に ping し、失敗したら
  捨てて張り直します。アプリ側には再接続ボタンも付いています。
- **MySQL 8 で `describe_table` / `search_schema` が必ず失敗する。** これも
  v0.5.1 で修正。8.0 以降 `information_schema` がデータディクショナリから
  提供され、`TABLE_NAME` が `VARBINARY`、`DATA_TYPE` が `BLOB` と宣言されるため
  型チェックに弾かれていました。**v0.5.0 以前を MySQL 8 で使っている人は上げて
  ください。** `list_relationships` はエラーを飲み込む実装だったので、失敗を
  「関連なし」として静かに返していました。
- **ログが JSON-RPC を壊さないか。** stdout は JSON-RPC 専用で、ログは全部
  stderr に出ます（`RUST_LOG`、既定 `info`）。デバッグしたいときは
  `RUST_LOG=debug` を `env` に足してください。

## いま作っているもの

非 SQL 系のアダプタに着手しています。

- **Cloud Firestore**: アダプタが入り、デスクトップアプリからも MCP からも
  使えるところまで来ています。クエリ文字列は Google のドキュメントに出てくる
  `StructuredQuery` の JSON をそのまま渡す形で、翻訳レイヤは挟んでいません。
  コレクションはスキーマを宣言しないので、`describe_table` はサンプリングして
  `string (12/20 sampled)` のように**推測であることを型名に書いて**返します。
- **MongoDB**: アダプタは動いていて（クエリは `{"find": "users", "limit": 10}`
  のようなコマンドドキュメント）、デスクトップアプリと MCP への配線が残りです。
  Firestore と違って読み取り専用を「構造」で保証できない — 全コマンドが同じ
  `runCommand` を通る — ので、ここだけは分類器が安全性の要になっています。

どちらもまだリリースには乗っていません。乗ったら `docs/compatibility.md` に
行が増えます。

---

dbboard 本体は OSS（MIT）です。バイナリは
[リリースページ](https://github.com/meta-taro/dbboard/releases/latest)、
デスクトップアプリは[ダウンロードページ](https://meta-taro.github.io/dbboard/)
から取れます。
