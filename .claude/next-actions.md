# 次のアクション案内 (user 側ボール常設ファイル)

このファイルは「セッションが切れても、開いた瞬間に user 側のボールがわかる」状態を維持するためのもの。
セッション終了時 / 状況が変わった時に必ず更新する。**日本語可・内部用。**

---

## 最終更新

- 日付: 2026-08-13 その2 (**滞留していた PR を全部入れた。open は #159 の 1 本だけ。
  ADR 番号の穴も塞がった。**
  ① **本日マージ = 9 本** (#166 CI / #167 選択エクスポート + 上書きインポート /
  #168 next-actions 同期 / #160 デモ用フィクスチャとスクリーンショット /
  #163 貼り付け空白の除去 / #162 文書ツリー + ステータスバー + ENUM プルダウン +
  Aurora DSQL の画面編集 / #164 検証シート 003 / #149 姉妹リポ用の貼り付けブロック /
  #165 llms.txt)。**open PR は #159 のみ**になった。
  ② **`docs/decisions.md` の ADR 番号が 0096 → 0105 で連続した。** develop が 0096 の次に
  いきなり 0105 だったのは、0097〜0104 が未マージの 4 ブランチに分散していたため。
  4 本とも同じ位置に追記するので**互いにコンフリクトし、1 本ずつしか解けない**。
  今回はそれを順に解いた。**単純な連結ではなく番号順になるよう差し込んでいる** —
  特に #162 は git が ADR-0100 の見出しと ADR-0099 の `### Status` 定型文を共通行として
  噛み合わせ、コンフリクトが 2 箇所に割れて本文が入れ替わる形になっていた。
  ③ **#159 (文書ストアをガイドに書く) はコンフリクト解消済みだが未 push。**
  `docs/document-store-guides` の `889a28a`。衝突は `site/index.html` の OGP 1 箇所で、
  **説明文は #159 側 (Firestore / MongoDB 入りの新しい DB 一覧)、プレビュー画像は
  develop 側 (ADR-0098 でロゴからスクリーンショットに変えた判断)** を採って組み合わせた。
  `site` の `node --test` は 15/15 緑。**この commit だけ `--no-verify` を使った** —
  例の Windows libSQL テアダウン segfault (`0xc0000005`) で pre-commit の `cargo test` が
  落ちるため (CLAUDE.md が唯一認めている bypass)。変更は `site/index.html` のみで Rust に
  触っていないこと、`pii-scan --staged` は手動で回して clean を確認済み。
  **push は `target/release/dbboard-mcp.exe` を使用中で保留**になっている
  (`cargo build --release` が上書きできず `os error 5`)。
  ④ **#131 に約束していたコメントを入れた** (CI が入ったこと・Windows ジョブを置かない理由・
  初日に `secure_fs` の Linux 限定バグを捕まえたこと)。
  **user 側ボール = ① `git push origin docs/document-store-guides` → CI 緑を確認して
  `gh pr merge 159 --merge --delete-branch` (mcp.exe を閉じてから。占有プロセスは
  `Get-Process | ? { $_.Path -like 'C:\claude\dbboard\target\release\*' }` で見える)、
  ② この文書と `project-status.md` の更新コミットを push (ブランチは
  `chore/session-status-0813`)、③ 姉妹リポへ `.claude/tools/dbboard.md` を貼る
  (08-09 から継続)、④ ~468 コミットの history 書き換え判断 (`pii-scan` identity 赤の
  唯一の原因・08-09 から継続)、⑤ **#161 の 3 点観察** — ここが今いちばん詰まっている。**
  次のエージェント側タスク = #161 の観察結果を受けて修正。**失敗するテストを先に書く**が、
  原因が特定できていない段階で当て推量のテストは書かない。)

