# アーカイブ — project-status.md セッションログ (2026-07-31 〜 2026-08-16 その2)

baseline §31 に基づく退避。`.claude/project-status.md` が 400 行トリガに
達したため、全文退避した (要約していない)。3 回に分けて退避している:

- 2026-08-09 退避 — 2026-07-31 〜 2026-08-05 その3 (v0.5.0 リリース周辺まで)
- 2026-08-19 退避 — 2026-08-05 その4 〜 2026-08-09 (v0.5.1 と Zenn 記事の回)
- 2026-08-24 退避 — 2026-08-13 〜 2026-08-16 その2 (CI 導入・滞留 PR 一掃 〜 v1.0 ゲート 4 を閉じた回)
- 2026-08-25 退避 — 2026-08-19 (検証シート 003 のために MCP へ 7 動詞を足した回)。
  ファイル末尾の「## 退避日 2026-08-25」以降がそれ。

これより古いものは `.claude/archive/project-status-2026-07.md`。

---

- 日付: 2026-08-16 その2 (**v1.0 ゲート 4 (コード署名) を「買わない」側で閉じた (#178)。
  残り 3 ゲート、全部 user 側。open PR = 0。develop = `7e275a8`。**

  **やったこと**: ADR-0106 を追加し、README 2 箇所 / `site/index.html` /
  `release.yml` の `--notes` を「未署名は決定であって漏れではない」に統一。
  `site/page.test.mjs` に文言の回帰テストを 1 本追加。
  `.claude/issues/0021-v1-0-criteria.md` のゲート 4 を CLOSED にした。

  **なぜ文言の書き換えが本体だったか**: 買う予定が無いのに `not signed **yet**` /
  `planned follow-up` / `tracked follow-up` と書き続けると、読み手は
  **後のリリースで消える不具合**だと受け取る。待つか、誤った前提で警告を通すかの
  どちらかになる。未署名で出すこと自体は擁護できるが、そう言わずに出すのは擁護できない。
  ADR-0044 の「有料の後回し」という位置づけは当時は正確だったが、1 年分のリリースを
  経て**事実上の誤情報**になっていた。

  **注記の置き場所**: ダウンロードが提供される場所すべて (DL ページ / README 2 箇所 /
  リリース本文)。検索結果からリリースページに直接来た人は README を見ないし、
  インストーラを手渡しされた人はどちらも見ない。docs ではなくダウンロードに随伴させる。

  **テストで守る判断**: この文言はすでに 2 ファイルで同時に間違っていた。
  記憶で守れていない実績があるので、`site/page.test.mjs` が
  `yet` / `planned` / `tracked follow-up` / `coming soon` の再出現で落ちるようにした。
  先に RED を確認 (`doesNotMatch` が `tracked follow-up` を検出して失敗) してから直した。

  **リポ設定**: `delete_branch_on_merge` を `true` に変更。マージ後のリモートブランチ
  削除が自動化され、PR ごとの削除 push が消えた (#178 で自動削除を実際に確認)。

  **検証**: pre-commit (fmt / clippy -D warnings / check / test) 通過。
  pii-scan は tree・commit message とも clean。`node --test site/*.test.mjs` 16/16。
  `release.yml` は PyYAML パース + 抽出した run ブロックの `bash -n` で構文検証。
  PR #178 の CI 4 本すべて緑。**動作確認 (アプリやリリースを実際に走らせての確認) は
  していない** — リリース本文の変更が効くのは次のタグを打ったときで、そこは未検証。

  **本日マージ**: #176 (`actions/checkout` v6)、#177 (セッション記録)、
  #178 (本件)。**残り**: v1.0 の 3 ゲート = #161 の 3 点観察 / `dbboard-web` への
  コントラクトのミラー / 検証シート 001–003 の実施。3 つとも baseline §38 の
  「人にしかできない工程」で、エージェント側から代われない。)

- 日付: 2026-08-16 (**v1.0 の条件を確定させ、凍結の前提としてコントラクトのずれを直した (#175)。**

  **やったこと**: `.claude/issues/0021-v1-0-criteria.md` に v1.0 の条件を 4 つだけ書いた。
  `docs/api-contract.md` の 4 件のずれを修正し、`crates/dbboard-connect/tests/api_contract_drift.rs` で
  再発を止めた。`docs/roadmap.md` の帳簿 (Phase 2 の `*(current)*`、Export results の行) を直した。

  **なぜ 4 つに絞れたか**: 本リポの SemVer 上の公開 API は HTTP 契約 (ADR-0011) であって
  機能一覧ではない。**エンドポイントやフラグの追加は additive で何も壊さない**ので、
  未実装の機能は 1.0 を妨げない。1.0 を妨げるのは「後から契約を変えざるを得なくなるもの」と
  「約束が嘘になるもの」だけ。この基準でロードマップ上の未着手項目を通すと 4 つだけ残る
  (#161 / 姉妹リポへのミラー / 検証シート未実施 / コード署名)。
  Saved queries・JSON エクスポート・Linux パッケージ等はどれも契約に触れないので 1.x で足りる。
  **これらをゲートにすると 1.0 は永久に来ない** — その失敗を避けるためのリストである、と
  issue 側にも明記した。

  **コントラクトは凍結できる状態になかった (4 件)**: `id` の一覧が 3 件のまま (実際は 9 件出荷済)、
  `has_foreign_keys` (ADR-0054) がワイヤに乗っているのに未記載、`GET /capabilities` の例 (5 フラグ) と
  `Capabilities` の節 (10 フラグ) が食い違い、「Phase 2 では全フラグ `false`」など事実でなくなった
  記述が 3 箇所。あわせて **`true` のフラグが必ずしも HTTP エンドポイントを意味しない**
  (Tauri IPC 経由の面がある・ADR-0089) 点を明文化した。姉妹リポはこの文書を実装根拠にするので、
  ずれたまま凍結すると 2.0 まで直せなくなる。

  **テストは先に RED を確認した**: 修正前に走らせて
  `["mysql","neon","supabase","aurora-dsql","firestore","mongodb"]` と `["has_foreign_keys"]` が
  欠落として出ることを見てから直した。置き場所を `dbboard-connect` にしたのは、
  全アダプタと `dbboard-core` の両方に依存する唯一のクレートだから。

  **エージェント側のミス 1 件 (記録)**: #175 の本文に、PR に含まれていない変更
  (`actions/checkout` の v6 更新) を書いた。**`git log origin/develop..HEAD` を
  ローカル HEAD で数えた**のが原因で、その commit は push 時点の先端に無かった。
  **PR 本文は `origin/<branch>` を基準に数える。** マージ後に本文へ訂正を追記し、
  中身は `ci/checkout-v6` に cherry-pick して patch 一致を確認した。

  **CI**: #175 は ci / pii-scan とも緑。pre-push は 1 度目に例の Windows libSQL テアダウン
  segfault で中断したが、`cargo test --all-features --release` を手元で通すと
  **テストバイナリ 52 本すべて ok・失敗 0** で、再実行して通った。)

- 日付: 2026-08-14 その2 (**v0.8.0 をリリースした。前のリリースを"使って"出てきた
  改善だけで組んだ初めてのリリースで、新規アダプタは無い。**

  **入れたもの**: #171 エクスポートダイアログの接続リストの可読性修正 → develop、
  #172 リリース準備 (CHANGELOG / roadmap / DESIGN.md / バージョンスタンプ) → develop、
  #173 `develop` → `main` のリリース PR。`main` = `29413b4`、タグ `v0.8.0` を push 済。

  **リリース内容** (ADR 単位): 0100 文書セルのツリー表示 / 0101 ステータスバー /
  0102 ENUM プルダウン (MySQL) / 0103 `aurora-dsql-iam` の画面編集 /
  0105 選択エクスポートと上書きインポート / 0097 `DBBOARD_CONFIG_DIR` /
  0099 貼り付け値の先頭空白除去 / (ADR 無し) エクスポートダイアログの可読性・
  CI で必須検証コマンドを回すようにしたこと。

  **なぜ 0.8.0 (minor) か**: 本リポの SemVer 上の公開 API は `docs/api-contract.md` の
  HTTP 契約 (ADR-0011) で、今回はそこに触れていない。`BUNDLE_VERSION` も据え置きなので
  0.8.0 が書いたバンドルは 0.7.0 でも開ける — 動いたのはエクスポート/インポートの**挙動**だけ。

  **リリース前に塞いだ穴**: `CHANGELOG.md` の `[Unreleased]` が**空のまま**だった
  (v0.7.0 以降に機能 6 + 修正 2 が入っているのに)。`docs/roadmap.md` の現況ブロックも
  v0.7.0 を現行として説明していた。どちらも「更新するか」を判断する人が読む場所で、
  **タグを打った後では埋められない**。両方この PR で埋めてから切った。

  **エージェント側のミス 1 件 (記録)**: DESIGN.md の 3 コンポーネント仕様 (`128f18e`) を
  #171 のブランチを push した**後**に commit したため、#171 のマージに乗らなかった。
  リリースブランチを `origin/develop` に rebase してから `git cherry-pick` で拾って
  `19d0564` として復旧。**push 済みブランチに追加 commit を積んだら、その PR が
  マージ済みでないか必ず確認する。**

  **CI**: #172 (ci 4m41s / pii-scan 9s)、#173 (ci 4m59s / pii-scan 17s)、
  `main` への push 後 (ci 3m36s / pii-scan 12s) すべて緑。タグ push で release CI
  (run `31784033330`) が起動 — Windows exe + MSI / macOS dmg を作って
  `SHA256SUMS.txt` 付きで publish する。v0.5.0 以降 publish ジョブが
  release オブジェクトを自力で view-or-create するので、**タグ push だけで完結する**。

  **release CI は 5 ジョブすべて緑**で、`v0.8.0` は 08-14 08:44Z に publish 済
  (`dbboard-desktop_0.8.0_x64-setup.exe` / `_universal.dmg` / `.app.tar.gz` /
  MCP の win・mac / `latest.json` / `SHA256SUMS.txt`)。DL ページは
  `releases/latest` を指しているだけなので**サイト側の変更は不要**。

  **残っているボール (すべて user 側)**: ① **公開された `.exe` の PII 目視確認**
  (CI はやらない・人間の作業)、② baseline §24 の security-reviewer をこのリリースで
  回すかの判断 (推奨はする。今回の変更は既存経路の UI 改善で新しい外向き通信は無い。
  v0.7.0 時点の実施記録はこのファイルに無い)、③ 検証シート 001/002/003 が全部 `未実施`、
  ④ 姉妹リポへ `.claude/tools/dbboard.md` を貼る、⑤ ~468 コミットの history 書き換え判断、
  ⑥ **#161 の 3 点観察** — 実行ボタンの不具合はここで止まったまま。)

- 日付: 2026-08-14 (**open PR = 0。PR の滞留は解消しきった。**

  **入れたもの (develop)**: #159 文書ストアをガイドに記載 / #169 08-13 のセッション記録。
  develop の HEAD は `8dd3ac5`。**未マージの PR は残っていない。**

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
  #165 llms.txt。develop の HEAD は `d89be6e`。

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

  **未了**: #159 (文書ストアをガイドに書く) はコンフリクト解消済み (`1ab3e74`) だが
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
  `bf7a696` を作った。これで develop が main の祖先に戻り、次のリリース PR は
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
  マージコミット `bf7a696` は hook を全部通している。)

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


- 日付: 2026-08-05 その3 (**push 失敗 3 連続の原因はコードではなくマシン資源の枯渇 2 種。
  コミット `e6db331`。**
  **① メモリ (対処済み・コード変更あり)**: `cargo test` が
  `memory allocation of 268435456 bytes failed` → `dbboard_config-*.exe (exit code:
  0xc0000409, STATUS_STACK_BUFFER_OVERRUN)` で異常終了。268435456 = ちょうど 256 MiB で、
  age の scrypt KDF (log_n=18, r=8 → 128 × 2^18 × 8) が **暗号化 1 回 / 復号 1 回ごとに**
  確保する連続領域。テストハーネスは論理コア数 (このマシンは 20) だけ並列に走るので、
  bundle / export / import 系が同時に到達すると数 GB を一斉に要求する。Rust の
  アロケート失敗ハンドラがプロセスを abort し、Windows がそれを
  `STATUS_STACK_BUFFER_OVERRUN` という名前で報告するため **メモリ安全性のバグに見える**
  が違う。**空きメモリ次第で通ったり落ちたりする**ため、pre-push が「リトライすれば
  そのうち通る」学習を生む点が最も悪質。対処: `crates/dbboard-config/src/bundle.rs` の
  `encrypt_bundle` / `decrypt_bundle` に `#[cfg(test)]` 限定の mutex (`kdf_guard`) を入れ、
  テスト時のみ KDF を直列化。**本番はロックしない** — export / import は人が意図して
  1 回ずつ行う操作であり、テストハーネスの都合で実運用を遅くするのは本末転倒。
  新規テストは付けていない (挙動ではなくテストの実行のされ方を変える変更であり、
  再現テストを書くと「そのマシンの空きメモリ」に依存する = 消したい非決定性そのもの)。
  **② ディスク (対処済み・コード変更なし)**: `target/debug` が **71.6 GB**
  (release は 11.9 GB) まで膨らみ、C: の空きが **225 MB**。症状は
  `error: linking with 'link.exe' failed: exit code: 1318` と
  `failed to build archive ...: ディスクに十分な空き領域がありません。 (os error 112)` で、
  **リンカ/ツールチェーンの問題に見える**。`cargo clean --profile dev` で 71.6 GB 解放
  (pre-push が使う `release/` は残す。全体 `cargo clean` だと pre-push が全ビルドし直しになる)。
  **検証**: `cargo fmt --all -- --check` OK / `cargo clippy --all-targets --all-features
  -- -D warnings` OK / `cargo test --all-features` = **943 passed / 0 failed**、
  唯一の異常終了は既知の Windows libSQL teardown segfault (`dbboard-turso` の全テストが
  ok を出した後に `STATUS_ACCESS_VIOLATION`) = 唯一許可された `--no-verify` ケース。
  `scripts/pii-scan.sh --staged` と `--message` を手で実行し両方 clean を確認してから commit。
  どちらもリポジトリ固有ではなくこのマシン固有の事象なので、`.claude/` ではなく
  エージェントメモリ側に記録した。)

- 日付: 2026-08-05 その2 (**`dbboard-mcp` の配布経路を新設。ADR-0090。コミット `c015b17`。**
  発端は user 経由で届いた「使いたいのに使えない AI エージェント」の意見。要求は
  (1) 入手元をリポジトリ内のファイルに書く (2) MCP 登録コマンドをコピペできる 1 行で。
  調べた結果、**根本原因は文書ではなくバイナリが一度も配布されていなかったこと**。
  `dbboard-mcp` は `tauri.conf.json` の `externalBin` / `resources` にも `release.yml` にも
  存在せず、入手手段は `cargo build --release -p dbboard-mcp` のみだった。
  **デスクトップインストーラへの同梱は却下** — エージェントが必要とするのは
  `claude mcp add` に渡す絶対パスであり、インストールツリーに埋めると手順が
  「このマシンのこの OS のこのインストーラ版ではどこか」の当て推量になる。
  **release CI**: `build-mcp-windows` (cargo build のみ) と `build-mcp-macos`
  (aarch64 + x86_64 → `lipo` universal) を追加、それぞれ checksum ファイル付き。
  `publish.needs` を 4 ジョブに更新。`latest.json` の glob (`out/*-setup.exe` /
  `out/*.app.tar.gz`) は MCP の資産名と一致しないので updater は無影響。
  **DL ページ**: 意図的に出さない。`bucketFor` は製品名接頭辞判定なので
  `dbboard-mcp-windows-x86_64.exe` は `.exe` でも null になる — 拡張子判定だったら
  「Windows 版をダウンロード」を押した人に headless な stdio サーバーを渡していた。
  `site/app.test.mjs` に固定テストを追加 (6 tests / 全緑)。
  **文書**: README / `crates/dbboard-mcp/README.md` / `site/index.html` の 3 箇所に
  OS 別の配置先 (`%LOCALAPPDATA%\dbboard\` / `~/.local/bin/`) とそこから導かれる
  `claude mcp add ... --scope user -- <path>` を記載。macOS は未署名なので
  `xattr -d com.apple.quarantine` も併記。クレート README には
  *Credentials without writing a file* (`DBBOARD_*` を `mcpServers` の `env` に置く。
  ただし `~/.claude.json` 自体がディスク上のファイルである点も明記) と
  *Behind a TLS-terminating proxy* (OS トラストストアが唯一のモード = ADR-0034、
  `--use-system-ca` 相当は**無い**、直す場所は OS 側) を新設。
  エージェントが評価していたエラー文言そのままの表
  (*Troubleshooting a failed connection*) は既存なので拡張のみ。
  **姉妹リポ向け** `.claude/tools/dbboard.md` の中身は用意して user に渡した
  (当リポからは編集不可 = baseline §27)。
  **検証**: pre-commit フック全緑 (fmt / clippy / check / test)、`node --test
  site/app.test.mjs` 6 pass / 0 fail、`yaml.safe_load` で release.yml のジョブ 5 件を確認。)

- 日付: 2026-08-05 (**issue #139 = egui クライアント退役 (ADR-0089) + 発見可能性の是正。**
  ブランチ `chore/retire-egui`。
  **削除**: `crates/dbboard-ui` / `apps/dbboard` / `crates/dbboard-i18n`、workspace
  メンバ、`eframe` / `egui_extras` / `egui_commonmark`、`deny.toml` の
  RUSTSEC-2026-0194 / -0195 の ignore 2 件 (抑制はそれを必要としたコードより長生き
  するので同じ変更で外す)。ブランド資産は `assets/` へ移設 (`dbboard.ico` /
  `dbboard-logo-256.png` — 削除したバイナリ配下にあっただけで egui のものではない)。
  **残した**: `crates/dbboard-server`。in-repo の consumer は消えたが dbboard-web が
  ミラーする HTTP 契約の実行可能な仕様書 = 削除はアーキテクチャ決定 (baseline §16)。
  死んだコードに見えるのが分かっているので、理由を module doc / `docs/api-contract.md` /
  `docs/architecture.md` の 3 箇所に書いた。
  **リリース CI**: cargo 版の `build-windows` / `build-macos` を撤去。以降
  `dbboard-windows-x86_64.exe` / `dbboard-<v>-x86_64.msi` /
  `dbboard-macos-universal-<v>.dmg` は publish されない。`SHA256SUMS.txt` は残りを
  引き続き網羅。
  **DL ページ**: `bucketFor` を拡張子判定から `dbboard-desktop` 製品名接頭辞判定へ
  (#135 を supersede)。v0.4.0 は両クライアントの資産を持つため、拡張子だけだと
  Releases API の返す並び順で提示するビルドが変わっていた。`site/app.test.mjs` に
  v0.4.0 の実資産 10 件を流す回帰テストあり (5 tests / 全緑)。
  **文書**: README / CLAUDE.md / DESIGN.md / docs/architecture.md / api-contract.md /
  compatibility.md / roadmap.md から現在形の egui 記述を一掃。「egui から移植した」と
  いう由来の記述は残す (コードがそう見える理由の説明なので)。`docs/desktop-parity.md`
  は archived バナー付きで凍結 (追跡すべき差分が無くなったため)。
  **web 側ミラー不要 (明示的 no-op)** — 共有契約は一切変わっていない。
  **発見可能性 (user 指摘「公開しただけじゃわからない」)**: web 検索を持つ別エージェントが
  dbboard を「一般公開されているツールではない」と結論していた。原因は 4 つとも構造的
  だったので全部潰した — リポジトリの `homepageUrl` が空 → DL ページに設定、topics が
  0 個 → 15 個追加、README の DL リンクが fold 下 → タイトル直下にバッジ付きブロック、
  リリースページが生の資産名だけ → `gh release create --notes` (これは
  `--generate-notes` の出力に **prepend** される) で冒頭に DL リンク。加えて site に
  canonical / og / `robots.txt` / `sitemap.xml`、CLAUDE.md・`crates/dbboard-mcp/README.md`・
  `apps/desktop/README.md` に URL 明記 (CLAUDE.md には「姉妹リポでも聞かれたらこの URL を
  答える」と書いた)。
  **検証**: `cargo fmt --check` / `clippy -D warnings` / `check --all-targets` /
  `test --all-features` 全緑 (0 failed)、`node --test site/app.test.mjs` 5/5、
  `apps/desktop` の `pnpm check` 271 files 0 errors / `pnpm test` 346 passed。
  **未着手**: 姉妹リポの `browser-verification.md` に dbboard の URL と
  `claude mcp add` 行が無い — 当リポからは編集できない (baseline §27) ので user 中継待ち。
  `crates/dbboard-server` の宙ぶらりん状態を扱う follow-up issue も未起票。)

- 日付: 2026-08-04 その3 (**v0.4.0 リリース。CI の Node バージョンずれを 1 件修正。**
  リリースが 2 週間止まっていた理由は、`Cargo.toml` のバージョンだけ 0.4.0 に上がって
  `CHANGELOG.md` の `## [Unreleased]` が空だったこと = タグを打つ根拠が無かった。
  ADR-0047〜0086 を棚卸しして 0.4.0 節を書き (`0359da6`)、compare リンクも v0.4.0 を
  追加して修正。PR #133 で main へ、タグ `v0.4.0` を push。
  **1 回目のタグビルド (run 30885499852) は Tauri 2 の 2 ジョブが `Install frontend deps`
  で即死**: `Error [ERR_UNKNOWN_BUILTIN_MODULE]: No such built-in module: node:sqlite`。
  `release.yml` の `actions/setup-node` が `node-version: 20` 固定、一方
  `apps/desktop/package.json` は `"packageManager": "pnpm@11.1.1"` を pin しており、
  pnpm 11 は `node:sqlite` を import する (Node 22.5 以降にしか存在しない)。
  つまり pnpm が 1 パッケージも解決しないまま落ちる。**手元の Node は v22.22.2 なので
  ローカルでは絶対に再現しない種類の失敗** — CI 側の pin だけが古かった。
  cargo のみの `build-windows` / `build-macos` は成功し `publish` は skip されたため、
  **release object が作られておらず何も publish されていない** (`gh release view v0.4.0`
  → release not found) = タグを安全に張り直せる状態だった。
  修正は両 `setup-node` を `node-version: 22` にするだけ (PR #134)。
  ただし #133 が squash マージだったため develop と main が `release.yml` で衝突し、
  `origin/main` を develop に取り込んで解決 (`d881fc5`)。#134 は **merge commit** で
  取り込み (`4a3364e`) — squash を続けると同じ乖離が毎回出るため。
  タグは `git tag -d` + `git push origin :refs/tags/v0.4.0` で消してから main 先端
  (`1dad53e`) に張り直し、**run 30888094754 は 5 ジョブ全緑で publish まで到達**。
  リリース v0.4.0 (draft=false) に 10 資産が付いた: `dbboard-windows-x86_64.exe` /
  `dbboard-macos-universal-0.4.0.dmg` / `dbboard-0.4.0-x86_64.msi` /
  `dbboard-desktop_0.4.0_x64-setup.exe` (+`.sig`) /
  `dbboard-desktop_0.4.0_universal.dmg` / `dbboard-desktop.app.tar.gz` (+`.sig`) /
  `latest.json` / `SHA256SUMS.txt` = egui 版と Tauri 版の両方、updater 署名込み。
  **運用方針 (user から常設)**: リリースは良い区切りで頻繁に切る。
  → エージェント側は **feat PR ごとに `## [Unreleased]` へ 1 行足す**ことで、
  いつタグを打っても変更履歴が揃っている状態を維持する。
  **MCP の書き込み対応が未決**: 現状の 7 tool は全て読み取り専用で、user から
  「使い物にならない」と指摘。`crates/dbboard-mcp/src/service.rs` には未公開の
  `apply_row_update` / `plan_dump` / `run_dump` / `plan_restore` / `run_restore` がある。
  公開してよいのは `apply_row_update` まで。**接続 CRUD は開けない** — agent が接続定義と
  keychain ref を書き換えられるようになり、baseline §15 の「credential 操作は人間のみ」を
  構造的に壊すため。dump/restore も同様に破壊的。v0.5.0 スコープにするかは user 未回答。
  なお commit `743ecea` と `d881fc5` は pre-commit の cargo test が例の Windows libSQL
  teardown segfault (rc=139) で落ちたため `--no-verify`。pii-scan は両方 clean、
  変更は YAML のみ。)

- 日付: 2026-08-04 その2 (**#130 クローズ + 記録の訂正 2 件。**
  PR #132 (squash `b08bb69`) が develop にマージされ、issue #130 はクローズ済
  (計測値を添えたコメントを投稿)。`feature/desktop-design-polish` の 14 コミットも
  push 済 (`7f4f940..42dfa1c`)。両 push とも pre-push は全緑で、既知の libSQL teardown
  segfault も出なかった (= `--no-verify` 不使用)。所要は修正後の実測どおり 1 回あたり約 58s。
  **訂正: #42 (外部 bastion 経由の live MySQL 検証) は未着手ではなく完了済だった。**
  user の指摘を受けて実機側を確認 — dbboard の接続一覧に MySQL 種別が 1 件登録されており、
  `connections.toml` の当該ブロックに `[connections.ssh]` と鍵パスフレーズの keyring ref が
  存在する = SSH トンネル経由の構成。ステータス側が数エントリにわたり「未着手」を
  引き写していたのが誤り。**接続情報そのもの (host / user / port) は tracked ファイルには
  書かない** — 非公開メモリと `.pii-denylist` のみ。
  なお `.pii-denylist` はこの端末に存在しないため、スキャナはこの種の情報に対して盲目である
  (ADR-0085 で記録済の既知ギャップ)。書かない運用で担保するしかない。
  **残る user 側ボール = 公開済 468 コミットの履歴書き換えの判断のみ。**
  未 push のローカル作業が無くなったので、実施するなら今が最も安全な時点。)

- 日付: 2026-08-04 (**issue #130 = `dbboard-desktop` が pre-push のたびに再コンパイルされる
  問題の原因確定と修正。commit `e271726`、ADR-0086。変更は
  `apps/desktop/src-tauri/Cargo.toml` の `crate-type` 1 行。**
  issue が推測していた `build.rs` の `cargo:rerun-if-changed` 欠落は**外れ**だった。
  `cargo build --release` を 2 回続けると 1s / 再コンパイル 0 件なので無条件リビルドではなく、
  再コンパイルが出るのは `cargo build --release` と `cargo test --all-features --release` が
  **交互に走るときだけ** = pre-push そのものの形。
  `CARGO_LOG=cargo::core::compiler::fingerprint=info` の出力が
  `dirty: UnitDependencyInfoChanged` を lib ユニットに名指しし、バイナリは
  `FsStatusOutdated(StaleDependency)` = 波及にすぎなかった。決定的だったのは、両コマンドが
  **同じ** `target/release/.fingerprint/dbboard-desktop-<hash>/lib-dbboard_desktop_lib.json`
  を書いていたこと。cargo は通常ユニットを `-C metadata` にハッシュ化して構成ごとに
  fingerprint を分けるが、`staticlib`/`cdylib` はリンカが見つけるために**出力ファイル名が
  固定**なのでハッシュを変えられず、パスも分かれない。中身が違うことも確認済 —
  `--all-features` はこのワークスペースでは no-op (`[features]` も `optional = true` も
  存在しない) だが dev-dependency はグラフに合流し、`cargo tree` の dev 有無の差分で
  `hyper` が `full`/`http2`、`hyper-util` が `http2`、`slab` が `default`、`tempfile` が
  `getrandom` を得る。つまり記録される依存ハッシュは正当に違い、それが 1 つの枠で
  上書きし合っていた。範囲の裏取りに `.fingerprint` ツリー全体を比較すると
  **1210 ユニット中、差があるのは 1 つだけ** (片側にしか無いユニットは 0)。
  **修正 = `crate-type = ["rlib"]`。** `staticlib`/`cdylib` は Tauri テンプレート由来で、
  モバイルホストが lib をリンクするためのもの。本アプリは desktop 専用 (`main.rs` は
  `run()` を呼ぶだけ、`gen/android`・`gen/apple` は無く、`cfg(mobile)` もソースに 0 件)。
  **実測 (warm ツリーで交互実行): test 直後の build 42s/1件 → 2s/0件、build 直後の test
  94s/1件 → 56s/0件、pre-push 合計 約136s → 58s。** issue に記録した 237s との差は
  マシン負荷と AV スキャンの揺れで、修正が取り除くのは「最大クレートのフルコンパイル
  1 回 × 往復 2 回」という固定量。残る 56s はテスト実行時間 = 明示的にスコープ外。
  **検証**: fmt/clippy/check/`cargo test --all-features` (48 スイート) すべて green、
  `cargo test --all-features --release` rc=0 (今回は libSQL teardown segfault も出ず)、
  release バイナリの起動・常駐を確認、pre-commit フック全通過 (`--no-verify` 不使用)。
  **副次的な発見 = このリポには cargo の CI が存在しない** (`.github/workflows/` は
  `pages.yml` / `pii-scan.yml` / `release.yml` の 3 つのみ)。つまり
  **pre-push が唯一のビルド・テストゲート**であり、遅さを理由に飛ばされると検査は
  どこにも残らない。#131 が持ち込んだ baseline §35 は「最後の砦は CI」を前提にするが、
  その前提はこのリポでは成立しない。cargo CI の新設が次の候補。)
- 日付: 2026-08-03 (**identity 誤検出の除去 = ADR-0085 (PR #128 `aa90129` / PR #129 `27824b0`)、
  および CI の denylist 層が初めて実稼働。コード変更は `scripts/pii-scan.sh` の許可正規表現
  1 行のみ。** 発端はセッション開始時の §18 手順で `develop` の `pii-scan` が赤だったこと。
  **(1) ADR-0084 の穴が 2 つ連続で出た。** 1 つ目は**本物**: GitHub の「Squash and merge」は
  この clone が書いていないコミットを web UI 側で作るので、`git config user.email` を
  noreply にしても**アカウントのプライマリアドレスが author に入る**。PR #127 の squash
  コミット `c355802` がこれで、CI が赤くなった。対応は GitHub の
  Settings → Emails → **Keep my email addresses private** を ON (§15 = human 操作、user が実施)。
  効果は次の squash `aa90129` の author が noreply になったことで実証済み。
  2 つ目は**誤検出**: 同じ `aa90129` の *committer* が `noreply@github.com` — GitHub 自身の
  web-flow アドレスで、`users.` 配下ではないため ADR-0084 の許可正規表現が弾いていた。
  **つまり ADR-0084 が着地して以来、web マージのたびに誤検出が出ていた**。しかも
  「設定で直せる本物の author リーク」と「設定では絶対に直らない committer 誤検出」が
  同じ赤い X として出るので、**片方がもう片方を隠していた**。条件が成立してもしなくても
  同じ結果を返す検査は情報を運ばない。**ADR-0085** = 許可を全文 alternation にして
  `noreply@github.com` を 1 個だけ追加。author/committer 同じ述語を使う (このアドレスは
  アカウントではなく GitHub のものなので誰も識別しない)。RED-first で selftest に
  3 つの正例と `evil-noreply@github.com` / `noreply@github.com.example.com` の負例を追加。
  **(2) CI secret `PII_DENYLIST` が存在しなかった** = PR #128 の緑を確認しに行って発覚。
  日次スキャンのログに `denylist: PII_DENYLIST secret absent — generic rules only` /
  `literal name detection off` とあり、**CI は実店舗名 3 件を含む `develop` を、clean
  だからではなく照合対象を持っていなかったから緑にしていた**。§15 に従い投入手順のみ提示し
  エージェントは触らず、user が作成。run 30784716586 で `denylist: materialized from secret`
  を確認、ツリーは advisory の `passworded-db-url` のみで BLOCKING 無し。
  **結果**: `develop` の `pii-scan` は push run 30786499201・日次 run 30803841065 とも green。
  **残る穴 (次セッション候補)** = ローカル `.pii-denylist` と CI secret の**中身の一致は
  誰も検証していない**。貼り間違い・部分コピーでも CI は緑になる。既存の `denylist_id()`
  (sha8) を使う `--denylist-digest` モードで、中身を出さずに突き合わせられる。
  **issue #130 を起票** = `dbboard-desktop` が毎回フル再コンパイルされ pre-push が約 4 分
  (実測 warm で build 99s / test 138s / 計 237s)。harness の 2 分上限を超えるので push は
  別ターミナル必須。原因は `build.rs` に `cargo:rerun-if-changed` が無いこと**と推測**する
  段階で、確定は `CARGO_LOG=cargo::core::compiler::fingerprint=info` を先に取ること。
  test 側 138s は実行時間なのでビルドを直しても残る。)
- 日付: 2026-07-31 (**コミット identity のスキャンを新設 = ADR-0084 (commit `cf11913`)、
  および記録ファイルの棚卸し (baseline §31)。コード変更なし。** 発端は user の質問
  「OSS プロジェクトとして `.claude` などを ignore してないのは問題ないか」。
  **`.claude/` 自体は問題なし** — 他人の名前を含む `.claude/rules/` と
  `.claude/templates/` は既に ignore 済み、tracked な 22 ファイルはスキャン clean。
  **本当の穴はコミットメタデータ**: 全コミットの author/committer が maintainer の個人
  Gmail だった。ファイル内の文字列は次のコミットで直せるが、identity はコミット
  オブジェクトの一部なので直す = 全子孫ハッシュ書き換え + force-push。`git grep` は
  ツリーを読むので、内容ルールでは構造的に検出できない。**対応** = (1) このリポの
  `user.email` を noreply に設定、(2) `pii-scan.sh --identity <range>` を新設 —
  `%ae/%ce/%an/%cn` を読む独立モード、GitHub の noreply 2 形式のみ許可、出力は値を
  伏せる (公開 Actions ログで隠したい当のアドレスを再公開しないため)。pre-commit は
  コミットが生まれる前に `git config user.email` を検査、CI は push/PR が導入した
  範囲のみ (履歴全体は書き換え前で一様に非準拠 = 常時赤になる)。RED-first の selftest で
  「identity モードがパースされるのに dispatch されず clean と report する」バグを発見 →
  `*)` arm で exit 2、(3) **既に公開済の ~428 コミットは直っていない** = 履歴書き換えは
  human 判断。手順は `docs/maintainer/history-sanitize-runbook.md` に `--mailmap` 節を追記。
  **棚卸し (§31)** = 本ファイル 3,689→180 行、`next-actions.md` 475→約 300 行を
  `.claude/archive/` へ**全文退避** (要約でも削除でもない)。
  **`.pii-denylist` を作成 (untracked・gitignored)** = ADR-0055 のブロッキング層が
  この PC で初めて有効化。作った直後に 3 件ヒットし、**「実店舗名を履歴から消すべき理由」
  を説明している当の段落 (`next-actions.md`) が実店舗名を書いていた**ことが判明 →
  除去 (commit `cdf6524`)。エントリは必ず十分長くする — 素の OS ユーザー名のような短い
  文字列は "system" 等に部分一致して全コミットをブロックする。
  **未公開ローカルコミットの identity を書き換え済み** = `git filter-branch --env-filter`
  を `--all --not --remotes=origin` に限定して実行。全ローカルブランチの未公開部分
  28 コミットが個人 Gmail → noreply に。**push 済みでないので force-push 不要・
  誰の clone も壊さない**。書き換え後に「ツリー + 件名 + 順序が全ブランチで一致」を検証、
  `refs/original/*` とバックアップ ref は削除 (Gmail を持つコミットオブジェクトを
  到達可能なまま残さないため。復旧は reflog から可能)。
  **残るのは origin 上の 468 コミット** = force-push を伴うので human 判断。)

---

## 退避日 2026-08-25

- 日付: 2026-08-19 (**検証シート 003 を通すために MCP へ 7 動詞を足した。
  PR 6 本マージ (#182 #183 #184 #185 #186 #187)、open は #180 の 1 本。
  develop = `84d613c`。**

  **やったこと**: (a) MCP に `set_editor_sql` / `run_query` / `open_ai_panel` /
  `open_ai_settings` (#185)、`get_ui_locale` / `set_ui_locale` (#182)、
  `capture_window` (#184) を追加。
  (b) クエリツールバーの反応が履歴のキャッシュで止まる不具合を修正 (#183)。
  (c) シート 003 を user が実施し 10 行すべて `OK` (#186)。
  (d) 三つのシートから `実施日` / `担当` の列を削除 (#187)。
  (e) Issue #181 起票。(f) #180 を `84d613c` に rebase。

  **なぜ MCP を足す話になったか**: 元のタスクは「シート 003 を人が実施する」で、
  言語メニューを開く・切り替える・画面を見る、を user に 1 手ずつ頼む予定だった。
  **人の操作が要ると気づいた時点でその口を MCP に足す**、という取り決めがあるので、
  頼む前に動詞を足した。結果、10 行のうち user が実際に手を動かしたのは
  `結果` 列の記入だけになった。

  **役割分担は崩していない**: 撮った画像を見て豆腐 (□) の有無を判定したのは
  エージェント側だが、`結果` に `OK` を書いたのは user。baseline §22 の
  「実物を動かして目で見た人だけが書く」は、判定を代行しない話ではなく
  **記入を代行しない**話として運用している。

  **003 の結果**: 豆腐は 1 つも出なかった。ハングル・簡体字・繁体字・キリル文字が
  同居する言語メニュー (最も出やすい行) を含め全 10 行 OK。egui 版で 2 回再発した
  不具合が、フォント選択が WebView2 の仕事になった後は再発していないことを、
  現行シェルで初めて人の目で確認した。**「直った」ではなく「別の仕組みに移った」**
  という 003 の前提が、ここでようやく検証された。

  **通す過程で見えた穴 → Issue #181**: 11 ロケール中 9 つが **334 キー中 30 キー
  (9%)** しか訳されていない。切替は動くが中身が英語のまま。シートが全行 OK に
  なったことより、こちらの方が実害が大きい。**シートは「仕様どおり動くか」しか
  見ない** (baseline §36) ことの実例。

  **列の削除 (#187)**: baseline §22 に公開リポ条項が付いた。`実施日` / `担当` を
  埋めると、commit の author と日時が既に持っている情報を**公開リポにもう一部**
  作ることになる。003 が両列を埋めた直後だったので、重複が具体的に見えた。
  001/002 は 10 列 → 8 列、003 は 11 列 → 9 列。`結果` は不変
  (003 の 10 個の `OK` はそのまま)。公開リポ向けの 4 つの規則を `#@ note` として
  各シートと README に入れた — baseline にだけ書くと、グリッドエディタを開いた
  **その瞬間**には見えないため。

  **ディスク**: `target/debug` が 14.2 GB まで育ち空きが 5 GB を切って、
  リンカが `os error 112` で落ちた。`cargo clean --profile dev` で 20440 ファイル /
  14.2 GiB を回収 (pre-push が要るのは release 側だけ)。空き 18.1 GB。
  代償として次の pre-commit は debug のフルリビルドになる。

  **#180 の衝突**: 独立していたはずの #180 が、develop の移動 (#186/#187 が同じ
  ヘッダに触った) で CONFLICTING になった。merge ではなく rebase で解消し、
  v0.8.0 への版数訂正は 001/002 にだけ適用 (003 は develop 側が既に正しい)。
  結果 **003 が差分から消えた**のが正しい形。現在 MERGEABLE。

  **検証**: #187 の commit は `--no-verify`。理由は Windows の libSQL teardown
  segfault (全テスト ok の後にプロセスが `0xc0000005` で落ちる) で、
  **唯一の許容ケース**。hook が飛ぶ分を補うため `pii-scan.sh --staged` と
  `--message` を単独で実行し両方 clean を確認した (fmt / clippy / check は
  hook 内で test に到達する前に通過済)。TSV は awk で構造検証 —
  001 `cols=8 rows=11 未実施=11` / 002 `cols=8 rows=13 未実施=13` /
  003 `cols=9 rows=10 OK=10`、落とした 2 列の残骸なし。
  **動作確認 (アプリを実際に走らせての確認) は 003 の 10 行が該当し、user が実施済**。)
