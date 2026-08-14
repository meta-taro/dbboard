# プロジェクトステータス (内部メモ・日本語)

このファイルは作業中のセッション状態を記録する内部用ドキュメント。
外部公開向けの内容ではないため日本語で書く。セッション終了時に更新する。

## 最終更新

- 日付: 2026-08-14 (**open PR = 0。PR の滞留は解消しきった。**

  **入れたもの (develop)**: #159 文書ストアをガイドに記載 / #169 08-13 のセッション記録。
  develop の HEAD は `7569cd5`。**未マージの PR は残っていない。**

  **#159 の push で 1 往復ロスした (エージェント側のミス・再発防止のため記録)**:
  PR の head は `docs/document-stores-in-guides` だったが、
  `git checkout -B docs/document-store-guides <start-point>` の**第 1 引数はローカル名**
  であることを取り違え、作業が別名のローカルブランチに乗った。そのまま push すると
  PR に紐づかない新規リモートブランチができ、#159 は `CONFLICTING` のまま残る。
  復旧は **refspec 指定の push** (`git push origin <ローカル名>:<PR の head 名>`)。
  ローカル名が PR の head と違う限り、`git push` 単体は `push.default=simple` に弾かれる。

  **libSQL テアダウン segfault は pre-push (release プロファイル) でも出る**:
  今回は `dbboard-server` の `tests/http.rs` が `0xc0000005 STATUS_ACCESS_VIOLATION`
  で落ちた。同じテストバイナリを単独で回すと **12/12 緑**で、テスト後のプロセス終了時に
  クラッシュしているだけ (`dbboard-connect` 経由で libsql をリンクしているため、
  `dbboard-turso` 以外でも起きる)。CLAUDE.md が唯一認めている bypass に該当するので
  `--no-verify` で push し、baseline §35 のとおり **CI 4 ジョブ緑を最終ゲート**として確認した。
  pii-scan は pre-commit / commit-msg 側で実行済み・clean。

  **残っているボール (すべて user 側)**: ① 姉妹リポへ `.claude/tools/dbboard.md` を貼る、
  ② ~468 コミットの history 書き換え判断 (`pii-scan` identity 赤の唯一の原因)、
  ③ **#161 の 3 点観察** — 実行ボタンの不具合はここで止まっている。)

- 日付: 2026-08-13 (**滞留 PR の一掃と、CI の導入。マージ 9 本、open は #159 の 1 本のみ。**

  **入れたもの (develop)**: #166 CI ワークフロー (ADR-0104) / #167 接続の選択エクスポートと
  上書きインポート (ADR-0105) / #168 next-actions 同期 / #160 デモ用フィクスチャと
  スクリーンショット (ADR-0097・0098) / #163 貼り付け値の空白除去 (ADR-0099) /
  #162 文書セルのツリー表示・ステータスバー・ENUM プルダウン・Aurora DSQL の画面編集
  (ADR-0100〜0103) / #164 検証シート 003 (UI ロケール) / #149 姉妹リポ用の貼り付けブロック /
  #165 llms.txt。develop の HEAD は `694bcb3`。

  **CI (ADR-0104)**: ubuntu-latest で 3 ジョブ (cargo fmt/clippy/check/test ・
  svelte-check + vitest ・ site の node --test) + 既存の `scan` (pii-scan)。
  `push`/`pull_request` とも `develop` と `main` が対象。**Windows ジョブは意図的に無し** —
  既知の libSQL teardown segfault (#131) で緑のコードのまま恒久的に赤くなるため。
  **導入初日に 1 件検出**: `dbboard-config` の `secure_fs` テスト 4 本が Linux 限定で失敗。
  分類器 `is_likely_cloud_synced_path` は正しく、テスト側が
  `r"C:\Users\alice\OneDrive\..."` とバックスラッシュ区切りのリテラルを渡していたのが原因
  (Unix では `\` は区切りではないので `Path::components()` が 1 セグメントに潰れる)。
  セグメントから `PathBuf` を組む形に修正。`cfg(windows)` で消す案は、その分岐が Linux で
  一度も踏まれなくなるため採らなかった。`OneDriveBackup` の否定テストも
  Linux では**空振りで緑**になっていたので同時に直っている。#131 に報告済み。

  **ADR 番号の連続性**: develop は 0096 の次が 0105 だった。0097〜0104 が未マージの
  4 ブランチに分散していたため。4 本とも `docs/decisions.md` の同じ位置に追記するので
  互いにコンフリクトし、**1 本ずつしか解けない**。番号順になるよう差し込んで解消し、
  現在は 0096 → 0105 が連続している。

  **未了**: #159 (文書ストアをガイドに書く) はコンフリクト解消済み (`889a28a`) だが
  **未 push** — `target/release/dbboard-mcp.exe` が使用中で `cargo build --release` が
  上書きできず、pre-push が通らないため。衝突は `site/index.html` の OGP 1 箇所のみで、
  説明文は #159 側・プレビュー画像は develop 側 (ADR-0098) を採った。
  この commit だけ `--no-verify` を使用 (Windows libSQL segfault・CLAUDE.md が認める唯一の
  bypass)。`pii-scan --staged` は手動で clean を確認済み。

  **#161 (実行ボタンがクリックで反応しない) は調査停止中** — 報告者側の 3 点観察待ち。
  クリックと Ctrl+Enter が同一関数・同一ガードであることまで確認済みで、コードだけでは
  これ以上切れない。観察が来るまで推測でテストを書かない。)

- 日付: 2026-08-09 (**Zenn 記事の公開と、それを書く過程で判明した文書 / コードの乖離の是正。
  コミット 3 本 (`f0cb0ca` / `28c15cc` / `913ee8b`)、ブランチ `feature/firestore-adapter`。**

  **記事**: `articles/dbboard-mcp.md` →
  <https://zenn.dev/dokokade/articles/46b8c608715963> (公開済)。
  「MCP サーバーを作りました」ではなく、**読んだ人がその場で導入できる手順記事**という
  指定。裏取りは全部一次情報 + 実機で、`claude mcp add` → `claude mcp list` の
  `✔ Connected` → stdio で 9 ツールを実際に叩き、成功例も拒否メッセージも**出力を
  そのまま貼った**。Zenn は別リポからビルドされる (ファイル名 = スラグ) ため、
  こちらのコピーを正本として `published: true` + 冒頭 HTML コメントに URL を記録。

  **記事を書くために読み直したら、ドキュメントが 3 種類の嘘をついていた:**

  1. **write allowlist (4 ファイル)。** `DROP INDEX` と `COMMENT ON` が「通る」、
     `DROP` は「インデックス以外を閉じる」と書かれていたが、**どちらもコードは
     一度もそう振る舞っていない**。実際の許可は `INSERT`/`UPDATE`/`DELETE`/`MERGE`
     + `CREATE TABLE`/`VIEW`/`INDEX`/`SCHEMA`/`ALTER TABLE` のみで、`DROP` は
     インデックスを含め**全オブジェクトが永久拒否**。ADR-0087 は正しく、派生
     ドキュメントだけがずれていた。両端を固定するテストを `write_policy.rs` に
     2 本追加してから直した (テスト先行 / baseline §20)。
  2. **MCP に「環境変数で接続そのものを渡す」経路は存在しない。** `DBBOARD_MYSQL_URL`
     等は `dbboard-server` の単一接続解決パス専用。MCP の `adapter_for` は
     `connections.toml` + OS キーチェーンしか見ず、読む環境変数は `DBBOARD_CONFIG` と
     `RUST_LOG` だけ。クレート README の該当節を書き換え、**ルート README と
     `site/index.html` に残っていた同じ案内も潰した** (ルート README は既に存在しない
     節へリンクしていた)。
  3. **`docs/compatibility.md` に MySQL / MariaDB 節が無いまま 3 リリース経過** (ADR-0068)。
     `max_execution_time` (MySQL・ミリ秒) と `max_statement_time` (MariaDB・秒) の
     綴り分けと、**なぜ 1 回プローブしてキャッシュするか** (持っていない変数を聞くと
     hard error になる) を記録した。

  **教訓**: 対外記事を書くのは、派生ドキュメントの棚卸しとして機能する。ADR (正本) が
  正しくても、README / サイト / CHANGELOG は独立にずれる。**ずれを見つけたら、まず
  ずれを固定するテストを書いてから文章を直す**。

  **導線**: 「公開しただけでは広まらない」という以前の指摘に沿って、ルート README の
  MCP 節 / クレート README の冒頭ボックス / ダウンロードページの "Use it from an
  AI agent" の 3 箇所から記事へリンクした。

  **検証**: fmt / clippy -D warnings / check / test 全緑、`site/app.test.mjs` 6 件パス、
  `pii-scan --staged` clean。3 コミットとも hook の bypass は**既知の Windows libSQL
  teardown segfault** のみが理由で、コミットメッセージに明記済み。

  **user 側ボール** = ① 3 コミットの push、② 姉妹リポへ `.claude/tools/dbboard.md`、
  ③ ~468 コミットの history 書き換え判断、④ PR #148 / #149 の取り込み (feat なので次は 0.6.0)。
  **次のエージェント側タスク** = issue 0020 スライス 3 (`BackendConfig::MongoDb` +
  `connect_adapter` + デスクトップの**追加/編集フォーム両方** + サイドバーのクエリ生成
  + MCP ツール説明)。)

- 日付: 2026-08-06 (**v0.5.1 パッチリリース。実運用で出た 2 バグ。**

  **① 中身。** どちらもエッジケースではなく実際の作業を止めたバグ。
  SSH バスティオン経由の接続が死んだまま再起動するまで復帰しない件 (ADR-0092) と、
  MySQL 8 のスキーマ内省が全テーブルで失敗する件。前者の原因は 3 層に分かれていて、
  russh が keepalive を既定で送らない / 失敗したアダプタがキャッシュから追い出されない /
  sqlx は死んだフォワードに再ダイヤルするだけ、のどれか 1 つを直しても解決しない。
  keepalive (30s×3) + アイドル 30 秒後の ping-on-borrow + 手動リコネクト UI で塞いだ。
  後者は `information_schema` がデータディクショナリ由来で `VARBINARY`/`BLOB` を
  宣言するため。**MCP の `list_relationships` がテーブル単位のエラーを飲み込んで
  空の結果を返していた**のも同時に直っている (エラーではなく空、が一番たちが悪い)。

  **② タグを打つ順序を間違えた。** develop → main の PR を作る前に `main` で
  `v0.5.1` タグを打ったので、`main` はまだ `v0.5.0` の中身のままだった。
  リリースワークフローが v0.5.0 の内容を v0.5.1 として組み立て始めたところで気づき、
  run をキャンセル。`gh release view v0.5.1` が `release not found` で、
  リリースオブジェクトは未作成 = 公開物は出ていない。タグを削除してやり直した。
  **原因は手順の書き方**で、develop → main のマージを独立したステップとして
  示さないまま「`main` 上でタグを push」と書いたこと。次回は必ずマージを
  前提コマンドとして並べる。

  **③ develop → main を squash から真のマージに変えた (今回の構造的な変更)。**
  #134 と #146 が squash merge だったため develop のコミットが main の祖先にならず、
  #151 の共通祖先が `v0.4.0` まで戻って、両側が触った 9 ファイル
  (`Cargo.lock` / `CHANGELOG.md` を含む) が全部衝突した。**リリースのたびに悪化する。**
  main の内容は develop に完全に含まれていた (`git log develop..main` の 2 コミットは
  どちらも develop の内容の squash) ので、`merge -s ours --no-commit` +
  `read-tree --reset -u develop` でツリーを develop と一致させたマージコミット
  `b98f7a6` を作った。これで develop が main の祖先に戻り、次のリリース PR は
  衝突しない。
  **代償**: squash が隠していた noreply 切替前の古いコミットが main から到達可能に
  なり、**`main` の `pii-scan` identity が赤になった**。アドレス自体は元から develop
  側で公開されているので新規の漏洩ではないが、~468 コミットの history 書き換えを
  やるまで main の CI は赤のまま = 放置のコストが上がった。

  **④ 検証。** fmt / clippy -D warnings / check / test (dev, release) /
  `cargo build --release` / `pii-scan --tree` / `pii-scan --range develop..HEAD`
  がすべて緑。`release: v0.5.1` のコミット 1 本だけ `--no-verify` を使ったが、これは
  Windows libSQL の teardown segfault (13 テスト全部 ok の後にプロセスが落ちる)
  という既知の唯一の例外で、PII スキャンは hook の 1 番目で通過済み + 手動で再実行済み。
  マージコミット `b98f7a6` は hook を全部通している。)

- 日付: 2026-08-05 その4 (**v0.5.0 リリース + 文書ストアを Phase 6 に確定 (ADR-0091)。**

  **① v0.5.0 を切った。** 動機は「文書が既に約束しているから」— README / クレート
  README / DL ページの 3 箇所が `dbboard-mcp-windows-x86_64.exe` /
  `dbboard-mcp-macos-universal` を「最新リリースから取れ」と書いているのに、タグが
  存在しない間はどのリリースもそれを持っていない。**手順書はタグが存在して初めて
  真になる。** 流れ: PR #144 (release/v0.5.0 → develop) → #145 (ADR-0091) →
  #146 (develop → main、v0.4.0 から 87 コミット) → `main` 上で `v0.5.0` タグ push。
  検証は `eeddf91` に対して fmt / clippy / debug テスト / release ビルド /
  release テストを通しで実行し、**debug・release とも 985 passed / 0 failed
  (43 バイナリ)**。今回は Windows libSQL teardown segfault も出ず、`--no-verify` は
  一度も使っていない。
  **リリースオブジェクトの手動作成はもう不要になっている** — publish ジョブに
  `gh release view || gh release create --generate-notes` のブートストラップが入り、
  タグ push だけで公開まで通る。v0.1.0〜v0.3.0 で必要だった手順は消えた。
  `docs/roadmap.md` の Phase 5 に残っていた「create-if-missing は tracked follow-up」
  という古い記述もこの機に訂正した。

  **② `pii-scan` が #146 で赤。既知の未処理分であり、新規の混入ではない。**
  identity チェックが指したのは **2026-07-22 のコミット群** (noreply アドレスへ
  切り替える前のもの)。今回のセッションで作った 4 コミット
  (`e6db331` / `b51fd25` / `eeddf91` / `622b186`) は author / committer とも
  すべて noreply。develop → main の PR は「main に無い全コミット」を走査対象に
  するため、87 コミット分に含まれる古い identity がまとめて出た。main への push 後の
  `pii-scan` は緑。**~468 コミットの history 書き換えをやるか否かは user 判断待ちのまま**で、
  今回のリリースはその母数を増やしていない。

  **③ MongoDB / Firestore を stretch から確定フェーズへ格上げ (ADR-0091)。**
  MongoDB は「Additional adapters (PlanetScale, MongoDB)」という 1 行の半分でしかなく、
  Firestore はどこにも書かれていなかった (口頭合意のみ)。**記録されていない合意は
  decisions.md が防ぐべき失敗そのもの**なので、実装前に方向を確定させた。
  `dbboard-core` を読み直した結果、障害は 4 つあり **trait は障害ではなかった**:
  - `DatabaseAdapter::query(&self, sql: &str)` — **問題なし**。trait 側は文字列を
    一切パースしていない。Mongo のコマンドドキュメントも Firestore の
    `StructuredQuery` も JSON 文字列なので、そのまま渡せる。中間クエリ IR は不要
    (SQL と 2 つの非類似な文書 API をまたぐ抽象は、誰も求めていない上に非可逆)。
  - `Value` (`Null | Integer | Real | Text | Blob`) — **障害**。平坦。文書は木。
  - `read_only.rs` — **障害**。`sqlparser` ベースで、パースできない入力は
    fail closed が原則。そのままだと Mongo のクエリを全拒否する。MCP の書き込みゲート
    (ADR-0087) がこの上に乗っている。
  - `describe_table` — **障害**。宣言済みカラム前提。コレクションには無い。

  ここから順序が決まった: **入れ子 `Value` を単独で先に** (issue 0018。全アダプタの
  行構築と dbboard-web との共有ワイヤ契約に触るので、単独で出さないと壊れたとき
  原因が特定できない。`serde_json` が core の本番依存になるが、`serde` / `sqlparser` と
  同じ「パースは純粋なデータ変換なので no-I/O 規則は保たれる」論拠が既にある) →
  **Firestore** (issue 0019。REST が読み `:runQuery` / `:batchGet` と書き `:commit` を
  **エンドポイントで分けている**ので、read-only は「どのエンドポイントを叩けるか」で
  決まり、分類器そのものが存在しない = 間違えようがない) → **MongoDB** (issue 0020。
  `runCommand` が何でも受けるので fail-closed の読み取り許可リストが必要。さらに
  `$out` / `$merge` は read であるはずの `aggregate` の中から書くため、verb を見るだけでは
  足りずパイプラインを歩く必要がある。`read_only.rs` が `starts_with("SELECT")` を
  拒否しているのと同じ罠 —
  `WITH x AS (DELETE … RETURNING *) SELECT * FROM x` は `WITH` で始まる書き込み)。
  スキーマは**サンプリングして、サンプリングだと分かる形で出す** — 推論を宣言済み
  スキーマと同じ見た目で描くと、人はそれを宣言済みとして信頼することを学習してしまう。
  **PlanetScale は新規アダプタ不要** (MySQL 互換なので既存 `dbboard-mysql` で届く。
  古い stretch 行は無関係な 2 つを束ねていた)。

  コミット: `eeddf91` (release: v0.5.0)、`622b186` (ADR-0091 + roadmap Phase 6 +
  issue 0018/0019/0020)。両方とも pre-commit フル通過。)

> 2026-08-05 その3 〜 2026-07-31 のセッションログは、baseline §31 に基づき
> [`.claude/archive/project-status-2026-08.md`](archive/project-status-2026-08.md)
> へ全文退避した (要約ではない)。さらに古いものは
> [`.claude/archive/project-status-2026-07.md`](archive/project-status-2026-07.md)。

## 注意点・既知の問題

- `develop` がデフォルトブランチ。Phase 2 完了時は `feature/adapter-trait-capability`
  → `develop` の PR を出す。release タグ運用は v0.1.0 で確立済 (`develop` → `main`
  release PR → tag push)。
- WEB 版 (`meta-taro/dbboard-web`) と同時並行で進めない、というルールは
  **「同じ contract layer」に限定**して運用する。今回の PWA pivot は contract に
  触らないため、desktop Phase 2 と並行可 (web 側 Claude が独立に担当)。
- Push は人間が実行する。エージェントは commit までで止めること。
- **Norton AV が claude.exe を quarantine するパターン**: pre-push の release build
  だけでなく、`@anthropic-ai/claude-code` の bin/claude.exe 本体も `.old.<timestamp>`
  にリネームされる事例を確認 (本セッション)。再発したら同じ手順 (リネームし戻し →
  ダメなら `npm i -g @anthropic-ai/claude-code` 再インストール)。Norton の例外設定
  追加も検討余地あり。memory `env-windows-norton.md` 更新候補。
- **GitHub Desktop の push が `remote: fatal error in commit_refs` で失敗するケース**:
  PowerShell `git push -v origin <branch>` でリトライすると通る。原因は GitHub Desktop
  と git CLI の細かい挙動差 or タイミング起因と推測。

## 開発ペースに関するメモ

- 二つのリポジトリを同時に同じ contract layer で進めない (Roadmap の Pacing Note 参照)。
- contract (アダプタ shape、エラー区分、スキーマスナップショット形状) の変更は
  両 repo の `docs/decisions.md` に ADR を書いてから着手する。
- 機能パリティは目標であって強制ではない。desktop 側で先に新アダプタを実装し、
  必要に応じて web 側に展開するリズムで進める想定。
- ただし **contract に触らない strategic な変更** (PWA pivot 等) は両 repo 並行で
  進めて OK。web 側の判断と進捗は web 側 Claude セッションに委譲。