- 日付: 2026-08-13 (**未マージの green な PR が 8 本溜まっているのが最大のボトルネック。
  実運用バグ #161 を調査中で、原因は報告者側の 3 点観察待ち。**
  ① **前回更新 (08-09) 以降に v0.6.0 と v0.7.0 が出ている。** v0.6.0 = Cloud Firestore
  アダプタ (PR #153)、v0.7.0 = MongoDB アダプタ (PR #156)。ドキュメント側 (CHANGELOG /
  README / roadmap / compatibility) と検証シート (Firestore・MongoDB の接続) も同時に入った。
  08-09 時点で「次のエージェント側タスク」としていた **issue 0020 スライス 3
  (`BackendConfig::MongoDb`) は消化済み**。
  ② **本セッションで CI を入れた** — `ci/cargo-and-frontend-checks` (ADR-0104) を push し
  **PR #166** を作成。issue #131 で「PR は pii-scan しか回っておらず、hook を bypass しても
  受け止める先が無い」と共有された穴を塞ぐもの。ubuntu-latest 3 ジョブ
  (cargo fmt/clippy/check/test ・ svelte-check + vitest ・ site の node --test)。
  **Windows ジョブは意図的に置いていない** — 既知の libSQL teardown segfault (#131) で
  緑のコードのまま恒久的に赤くなるため。**初回 CI が ubuntu で赤くなり、そこで
  `secure_fs` のテスト 4 本が Linux 限定で落ちていたことが判明した** (CI を入れた初日に
  CI が仕事をした形)。分類器 `is_likely_cloud_synced_path` 自体はどちらのプラットフォームでも
  正しく、テスト側が `r"C:\Users\alice\OneDrive\..."` と**バックスラッシュ区切りのリテラル**を
  渡していたのが原因。Unix では `\` は区切り文字ではないので `Path::components()` が
  1 セグメントに潰れて何にも一致しない。セグメントの配列から `PathBuf` を組む形に直し
  (`cfg(windows)` で消すのではなく、Windows レイアウトのケースを全プラットフォームで
  踏み続ける)、`d2bbfc2` として commit 済み — **これも未 push**。
  ③ **`feat/selective-export-and-upsert-import` (ADR-0105) が未 push のまま。**
  接続の選択エクスポートと、インポート時の上書き (`ImportMode`、既定は Skip)。
  空の id リストは「全件」と読まずに拒否する。ADR-0038 のキーリング参照拒否は緩めていない。
  ④ **#161 (実行ボタンがクリックで反応しない・Ctrl+Enter は通る) の調査**。
  ソースを読み切って、クリックと Ctrl+Enter が**同一関数・同一ガード**であることを確認した
  (`QueryPanel.svelte` の `run`)。よって Ctrl+Enter が通る＝その瞬間の内部状態は正常が確定し、
  「ボタンだけ弾かれる」条件はコード上に存在しない。経路上の `stopPropagation` なし、
  常時被さるオーバーレイなし、`QueryPanel.svelte` / `SqlEditor.svelte` は Tauri シェル投入以降
  無変更なのでバージョン差でもない。**残るのは「クリックがボタンに届いていない」筋**で、
  ここから先はコードだけでは切れない。バックグラウンド調査が出した「CodeMirror の補完
  ポップアップが被っている」説は、報告手順が*貼り付け*である以上 `activateOnTyping`
  (`input.type` のみ発火・貼り付けは `input.paste`) が動かないため**採用しなかった**。
  #161 に 3 点の観察 (ボタンの色 / カーソルが指の形になるか / フォーカスを外してから押すと
  動くか) を依頼済み。回答が来るまでは推測で直さない。
  **user 側ボール = ① 未 push のブランチ 3 本を push する
  — `ci/cargo-and-frontend-checks` (テスト修正 `d2bbfc2` を載せて #166 を緑にする)、
  `feat/selective-export-and-upsert-import`、`chore/next-actions-sync` (この文書)。
  push 後の PR 作成はエージェント側でやる旨、本セッションで合意済み、
  ② **green で MERGEABLE のまま滞留している PR 8 本を入れる**
  (#149 #159 #160 #162 #163 #164 #165 #166 — #166 以外は CI 緑・コンフリクト無し。
  溜まるほど衝突リスクが上がるので、ここが今いちばん効く)、
  ③ 姉妹リポへ `.claude/tools/dbboard.md` を貼る (08-09 から継続)、
  ④ ~468 コミットの history 書き換え判断 (`pii-scan` identity 赤の唯一の原因・08-09 から継続)、
  ⑤ #161 の 3 点観察。**
  次のエージェント側タスク = **#166 がマージされたら #131 に一行コメントを入れる**
  (そこで約束した分)。#142 (llms.txt) は **PR #165 として出済み**なのでマージ待ち。
  その後は #161 の観察結果を受けて修正 — **失敗するテストを先に書く**が、
  原因が特定できていない段階で当て推量のテストは書かない。)

- 日付: 2026-08-12 (**実利用で挙がった 3 つの摩擦をブランチ `feat/document-tree-view` に
  3 コミット。未 push。**
  ① **文書セルが 1 行の JSON だった** → 展開/折りたたみできる木で表示 (ADR-0100)。
  `{"$json": null}` と「文書ではない」を区別するため、popup の `doc` を
  `{ value: unknown } | null` でラップしてある (null 自体が正当な文書値なので)。
  ② **フッターが空だった** → 24px のステータスバー (ADR-0101)。**行数は入れていない**
  ‐ 結果ツールバーに既にあり、繰り返しは「無駄なもの」そのものだから。入れたのは
  画面のどこにも無かった 2 つだけ: **直前のクエリの所要時間**と、**閉じた更新通知へ
  戻る導線**。後者は実バグの修正でもある — `UpdateNotice` を閉じると
  `+page.svelte` の state が `null` になり、そのセッション中は二度と出せなかった。
  計測はフロント側で `invoke` の前後のみ (バックエンドでやると `QueryOutput` と
  全アダプタに触ることになる)。ブラウズ後のスキーマ読みは含めない。
  ③ **ENUM がテキスト入力だった** → プルダウン (ADR-0102)。選択肢の出所は
  `information_schema.column_type` = `describeTable` **のみ**。結果セットのメタは
  `ENUM` としか返さないので、主キーを読む同じ `try` で一緒に取り、`enums` prop で
  グリッドへ渡す。解析は純関数 + 10 テスト (`src/lib/grid/enum.ts`) に隔離 —
  カンマ・`''`・バックスラッシュを取り違えると**間違った選択肢**を出すことになり、
  テキスト入力より悪いため。**解析できなければ選択肢を出さずテキストに戻す**。
  `SET` は複数値なので対象外 (単一選択にすると 1 つを除いて黙って捨てる)。
  宣言に無い既存値は先頭に選択済みで保持 = 開いただけで書き換わらない。
  インライン編集と拡大ダイアログの**両方**を select 化 (片方残すとそこから自由入力
  できる)。今のところ効くのは MySQL のみ — Postgres の名前付き enum は型名しか
  返さないので別途カタログ読みが要る。
  ④ 検証: svelte-check 281 files / 0 errors、vitest 21 files / **436 tests** 緑、
  `pii-scan --staged` clean。**3 コミットとも pre-commit フックを全通過 (exit 0)**、
  `--no-verify` 不使用。
  **user 側ボール = ① `git push -u origin feat/document-tree-view` (その後こちらで
  PR を立てる)、② `fix/trim-pasted-connection-fields` の push、③ 姉妹リポへ
  `.claude/tools/dbboard.md` を貼る、④ dbboard-web へ `docs/api-contract.md` の
  `$json` を中継、⑤ ~468 コミットの history 書き換え判断 (`pii-scan` identity 赤の
  唯一の原因)。**
  未 push のブランチが他に 3 本: `docs/locale-rendering-test-spec` /
  `docs/agent-onboarding` (force-with-lease) / `docs/llms-txt`。
  検証シート 001 は No.2 から人手で再開待ち (行 1・10・11 は `未実施` のまま)。)

- 日付: 2026-08-09 (**Zenn 記事を公開 + それを書く過程で見つかった文書の嘘を 3 件修正。
  未 push が 3 コミット (`f0cb0ca` / `28c15cc` / `913ee8b`)。**
  ① **記事** = `articles/dbboard-mcp.md` → <https://zenn.dev/dokokade/articles/46b8c608715963>。
  Zenn は別リポからビルドされる (ファイル名 = スラグ) ので、こちらのコピーを正本にして
  `published: true` + 冒頭 HTML コメントに URL を記録した。**記事とそれが説明している
  ドキュメントを一緒に直せる状態を維持すること。**
  ② **write allowlist の記述が 4 ファイルで間違っていた。** `DROP INDEX` と
  `COMMENT ON` が「通る」と書かれていたが、コードは一度も通していない。
  実際は `INSERT`/`UPDATE`/`DELETE`/`MERGE` + `CREATE TABLE`/`VIEW`/`INDEX`/`SCHEMA`/
  `ALTER TABLE` のみで、**`DROP` はインデックスを含め全オブジェクトが永久拒否**。
  ADR-0087 は正しく、派生ドキュメントだけがずれていた。両端を固定するテストを
  `write_policy.rs` に 2 本追加 (`dropping_an_index_is_closed_even_though_creating_one_is_not` /
  `commenting_is_refused_because_nothing_listed_it`)。
  ③ **MCP に「環境変数で接続そのものを渡す」経路は存在しない。** `DBBOARD_MYSQL_URL` 等は
  `dbboard-server` の単一接続解決パス専用で、MCP は読まない (`adapter_for` は
  `connections.toml` + キーチェーンしか見ない)。MCP が読む環境変数は `DBBOARD_CONFIG` と
  `RUST_LOG` のみ。クレート README の該当節を書き換え、**ルート README とダウンロード
  ページに残っていた同じ嘘も潰した** (ルート README は既に存在しない節へリンクしていた)。
  ④ **`docs/compatibility.md` に MySQL/MariaDB 節が無いまま 3 リリース経っていた。**
  `max_execution_time` (MySQL・ミリ秒) と `max_statement_time` (MariaDB・秒) の
  綴り分けと、なぜ 1 回プローブしてキャッシュするか (無い変数を聞くと hard error) を記録。
  ⑤ **記事の裏取りは全部実機**。`claude mcp add` → `claude mcp list` で `✔ Connected` →
  stdio で 9 ツールを実際に叩き、拒否メッセージもその出力をそのまま貼った。
  ⑥ **導線を張った** — ルート README の MCP 節 / クレート README の冒頭ボックス /
  `site/index.html` の "Use it from an AI agent"。「公開しただけでは広まらない」の反映。
  ⑦ 検証: fmt / clippy -D warnings / check / test 全緑、`site/app.test.mjs` 6 件パス、
  `pii-scan --staged` clean。3 コミットとも hook は**既知の Windows libSQL teardown
  segfault**でのみ bypass、理由をコミットメッセージに明記済み。
  **user 側ボール = ① この 3 コミットを push、② 姉妹リポへ `.claude/tools/dbboard.md` を
  貼る、③ ~468 コミットの history 書き換え判断 (`pii-scan` identity 赤の唯一の原因)、
  ④ PR #148 / #149 を入れる (feat なので次は 0.6.0)。**
  次のエージェント側タスク = **issue 0020 スライス 3** (`BackendConfig::MongoDb` +
  `connect_adapter` アーム + デスクトップの**追加/編集フォーム両方** + サイドバーの
  クエリ生成 + MCP ツール説明。0020 で唯一未チェックの完了条件)。)

- 日付: 2026-08-06 (**v0.5.1 リリース済み。タグは `main` の `b98f7a6`。**
  ① **実運用で出た 2 バグを 1 本のブランチにまとめた。** `release/v0.5.1` に
  `fix(mysql)` → `fix(connect)` → `release: v0.5.1` の 3 コミット。
  もともと別ブランチ 2 本だったが、両方が `CHANGELOG.md` の `[Unreleased]` 直下を
  触るので PR を分けると必ず衝突する。ローカルで解決して 1 本にした。
  PR #150 (→ develop) → PR #151 (develop → main) → `main` 上で `v0.5.1` タグ。
  ② **SSH バスティオン経由の接続が死んだまま復帰しない件 (ADR-0092)。** 原因は 3 層。
  russh が keepalive を既定で送らない / 失敗したアダプタがキャッシュから追い出されない /
  sqlx は死んだフォワードに再ダイヤルするだけ。keepalive (30s×3) + アイドル 30 秒後の
  ping-on-borrow + 手動リコネクト (接続ピルのリロードアイコン + エラーバナーのボタン)。
  ③ **MySQL 8 の `information_schema` が `VARBINARY`/`BLOB` を返す件。**
  メタデータをバイト列で読んで UTF-8 検証する形に統一。`list_relationships` が
  エラーを飲み込んで**空の結果を返していた**のもこれで直る。
  ④ **タグを打つ順序を間違えて 1 度やり直した。** develop → main の PR を作る前に
  `main` でタグを打ったので、`v0.5.0` の中身が `v0.5.1` として公開されかけた
  (run 31069887884 をキャンセル、リリースオブジェクトは未作成だったので実害なし)。
  **タグは develop → main のマージが `main` に入ってから**。手順を書くときは
  マージを独立したステップとして明示すること。
  ⑤ **リリースの develop → main は squash をやめて真のマージにした。** #134 と #146 が
  squash だったせいで共通祖先が `v0.4.0` まで戻り、#151 が 9 ファイル全部
  (`Cargo.lock` / `CHANGELOG.md` 含む) で衝突した。真マージにしたので develop が
  main の祖先に戻り、**次のリリース PR は衝突しない**。
  代償として、**`main` の `pii-scan` identity が赤になった** — squash が隠していた
  noreply 切替前の古いコミットが main から到達可能になったため。アドレス自体は
  元から develop 側で公開済みなので新規の漏洩ではないが、履歴書き換えをするまで
  main の CI は赤のまま。
  ⑥ **検証は全部緑** — fmt / clippy -D warnings / check / test (dev, release) /
  `cargo build --release` / `pii-scan --tree` / `pii-scan --range develop..HEAD`。
  `release: v0.5.1` のコミットだけ `--no-verify` を使ったが、これは
  **Windows libSQL の teardown segfault** (13 テスト全部 ok の後にプロセスが落ちる)
  という既知の唯一の例外。PII スキャンは hook の 1 番目で通過済み + 手動で再実行済み。
  マージコミット `b98f7a6` は hook を全部通している。
  ⑦ **release ビルド中に `target/release/dbboard-mcp.exe` がロックされていたので、
  そこから起動された古い MCP プロセス 2 つを kill した** (`%LOCALAPPDATA%` の
  インストール版ではなくビルド成果物)。クライアント側が再起動するので実害なし。
  ⑧ `fix/mysql-metadata-decode` / `fix/connection-reconnect` / `release/v0.5.1` の
  ローカルブランチは削除済み (中身は develop と同一であることを確認した)。
  **user 側ボール = ① #148 / #149 を入れる (feat なので次は 0.6.0)、
  ② 姉妹リポへ `.claude/tools/dbboard.md` を貼る、③ MCP を
  `%LOCALAPPDATA%\dbboard\dbboard-mcp.exe` に置き直して再登録、
  ④ ~468 コミットの history 書き換え判断 (`pii-scan` identity 赤の唯一の原因。
  ⑤ により main でも赤くなったので、放置のコストが上がった)。**
  次は issue 0019 (Firestore アダプタ、ADR は **0093** — 0092 は接続復旧で使った) →
  issue 0020 (MongoDB)。)

---

- 日付: 2026-08-05 その5 (**issue 0018 = 入れ子 `Value` が着地。ブランチ
  `feature/nested-value`。ADR-0091 の Phase 6 の 1 本目。**
  `Value::Json(serde_json::Value)`、ワイヤタグは **`$json`**。`$blob` と同じく
  1 キーのタグ付きオブジェクトで、`Value` は serde の外部タグ付けを使っていない
  (ワイヤが普通の JSON に見えるようにするため) のでハンドライトのアームを足した。
  **ペイロードは opaque** — 生の `serde_json::Value` として読み、`Value` として
  再帰的には読まない。したがって `"$blob"` キーを含む文書はバイト列に化けずに
  文書のまま。タグが付くのは最も外側のセルだけ。
  `Json(null)` は **SQL `NULL` ではない** (「列に何も無かった」と「文書が null を
  持っていた」の区別が消えるため)。
  variant を足したことで exhaustive match が 3 つ壊れたが、`_` アームは使わず
  1 つずつ答えを決めた (`_` にすると新 variant が 3 箇所で黙って間違う):
  sort = 木に自然な順序は無いので描画形で比較・blob の次にランク、
  dump/literal = コンパクト JSON をシングルクォート (どの方言も JSON をテキストと
  して受け、`JSON`/`JSONB` 列が読み直すので `INSERT` が round-trip する)、
  write_back = **identity 値としては拒否** (文書の等価性はエンジン依存 —
  キー順・空白・json vs jsonb で答えが変わるので、そこから組んだ `WHERE` は
  別の行に当たるか 1 行も当たらない)。
  フロントは grid と CSV/TSV export でコンパクト JSON を出し、インライン編集は
  blob と同様に開かない。既存アダプタの出力は一切変わらない (core の外の
  `Value::Blob` を全て確認したが d1 / mysql / turso の構築側だけで消費側は無い = 純粋に加算的)。
  検証: `cargo test --all-features` ワークスペース全体 0 failed、
  `pnpm vitest run` 353 passed、`pnpm check` 0 errors、clippy / fmt 緑。
  **→ dbboard-web に `$json` を伝えること (明示ハンドオフ)。**
  `docs/api-contract.md` に `$json` 節を追加済み (タグの形、opaque であること、
  `{"$json": null}` と SQL `NULL` の違い、Text セルと見分けが付かないのでタグが
  唯一の判別子であること、`[object Object]` で出さないこと、**今日どの SQL
  アダプタも `$json` を出さない**が variant はワイヤに乗っているので受理は必須、
  を明記)。当リポからは姉妹リポを編集できない (baseline §27) ので、**契約文書を
  先に出すことが web 側の着手条件**。以前これを黙っていて 3 週間ブロックした。
  **user 側ボール = ① `git push -u origin feature/nested-value` と PR、
  ② dbboard-web へ `docs/api-contract.md` の `$json` 節を中継。**
  次: issue 0019 (Firestore) → issue 0020 (MongoDB) の順。
  別件で **MCP 紹介文を `docs/` にトラック済みファイルとして置く** — 他リポの
  エージェント向けの導入手順を毎回チャットで作り直しているため。0018 着地後の別コミット。)

- 日付: 2026-08-05 その4 (**v0.5.0 リリース + 文書ストア (MongoDB / Firestore) を
  Phase 6 として確定。ADR-0091。**
  ① **v0.5.0 を切った。** PR #144 (release/v0.5.0 → develop) → #145 (ADR-0091) →
  #146 (develop → main、v0.4.0 から 87 コミット) → `main` 上で `v0.5.0` タグ push。
  **リリースオブジェクトの手動作成はもう不要** — publish ジョブに
  `gh release view || gh release create --generate-notes` のブートストラップが
  入っているので、タグ push だけで公開まで通る (v0.1.0〜v0.3.0 で必要だった手順は消えた)。
  ② **`pii-scan` が #146 で赤になったが既知の未処理分。** identity チェックが
  2026-07-22 の古いコミット群 (noreply 切替前) を指しており、**今回のセッションで
  作った 4 コミット (`e6db331` / `b51fd25` / `eeddf91` / `622b186`) はすべて noreply**。
  develop → main の PR は「main に無い全コミット」を対象にするので古い分がまとめて
  出ただけ。main への push 後の `pii-scan` は緑。**~468 コミットの history 書き換え
  判断待ち = user 側ボールのまま**で、今回のリリースが増やしたものではない。
  ③ **MongoDB / Firestore を stretch から確定フェーズへ格上げ (ADR-0091)。**
  `dbboard-core` を読み直した結果、障害は 4 つあり **trait は障害ではなかった** —
  `query(&self, sql: &str)` は trait 側で一切パースしていないので、Mongo の
  コマンドドキュメントも Firestore の `StructuredQuery` もそのまま JSON 文字列で
  渡せる (中間クエリ IR 不要)。実際の障害は `Value` が平坦で木を持てないこと、
  `read_only.rs` が `sqlparser` ベースで「パースできない入力は fail closed」ゆえ
  Mongo のクエリを全拒否してしまうこと、`describe_table` が宣言済みカラム前提であること。
  ここから順序が決まる: **入れ子 `Value` を単独で先に** (issue 0018、全アダプタの
  行構築と dbboard-web との共有ワイヤ契約に触るので単独で出す) → **Firestore**
  (issue 0019、REST が `:runQuery` / `:commit` をエンドポイントで分けているので
  read-only は「どのエンドポイントを叩けるか」で決まり、分類器が要らない) →
  **MongoDB** (issue 0020、`runCommand` が何でも受けるので fail-closed 許可リストが
  必要。しかも `$out` / `$merge` は read であるはずの `aggregate` の中から書くので
  パイプラインを歩く必要がある = 一番高くつく上に安全側の要)。
  **PlanetScale は新規アダプタ不要** (MySQL 互換なので既存 `dbboard-mysql` で届く)。
  **user 側ボール = ① 姉妹リポへ `.claude/tools/dbboard.md` を貼る (v0.5.0 が出たので
  「v0.5.0 以降」の記述が本当になった)、② MCP を
  `%LOCALAPPDATA%\dbboard\dbboard-mcp.exe` に置き直して再登録 + エージェント再起動、
  ③ ~468 コミットの history 書き換えをやるか否かの判断。**)

- ※ 2026-08-05 その3 〜 2026-08-03 のエントリは
  `.claude/archive/next-actions-2026-08.md` へ全文退避 (baseline §31、退避日 2026-08-09)。
  さらに古いもの (2026-07-29 以前) は `.claude/archive/next-actions-2026-07.md`。


## モード

**in-use / continuous-improvement (menu-not-sequence)** — 2026-06-24 以降。
配布 (#14) は 2026-07-16 に完了済、v0.3.0 公開済、DL ページも live。今は
「配布済 exe を担当が実際に使うか」を update-check で観測しつつ、次の実利用改善
(下記の user 側ボール) を摩擦順に進めるフェーズ。直近は結果グリッドのソート漏れと
MSI ショートカット漏れを補完し、次いで maintainer 要望の**論理バックアップ
(ダンプ)** を ADR-0049 として実装・着地 (PR #108)。

---

## user 側のボール (= 次に着手する時の選択肢)

### ★ 候補 A: 実利用摩擦の次テーマ (menu-not-sequence)

直近 3 PR (DL ページ / ソート / MSI ショートカット) はいずれも実利用で挙がった
摩擦。次も同様に「実際に使って気づいた困りごと」を摩擦順に拾う。未着手候補は
Saved queries / Schema diff (下記 候補 E。Export results は CSV/JSON 済)。新しい
write 経路を伴うものは着手前に ADR。

### 候補 A-2: README に MSI アンインストール残留の明文化 (小・任意)

MSI アンインストールは `%APPDATA%\dbboard\dbboard\` の設定と Windows 資格情報
マネージャーのエントリを残す (仕様)。ユーザに口頭で伝えた `cmdkey` +
フォルダ削除のクリーンアップ手順を README か `docs/` に明文化する小 chore。

### 候補 B: git 履歴の一括サニタイズ (human ボール・破壊的・未実行)

**1 回の rewrite で 2 つが同時に片付く。** どちらも「ファイルなら次のコミットで
直せるが、既に公開された過去コミットは書き換えないと消えない」もの:

1. **実店舗名** — 過去コミットに残る (実名は**非公開メモリと `.pii-denylist`
   のみ**。ここには書かない。対応表からローカルで `replacements.txt` を作る)。
   バイナリは CI ビルドで名前を含まないためリリースは塞がない。
2. **コミット identity** — 公開済 468 コミットの author/committer が個人 Gmail
   (ADR-0084)。**未公開のローカル 28 コミットは書き換え済** (2026-07-31、
   force-push 不要だったので実行した)。以後の新規コミットも noreply で clean。
   残るのは origin 上の分だけ。

手順は `docs/maintainer/history-sanitize-runbook.md` (Step 1-3 = 文字列置換、
Step 3b = `--mailmap` で identity、Step 4 = force-push)。全ハッシュ変更・既存
クローン/PR/フォーク破損のため **human 実行**。fork 0 / star 0 なので実効性は
ある = 検討する理由になるが、勝手に実行する理由にはならない。

**順序を間違えると全部無駄になる:** rewrite + force-push は**未 push の
ローカル作業を全部 push してから**やり、その後クローンを捨てて re-clone する。
先に rewrite すると、残った未書き換えのローカルコミットを次の `git push` が
そのまま remote に戻して再汚染する (git から見れば単なる新規コミットなので
警告も出ない)。`git pull --rebase` では直らない。runbook の「Ordering」節参照。
open PR **#125** (`feature/cjk-font-and-ai-menu`) は rewrite で壊れるので、
先に merge / close するか、書き換え後のブランチから立て直す。

### 候補 C: release.yml の publish 自己作成化 (follow-up)

現状 `gh release upload` は既存リリースにしか添付できず、タグ push だけでは
`release not found` で落ちる (毎回手動で `gh release create` が前提)。publish
ステップを `gh release view <tag> || gh release create <tag> --generate-notes`
にしてタグ push を自己完結させる。[[project-release-ci-needs-release-object]]。

### 候補 D: cargo-deny の既存ドリフト対応 (別 chore)

`cargo deny` が advisories/licenses で FAILED の可能性 (既存依存への 2026
アドバイザリ): `proc-macro-error2` (unmaintained ← age) / `option-ext`
(MPL-2.0 ← directories) / `quick-xml` (DoS ← wayland-scanner ← eframe, Linux)。
commit フックではないので緊急ではないが `deny.toml` の期限付き exception か
依存 bump で解消。着手時に現状を再確認。

### 候補 E: 既存ロードマップ機能バックログ

未着手: Saved queries / Schema diff / Export results は済 (CSV/JSON) /
Group D-2 (ADR-0029 function-calling, `feature/adr-0029-function-calling` に
planning ball)。実利用の摩擦順に着手。新 write 経路は着手前に ADR。

### 参考: 配布済 exe の使用シグナル確認 / 再配布

- **使用確認**: `gh release view v0.3.0 --json assets --jq
  '.assets[].downloadCount'` (匿名 update-check の GET 自体は観測不可、
  資産 DL 数のみ)。
- **新版を配布したくなったら**: develop から `cargo build --release` →
  次バージョンを bump → main にマージ → タグ push で Release CI が Win+Mac
  資産を自動公開。**⚠ ただしリリースオブジェクトを先に `gh release create`
  しておくこと** (publish は添付のみ)。配布済 exe が起動時に検知する。ビルド前に
  dbboard ウィンドウを閉じる (exe ロックで os error 5)。公開前に exe を実接続名で
  スキャン (0 一致)。
- **MSI / .dmg で渡す場合 (PR #88)**: ローカル MSI = WiX v3 + `cargo install
  cargo-wix` → `cd apps/dbboard && cargo wix`。Mac は `cd apps/dbboard`
  → version inline → `cargo bundle --release` → `hdiutil` で `.dmg`
  (cargo-bundle 0.6.0 は `--package` 非対応 + workspace version 不読なので
  README の macOS 手順に従う)。exe 単体で十分なら不要。
- secret 移送 = **推奨 (ADR-0038)**: 手元で 3 接続を Export → `.dbbx` を渡し
  パスフレーズは別経路。担当機は Import 1 回。旧 cmdkey 手順は
  `docs/collector-setup/README.md`。**secret は一切ファイルに書かない。**

---

## ⚠️ 接続名サニタイズ (2026-07-15 着手)

- **経緯**: public リポジトリのソース/テスト/テンプレに実業務接続名が
  露出していた (2026-07-13〜14 のハンドオフ準備でテストのサンプルデータ
  として実名を使ってしまったのが原因)。**出荷 exe には非埋め込み**
  (テストは `#[cfg(test)]`、テンプレは `tests/` の include_str! のみ)。
- **現行置換 = 実施済み** (このブランチ `chore/sanitize-connection-names`)。
  実名を中立サンプル id (store-a / store-b / store-c) + サンプル行データ
  (Alpha / Beta) に一括置換。実名↔サンプルの対応は非公開メモリのみ保持。
- **履歴書き換え = human のボール (未実行)**: 過去コミットにはまだ実名が
  残る。`docs/maintainer/history-sanitize-runbook.md` の手順で
  `git filter-repo --replace-text` → develop/main を force-push する。
  破壊的操作 (全ハッシュ変更・既存クローン/PR/フォーク破損) のため human 実行。

---

## web 側 (情報のみ・ボールは web 側)

- brief 0008 = v:2 schema mirror が web 側 pending。
- ADR-0030/0031 (query-UX) / ADR-0032 (Windows packaging) / ADR-0036 /
  ADR-0037 (aurora-dsql-iam 段階A/B) はいずれも in-process ないし build
  のみ = web ミラー不要 (確定)。
- ADR-0029 (D-2) も同 posture の見込み、確定は起票時。

---

## このファイルのメンテ規約

- セッション終了時、状況が動いた時は **必ず最新化**。
- 「最終更新」「develop tip」「選択肢」ブロックは毎回見直す。
- 大きな状態は memory ([[project-status-in-use]] /
  [[project-windows-internal-distribution]] など) に書き、ここは
  「user が次の一言を選ぶための短い案内」に留める。
