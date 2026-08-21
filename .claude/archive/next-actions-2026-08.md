# アーカイブ — next-actions.md セッションログ (2026-08-03 〜 2026-08-19)

baseline §31 に基づく退避。いずれも全文で、要約していない。

- **退避日 2026-08-09**: `.claude/next-actions.md` が 415 行になり 400 行トリガを
  踏んだため、v0.5.0 リリース周辺 (2026-08-03 〜 2026-08-05 その3) までを退避。
- **退避日 2026-08-14**: 同じく 435 行でトリガを踏んだため、2026-08-05 その4 〜
  2026-08-06 (v0.5.1 リリース周辺) を追加で退避。
- **退避日 2026-08-16**: 同じく 402 行でトリガを踏んだため、2026-08-09 〜
  2026-08-13 その2 (Zenn 記事公開 〜 滞留 PR 一掃) を追加で退避。
  ファイル中ほどの 2 つ目の `---` 以降がそれ。
- **退避日 2026-08-20**: 同じく 425 行でトリガを踏んだため、2026-08-14 〜
  2026-08-19 その2 (v0.8.0 リリース 〜 v1.0 ゲート確定 〜 v0.9.0 リリース) を
  追加で退避。ファイル末尾の 3 つ目の `---` 以降がそれ。**この退避で
  `next-actions.md` に残る日付エントリは 2026-08-20 (v0.10.0) の 1 本だけになった。**

これより古いものは `.claude/archive/next-actions-2026-07.md`。

---

- 日付: 2026-08-05 その3 (**push が 3 回落ちたが、どちらもコードではなくマシン側の枯渇。**
  ① メモリ: age の scrypt (log_n=18, r=8) が 1 回の暗号化/復号で 256 MiB を確保し、
  20 コア並列のテストハーネスが同時に何 GB も要求 → アロケート失敗 →
  Windows が `STATUS_STACK_BUFFER_OVERRUN (0xc0000409)` と表示するのでメモリ安全性の
  バグに見えるが違う。**空きメモリ次第で成功したり落ちたりする**のが最悪なので、
  `crates/dbboard-config/src/bundle.rs` に `#[cfg(test)]` 限定の KDF mutex を入れた
  (本番はロックしない = export/import は人が 1 回ずつ行う操作)。コミット `e6db331`。
  ② ディスク: `target/debug` が **71.6 GB** に膨らみ C: の空きが 225 MB。
  `link.exe exit code 1318` / `os error 112` として出るので **リンカのバグに見える**。
  `cargo clean --profile dev` で解放 (pre-push が使う `release/` は残す)。
  検証: fmt / clippy 緑、`cargo test --all-features` は **943 passed / 0 failed**、
  既知の Windows libSQL teardown segfault のみ (= 唯一許可された `--no-verify` ケース。
  PII スキャンは staged + message の両方を手で実行して clean 確認済み)。
  **user 側ボール = `git push -u origin chore/retire-egui`。**)

- 日付: 2026-08-05 その2 (**`dbboard-mcp` に配布経路が無かった件を潰した。ADR-0090。**
  user 経由で「使いたいのに使えない AI エージェント」の意見が届いた。原因は文書の
  書き方ではなく **バイナリが一度も配布されていなかったこと** — `tauri.conf.json` にも
  `release.yml` にも `dbboard-mcp` が無く、`cargo build` が唯一の入手手段だったので、
  README の `claude mcp add dbboard -- /absolute/path/to/dbboard-mcp` は Rust
  ツールチェーンの無いマシンでは存在し得ないファイルを指していた。対応: release CI に
  `build-mcp-windows` / `build-mcp-macos` (lipo universal) を追加し
  `dbboard-mcp-windows-x86_64.exe` / `dbboard-mcp-macos-universal` + checksum を publish、
  DL ページには出さない (`bucketFor` は製品名接頭辞判定なので `.exe` でも null。
  `site/app.test.mjs` にテスト追加 = 6 tests 全緑)、README / クレート README /
  site の 3 箇所に OS 別の配置先 + コピペ可能な `claude mcp add` 1 行、
  認証情報を `DBBOARD_*` 環境変数で渡す方法、TLS 終端プロキシ下では
  `--use-system-ca` 相当のフラグが**無い** (OS トラストストアが唯一のモード = ADR-0034) と
  明記。コミット `c015b17`。
  **user 側ボール = 姉妹リポ用の `.claude/tools/dbboard.md` の中身をこちらで用意済み
  (貼り付けるのは user。当リポからは編集できない = baseline §27)。**)

- 日付: 2026-08-05 (**issue #139 = egui クライアントの退役。ADR-0089。**
  `crates/dbboard-ui` / `apps/dbboard` / `crates/dbboard-i18n` を削除し、Tauri 2 +
  SvelteKit が唯一のクライアントになった。リリース CI の `build-windows` /
  `build-macos` (cargo 版) を撤去 — v0.4.0 までは egui 資産が付いたままだが、以降は
  付かない。DL ページの `bucketFor` は拡張子ではなく `dbboard-desktop` 製品名接頭辞で
  判定 (#135 を supersede。v0.4.0 が両クライアントの資産を持つため、拡張子だけだと
  Releases API の並び順で結果が変わっていた)。**`crates/dbboard-server` は意図的に残す**
  — dbboard-web がミラーする HTTP 契約 (`docs/api-contract.md`) の実行可能な仕様書で、
  削除はアーキテクチャ決定 (baseline §16) = #139 のスコープ外。理由を module doc /
  api-contract / architecture の 3 箇所に明記した。トップレベル文書 (README, CLAUDE.md,
  DESIGN.md, docs/architecture.md, api-contract.md, compatibility.md, roadmap.md) から
  現在形の egui 記述を一掃、`docs/desktop-parity.md` は archived バナー付きで凍結。
  **web 側ミラー不要 (明示的 no-op)** — 共有契約は一切変わっていない。
  **同セッションで user から「公開しただけでは広まらない」指摘。** 実際、web 検索を
  持つ別エージェントが dbboard を「一般公開されているツールではない」と判断していた。
  原因を潰した: リポジトリの homepageUrl (空だった) を DL ページに設定、topics 15 個を
  追加 (0 個だった)、README 先頭に DL ブロック + バッジ、release CI の `--notes` で
  全リリースページ冒頭に DL リンク、site に canonical/og/robots.txt/sitemap.xml、
  CLAUDE.md・dbboard-mcp/README・apps/desktop/README に URL 明記。
  **user 側ボール = (1) この PR (`chore/retire-egui`) の push とマージ、
  (2) 姉妹リポの `browser-verification.md` に dbboard の URL と `claude mcp add` 行が
  無い件 — 当リポからは編集できない (baseline §27) ので user から中継が必要、
  (3) MCP write を v0.5.0 に入れるかの判断、(4) 公開済 468 コミットの履歴書き換えの判断。**)

- 日付: 2026-08-04 その3 (**v0.4.0 をリリースした。** 経緯: バージョンは 2 週間前に
  0.4.0 へ上がっていたが `CHANGELOG.md` の `## [Unreleased]` が空で、タグを打つ根拠が
  無いまま放置されていた。0.4.0 節を書き起こし (`0359da6`)、PR #133 で main へ、
  タグ `v0.4.0` を push。**1 回目のタグビルドは Tauri 2 ジョブが両方落ちた** —
  `Install frontend deps` で `ERR_UNKNOWN_BUILTIN_MODULE: node:sqlite`。
  `release.yml` が Node 20 固定なのに `apps/desktop` は `pnpm@11.1.1` を pin しており、
  pnpm 11 は `node:sqlite` (Node 22.5 以降) を import する。**ローカルの Node が
  v22.22.2 なので手元では一切再現しない類の失敗。** cargo だけの
  `build-windows` / `build-macos` は成功、`publish` は skip = **何も publish されて
  いなかったのでタグを移動して復旧できた**。修正 = 両 `setup-node` を `node-version: 22`
  (PR #134、`4a3364e`)。`v0.4.0` を削除して main 先端で張り直し
  (`1dad53e`)、**run 30888094754 は全緑で publish 完了** — v0.4.0 に 10 資産
  (egui 版 exe/dmg/msi + Tauri 版 setup.exe/dmg/app.tar.gz + updater 署名 +
  `latest.json` + `SHA256SUMS.txt`) が付いた。
  **user から常設の方針: リリースは良い区切りで頻繁に切ること。**
  → エージェント側の約束: **feat PR ごとに `## [Unreleased]` に 1 行足す**
  (今回の「タグを打とうにも変更履歴が無い」を再発させない)。
  **MCP は読み取り専用では使い物にならない、と user 指摘。** 現状 7 tool は全て read。
  `service.rs` には未公開の write プリミティブ (`apply_row_update` / `plan_dump` /
  `run_dump` / `plan_restore` / `run_restore`) がある。**`apply_row_update` の公開は
  妥当、接続 CRUD は開けてはいけない** (agent が接続定義と keychain ref を書き換えられる
  = baseline §15 の人間専有境界を壊す)。dump/restore も破壊的。→ v0.5.0 スコープとして
  ADR 付き issue を起こすかは **user 未回答**。
  **user 側ボール = (1) MCP write を v0.5.0 に入れるかの判断、
  (2) 公開済 468 コミットの履歴書き換えの判断。**)

- 日付: 2026-08-04 その2 (**#130 は PR #132 (`b08bb69`) で develop に着地・issue クローズ済。
  `feature/desktop-design-polish` も push 済 (`7f4f940..42dfa1c`)。**
  **さらに #42 (外部 bastion 経由の live MySQL 検証) は既に済んでいた** — user から
  「MySQL の接続は先日確認した」と指摘を受けて実機を確認したところ、dbboard の接続一覧に
  MySQL 種別のエントリが 1 件登録済で、`connections.toml` の当該ブロックには
  `[connections.ssh]` と鍵パスフレーズの keyring ref が入っていた = SSH トンネル経由。
  **これまでのエントリが「未着手」として引き写し続けていたのが誤り** (前回の
  `TAURI_SIGNING_PRIVATE_KEY` と同じ、確認せずに前エントリをコピーした結果)。
  接続情報 (host / user / port) は tracked ファイルには書かない — 非公開メモリと
  `.pii-denylist` のみ。
  **今の user 側ボール = 公開済 468 コミットの履歴書き換えの判断のみ**
  (ローカルの未 push 作業は無くなったので、やるなら今が最も安全)。
  **エージェント側の次候補** = cargo CI の新設 (pre-push が唯一のゲートである件)、
  pii-scan の `--denylist-digest` モード。)

- 日付: 2026-08-04 (**issue #130 = pre-push の再コンパイル解消。commit `e271726`、ADR-0086。**
  修正は `apps/desktop/src-tauri/Cargo.toml` の `crate-type` 1 行。
  **原因は推測されていたもの (`build.rs` の `cargo:rerun-if-changed` 欠落) ではなかった** —
  `cargo build --release` を 2 回続けると 1s / 0 件で終わるので無条件リビルドではなく、
  再コンパイルは **build と test が交互に走るときだけ**出ていた (= pre-push の形)。
  `CARGO_LOG=cargo::core::compiler::fingerprint=info` が
  `UnitDependencyInfoChanged` を lib ユニットに、バイナリは波及 (`StaleDependency`) と
  名指し。両コマンドが**同じ** fingerprint ファイルを書いていた: `staticlib`/`cdylib` は
  出力ファイル名が固定なので cargo の `-C metadata` ハッシュを持てず、ビルド構成ごとに
  fingerprint ディレクトリが分かれない。`--all-features` はこのワークスペースでは no-op
  (`[features]` も `optional` も無い) だが dev-dependency はグラフに合流するので
  `hyper` が `full`/`http2` 等を得て依存ハッシュが正当に変わり、1 つの枠を奪い合っていた。
  裏取り = `.fingerprint` ツリー全体で **1210 ユニット中、差は 1 つだけ**。
  修正は `crate-type = ["rlib"]` (staticlib/cdylib は Tauri テンプレ由来のモバイル用で、
  `gen/android`・`gen/apple`・`cfg(mobile)` はどれも存在しない)。
  **実測: test 直後の build 42s→2s、build 直後の test 94s→56s、pre-push 合計 約136s→58s、
  再コンパイル 0 件。** 残る 56s はテスト実行時間 = この issue のスコープ外。
  全ゲート green・pre-commit 全通過 (`--no-verify` 不使用)・release バイナリの起動も確認。
  **副次的な発見: このリポには cargo の CI が無い** (`.github/workflows/` は pages /
  pii-scan / release のみ)。つまり **pre-push が唯一のビルド・テストゲート**で、
  遅いから飛ばすと誰も検査しなくなる。#131 の baseline §35 が前提にする「最後の砦は CI」は
  このリポでは成立しない。
  **今の user 側ボール = (1) `chore/post-pr129-doc-sync` の push (2 コミット: `2fab7ba` docs +
  `e271726` perf) → PR → develop、以下は据え置き: (2) 公開済 468 コミットの履歴書き換えの
  判断 (やるなら**先に全ローカル作業を push してから**)、(3) `feature/desktop-design-polish`
  の push (未 push 14 コミット)、(4) #42 = 外部 bastion 経由の live MySQL 検証
  (**実接続 = 明示的な GO と認証情報が必要**)。**エージェント側の次候補** =
  `--denylist-digest` モード、cargo CI の新設 (上記の副次的発見)。
  **記録の訂正: `TAURI_SIGNING_PRIVATE_KEY` は 2026-07-30 に投入済み** (`gh secret list` で確認)。
  前エントリから user ボールとして引き写していたが、既に済んでいたもの。ただし
  `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` は未設定 — 鍵にパスフレーズを付けていない場合は
  空で通るので、v0.4.0 の署名が通るかは**最初のリリース実行時に判明する**。
  **リリース状況の実測**: 最新リリースは v0.3.0 (2026-07-22)、workspace version は既に
  `0.4.0`、`origin/develop` は `origin/main` より **76 コミット先行**。つまり v0.4.0 は
  「切るかどうか」ではなく「中身が溜まりきっている」状態。)
- 日付: 2026-08-03 (**identity 赤の解消 = ADR-0085 (PR #128 / #129)、および CI の
  denylist 層が初めて実稼働。** セッション開始時の §18 手順で `develop` の `pii-scan` が
  赤だったのが発端。中身は**本物 1 件と誤検出 1 件が重なっていた**。
  **本物** = GitHub の「Squash and merge」は web UI 側でコミットを作るので、この clone の
  `git config user.email` が noreply でも**アカウントのプライマリアドレスが author に入る**
  (PR #127 の squash `c355802`)。→ user が GitHub の Settings → Emails →
  **Keep my email addresses private** を ON (§15 = human 操作)。次の squash `aa90129` の
  author が noreply になったことで効果は実証済み。
  **誤検出** = 同じ `aa90129` の *committer* が `noreply@github.com` (GitHub 自身の web-flow
  アドレス)。`users.` 配下ではないので ADR-0084 の許可正規表現が弾いていた。**ADR-0084 が
  着地して以来 web マージのたびに出ていた**が、上の本物と同じ赤い X に見えるので
  **片方がもう片方を隠していた**。→ ADR-0085 で許可を全文 alternation にしてこの 1 個を追加。
  **`PII_DENYLIST` secret が存在しなかった** = 日次スキャンのログが
  `literal name detection off` で、**CI は実店舗名 3 件を含む `develop` を、clean だからでは
  なく照合対象を持っていなかったから緑にしていた**。§15 に従い手順のみ提示、user が作成 →
  run 30784716586 で `materialized from secret` を確認、BLOCKING 無し。
  **`develop` は green** (push run 30786499201 / 日次 run 30803841065)。
  **今の user 側ボール = (1) 公開済 468 コミットの履歴書き換えをやるかどうかの判断
  (やるなら**先に全ローカル作業を push してから** — 順序を間違えると未書き換えの
  ローカルコミットが即座に再汚染する。runbook に追記済)、(2)
  `feature/desktop-design-polish` の push、(3) v0.4.0 前に `TAURI_SIGNING_PRIVATE_KEY` 設定、
  (4) #42 = 外部 bastion 経由の live MySQL 検証 (**実接続 = 明示的な GO と認証情報が必要。
  エージェントは勝手に接続しない**)。**エージェント側の次候補** = `--denylist-digest`
  モード (ローカル `.pii-denylist` と CI secret の中身を sha8 で突き合わせる — 現状、
  貼り間違い・部分コピーでも CI は緑になる)、issue #130 (`dbboard-desktop` のフル再
  コンパイルで pre-push 約 4 分。原因確定は `CARGO_LOG` を先に取ってから)。)


---

- 日付: 2026-08-06 (**v0.5.1 リリース済み。タグは `main` の `bf7a696`。**
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
  マージコミット `bf7a696` は hook を全部通している。
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

---

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
  `docs/document-store-guides` の `1ab3e74`。衝突は `site/index.html` の OGP 1 箇所で、
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
  踏み続ける)、`c5833f3` として commit 済み — **これも未 push**。
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
  — `ci/cargo-and-frontend-checks` (テスト修正 `c5833f3` を載せて #166 を緑にする)、
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


---

- 日付: 2026-08-19 その2 (**v0.9.0 を出した。タグ push まで完了、release CI 実行中。
  あわせて CI が遅い件を計測して潰し、#194 を実装した。未 push は 5 本に増えた。**
  ① **リリース経路**: #197 (`develop` → `main`) マージ済 → `main` = `3527998`、
  タグ `v0.9.0` (annotated・`3527998` を指す) push 済 → release run `32265722883`。
  タグ push だけで完結する (v0.5.0 以降)。**残るのは公開 `.exe` の PII 目視確認だけ。**
  ② **タグ push に `--no-verify` を使った (baseline §35 の記録)。** pre-push の
  `cargo build --release` が `LNK1104: dbboard_mcp.exe を開くことができません` で落ちた。
  原因は `target/release/dbboard-mcp.exe` が **3 プロセス動いていた**こと (PID 12340 /
  17524 / 31856、いずれも親が生きている `claude` = **別セッションの MCP サーバー**)。
  他セッションを落とす判断はしていない。タグが指す `3527998` は**すでに origin/main に
  あるコミット**で、その SHA に対する CI は当日 14:26Z に `ci` / `pii-scan` とも緑。
  つまり pre-push が確かめようとしたことはリモート側で既に済んでおり、タグ push は
  新しい内容を 1 バイトも送っていない。環境要因 + CI 緑確認済 = §35 の条件どおり。
  ③ **CI が遅い件は Rust のせいではなかった (ADR-0114、`ci/faster-verification` =
  `e7442f9`)。** 16m46s のジョブの内訳を実測すると、apt install **572s** /
  cargo キャッシュ復元 **255s** に対し、**fmt + clippy + check + test は 158s** しかない。
  真因は 2 つ。(a) `-dev` パッケージ 89.3 MB を Azure の Ubuntu ミラーが **~163 kB/s**
  でしか流さない (レートが run ごとに変わるので、総時間が 16 分〜30 分超と暴れていた
  のもこれで説明がつく)。(b) キャッシュした `target` が **8.92 GiB** に育ち、GitHub の
  **リポジトリあたり 10 GiB** 上限に当たって、保存のたびに他のエントリを全部追い出して
  いた → 次の run は部分一致で復元してどのみち再ビルド。対策は `.deb` を
  **ワークフローファイルの hash をキーに**キャッシュ (apt は `_apt`、`actions/cache` は
  `runner` で読むので**所有権の受け渡しが要る**。これを戻さないと保存が黙って空になる) と、
  `CARGO_PROFILE_DEV_DEBUG` / `_TEST_DEBUG` を `false` (CI はデバッガを開かないし、
  シンボル付きバックトレースを検証するテストも無い)。`cargo check` は clippy に
  包含されるが 11s なので**残した** — ワークフロー冒頭が CLAUDE.md の必須コマンドを
  回すと約束しているため。**ローカルでは検証できない**ので、証拠は最初の run の
  ステップ時間になる。
  ④ **#194 を実装した (`fix/export-ref-ownership` = `d6f8b84`、ADR-0113)。**
  他接続の keychain スロットを名乗る entry を**エクスポート時**に検出する。ref は
  `admin.rs` の `keyring_ref()` 1 箇所でしか作られず、`update` は `id` を書き戻すので
  **rename 経路が存在しない** = 「自分の id から導出されない ref を持つ entry は不正」が
  **ストアを引かずに entry 単体で**判定できる。インポート側 (ADR-0038) より強い。
  **拒否はしない・警告する**のが ADR-0113 の決定 — バックアップを最も必要とする
  操作者にとって、成功行を警告で押しのけたら「失敗した」と読めてしまう。
  ⑤ **未 push = 5 本。** `ci/faster-verification` (`e7442f9`) と
  `fix/export-file-names` (`a993ad0`) は独立、残り 3 本は stack
  (`feat/turso-remote` `c645fe4` → `fix/import-report-reasons` `e5ae75c` →
  `fix/export-ref-ownership` `d6f8b84`)。**`ci/faster-verification` を先に入れると、
  以降の全 run が 9 分の apt ダウンロードを払わなくなる**ので優先度が高い。
  **user 側ボール = ① 公開 `.exe` の PII 目視確認、② 5 本の push (→ PR は私が作る)、
  ③ #180 / #189 のマージ、④ v1.0 の残り 3 ゲート (下記 候補 0)、⑤ 従来からの継続分。**
  次のエージェント側タスク = push 後の PR 作成のみ。)

- 日付: 2026-08-19 (**未 push のブランチが 3 本たまった。3 本とも中身は完成・検証済で、
  詰まっているのは push だけ。** develop = `5ac81bf` (v0.9.0)、open PR = #180 / #189。
  ① **`fix/export-file-names` = `a993ad0`** — エクスポートの既定ファイル名に日時を入れた。
  user 要望「ファイル名固定なのやめてほしい。日時いれるだけでだいぶ変わる」そのまま。
  ② **`feat/turso-remote` = `1d39378` + `c645fe4`** — issue #191。リモート Turso を
  2 つ目の kind として実装 (**ADR-0111**)。kind は 11 種になったが**ワイヤ id は 9 のまま**
  (`turso-remote` は `turso` として名乗る) = コントラクト非破壊。docs は別コミット。
  ③ **`fix/import-report-reasons` = `e5ae75c`** (②の上に stack) — user の指摘どおり、
  **インポートの「入らなかった理由」3 つを分けて報告する**ようにした (**ADR-0112**)。
  もとは `skipped: Vec<String>` 1 本に (1) 束内 id 重複 / (2) 既存 + Skip モード /
  (3) 他接続が持つ keyring ref との衝突 (ADR-0038) が全部入っていた。
  「既に存在します」が真なのは (2) だけ、overwrite で直るのも (2) だけ。
  (3) は**両方とも嘘**で、しかも overwrite 再実行はバイト同一の結果になるため、
  ヒントが操作者を行き止まりに送っていた。`skipped_existing` / `duplicate_in_bundle` /
  `refused: Vec<RefusedEntry>` の 3 本に分割し、refusal は**衝突の両側** (ref 名と
  その持ち主) を名指しする。文言規則は `import-report.ts` に純関数として出して
  9 本のテストで固定。**チェック自体は一切変えていない** (user の
  「Not a request to relax the check」どおり)。②③とも stack しているのは、
  同じ 5 ファイルを触るため。②が develop に入れば③の差分は自分の変更だけになる。
  ④ **ディスクが 0 GB になり commit が落ちた。** pre-commit フックは私の
  `CARGO_PROFILE_DEV_DEBUG=false` を継承しないので、debuginfo 付きで debug ツリーを
  丸ごと再ビルドしにいく。`cargo clean --profile dev` で **16.6 GiB 回収** (現在 16.3 GB 空き)、
  `target/release` は pre-push のために残した。**これで 08-16 から未回答だった
  「`cargo clean --profile dev` の可否」は事実上決着**。再発防止は
  `export CARGO_PROFILE_DEV_DEBUG=false CARGO_PROFILE_TEST_DEBUG=false` を
  **commit と同じシェル行に置く** (フック側の cargo に継承させるため)。
  ⑤ **新規 issue 3 本** — **#194** = 同じ ref 衝突を**エクスポート時**に検出する
  (ref は `admin.rs` の 1 箇所でしか作られず rename 経路が無いので、
  「自分の id から導出されない ref を持つ entry は不正」が**ストア参照なしで判定できる** =
  インポート側の検査より強い)。**#195** = `dbboard-mcp.exe` に更新経路が無い。
  **#196** = パスフレーズをワイヤに乗せない MCP エクスポート verb (user の
  「パスワード設定も AI エージェントに託した方がセキュア」に対する回答。
  生成はエージェント側が正しいが、**MCP の戻り値は Claude Code の `.jsonl` に
  平文で残る**ので、値ではなく資格情報ストアの **ref 名**だけを返す設計にした)。
  **user 側ボール = ① 3 本の push (→ PR は私が作る)、② #180 / #189 のマージ、
  ③ v1.0 の残り 3 ゲート (下記 候補 0)、④ 従来からの継続分。**
  次のエージェント側タスク = push 後の PR 作成のみ。)

- 日付: 2026-08-19 (**検証シートを人手で回すつもりが、MCP の口を 7 つ足す話になった。
  PR 6 本マージ、シート 003 が 10 行すべて OK で埋まった。open は #180 の 1 本のみ。**
  ① **発端は「シート 003 を user が実施する」だった。** 言語メニューを開く・切り替える・
  画面を見る、を 1 手ずつ頼むはずが、頼む前に**その手をエージェント側から打てるようにした** —
  エディタ・実行・AI パネルの操作 4 動詞 (#185)、表示言語の読み書き 2 動詞 (#182)、
  ウィンドウの撮影 1 動詞 (#184)。
  「人の操作が要る」と気づいた時点で MCP に足す、という取り決めをそのまま実行した形。
  ② **判定はエージェント、記入は user。** 撮った画像を見て豆腐 (□) の有無を判断したのは
  エージェント側だが、`結果` 列に OK を入れたのは user (#186)。baseline §22 の
  「実物を動かして目で見た人だけが書く」は崩していない。
  ③ **10 行すべて OK。豆腐は 1 つも出なかった。** egui 版で 2 回再発した不具合が、
  Tauri + WebView2 に移った後は再発していないことを、現行シェルで初めて人の目で確認した。
  ④ **代わりに別の穴が見えた → Issue #181。** 11 ロケール中 9 つが **334 キー中 30 キー
  (9%) しか訳されていない**。切替は動くが中身が英語のまま。
  シートが通ったことより、通す過程で見えたこちらの方が大きい。
  ⑤ **シートから `実施日` / `担当` を落とした (#187)。** baseline §22 に公開リポ条項が
  付いた。commit の author と日時が同じことを持っているのに、シート側にも個人の作業記録を
  重ねると公開範囲だけが広がる。003 が両列を埋めた直後だったので具体的に効いた。
  ⑥ **ディスクが尽きた (2 回)。** `target/debug` が 14.2 GB まで育ち、空きが 5 GB を切って
  リンカが `os error 112` で落ちた。`cargo clean --profile dev` で回収 (pre-push が要るのは
  release 側だけなので debug を捨てて困らない)。空き 18.1 GB。
  **が、その後の debug フルリビルドでまた 5.7 GB まで落ちて `serde` の `.rmeta` が
  途中で切れ、コンパイラが `E0786 invalid metadata files` で落ちた。**
  リンカエラーに見えたり crate 破損に見えたりするが、**症状が変わるだけで原因は同じ**。
  → **user 側ボール: C: を repo の外で空ける。** `target/debug` (14.2 GB) と
  `target/release` (22 GB) が同居できる余地が無く、いまの空き 14 GB では
  **debug のフルビルドが完走しない = pre-commit が原理的に走らない**。
  候補は pnpm ストア 8.1 GB / 他リポの `node_modules` 約 4.5 GB / Temp 1.7 GB。
  どれもこのリポの持ち物ではないので、消す判断は user 側。
  ⑦ **#180 は rebase 済み。** develop が動いたことで 3 シートのヘッダが衝突した。
  rebase で解消し、003 は develop 側が同じ内容を持っているので**差分から消えた** (正しい形)。
  現在 MERGEABLE。
  **user 側ボール = #180 のマージ + v1.0 の残りゲート (下記)。**
  次のエージェント側タスク = **無し**。)

- 日付: 2026-08-16 その2 (**v1.0 ゲート 4 (コード署名) を「買わない」側で閉じた。
  残りは 3 つ、全部 user 側ボール。PR #176 / #177 / #178 マージ済、open PR = 0。**
  ① **user 判断: 証明書は買わない。** issue 0021 のゲート 4 には最初から代替経路が
  書いてあった (「買わないなら未署名であることを README とリリースノートに明記して出す」)
  ので、そちらを取った。**ADR-0106** に決定として記録。
  ② **文言が嘘になっていた。** README 2 箇所と DL ページが `not signed **yet**` /
  `planned follow-up` / `tracked follow-up` と、1 年分のリリースにわたって書き続けていた。
  買う予定が無いのに「まだ」と書くと、読み手は**後のリリースで消える不具合**と受け取る。
  全部「決定であって漏れではない」に書き換え、検証すべきものとして `SHA256SUMS.txt` を
  名指しした。
  ③ **注記をリリース本文にも載せた** (`release.yml` の `--notes`)。検索結果から
  リリースページに直接来た人は README を見ない。**ダウンロードが提供される場所すべて**に
  注記が付く状態にするのが ADR-0106 の約束。
  ④ **文言をテストで守る** — `site/page.test.mjs` に 1 本追加。"Before you run it" 段落に
  `yet` / `planned` / `tracked follow-up` / `coming soon` が現れたら落ちる。
  **すでに 2 ファイルで間違っていた**ので記憶ではなくテストにした。先に RED を確認済み。
  ⑤ **`delete_branch_on_merge` を `true` にした。** マージ後のリモートブランチ削除が
  自動になり、PR ごとの削除 push が不要になった (#178 で実際に自動削除を確認)。
  ⑥ **`actions/checkout` を v6 に統一** (#176)。ただし**最新は v7.0.1** で、v6 は 1 メジャー
  遅れ。恒久対策として `.github/dependabot.yml` の `github-actions` エコシステムを
  提案したが、**PR が増えるため要否は user 判断**として保留中。
  ⑦ 検証: pre-commit (fmt / clippy -D warnings / check / test) 通過、pii-scan は tree と
  commit message の両方 clean、`node --test site/*.test.mjs` 16/16、
  `release.yml` は PyYAML パース + 抽出した run ブロックの `bash -n` で構文検証。
  PR #178 の CI 4 本すべて緑。
  **user 側ボール = v1.0 の残り 3 ゲート (下記) + dependabot の要否判断。**
  次のエージェント側タスク = **無し**。3 ゲートとも baseline §38 の「人にしかできない工程」で、
  user が動くまでエージェント側から進められない。)

- 日付: 2026-08-16 (**v1.0 の条件を 4 つに確定して #175 をマージした。
  ここから先は 4 つとも user 側ボール。**
  ① **v1.0 = 機能が出揃うことではなく、`docs/api-contract.md` を壊さない約束** (ADR-0011)。
  この定義に落とすと、エンドポイントやフラグの追加は additive なので 1.0 を妨げない。
  ロードマップ上の未着手項目のうち実際にゲートになるのは **4 つだけ**で、
  全部 `.claude/issues/0021-v1-0-criteria.md` に書いた。
  ② **凍結の前にコントラクト自身が凍結できる状態になかった。** `id` の一覧が 3 件のまま
  (実際は 9 件)、`has_foreign_keys` (ADR-0054) が未記載、`GET /capabilities` の例 (5 フラグ) と
  `Capabilities` の節 (10 フラグ) が食い違い、「Phase 2 では全フラグ `false`」が 3 箇所。
  全部直し、**先に RED を確認してから**
  `crates/dbboard-connect/tests/api_contract_drift.rs` で再発を止めた
  (フラグ側は `Capabilities` をシリアライズして名前を取るのでテスト編集不要、
  id 側は `BackendConfig` の網羅 match なのでバックエンド追加でビルドが止まる)。
  ③ **ロードマップの帳簿修正**: Phase 2 が全項目 `[x]` なのに `*(current)*` のまま
  (exit criteria が参照する `crates/dbboard-ui` は ADR-0089 で削除済み)、
  `Export results (CSV / JSON)` が未チェックだが CSV/TSV は ADR-0035 で出荷済み。
  どちらも 1.0 までの距離を実際より遠く見せていただけ。
  ④ **エージェント側のミス 1 件 (記録)**: #175 の本文に
  「`actions/checkout` を v6 へ」と書いたが、**その commit (`ea59c4a`) は push 時点の
  ブランチ先端に無く、PR に入っていなかった**。ローカル HEAD で
  `git log origin/develop..HEAD` を数えたのが原因。**PR 本文は push 済みの範囲
  (`origin/<branch>`) で数える。** マージ後に PR 本文へ訂正を追記し、
  中身は `ci/checkout-v6` の `55fbba1` として cherry-pick 済 (原 commit と patch 一致を確認、
  author は user のまま)。
  ⑤ **`actions/checkout` の最新は v7.0.1** で v6 は 1 世代前。今回は user の明示的な選択なので
  v6 のまま出す。
  **user 側ボール = ① `git push -u origin ci/checkout-v6` (→ PR は私が作る。
  CI 自身の定義を変えるので CI 緑がそのまま動作確認になる)、
  ② v1.0 ゲート 1 = #161 の 3 点観察 (ボタンの色 / カーソル形状 /
  一度別の場所をクリックしてからだと効くか)、
  ③ v1.0 ゲート 2 = コントラクトを姉妹リポ `dbboard-web` へミラー (リポをまたぐ)、
  ④ v1.0 ゲート 3 = 検証シート 001/002/003 の実施 (baseline §22・人間のみ。
  Firestore エミュレータが動いている間は 001 の 2〜9 行目が実施可能)、
  ⑤ v1.0 ゲート 4 = コード署名を買うか、買わないなら「未署名」を README と
  リリースノートに明記して出す (Norton / SmartScreen が騒ぐ件の恒久解はこれ)、
  ⑥ Norton の除外設定 (GUI のみ・**除外リストは 2 つあり、両方に入れないとビルドは速くならない**)、
  ⑦ 公開 `.exe` の PII 目視確認、⑧ 姉妹リポへ `.claude/tools/dbboard.md` を貼る (08-09 から継続)、
  ⑨ ~468 コミットの history 書き換え判断 (08-09 から継続)** —
  ②〜⑤ が v1.0 の全部。①は今すぐ終わる。
  なお `cargo clean --profile dev` (`target` 48 GB の dev 側を落とす。release は残るので
  pre-push は速いまま、次の dev ビルドだけフルになる) の可否は**未回答のまま**。)

- 日付: 2026-08-14 その2 (**v0.8.0 を切った。タグ push まで完了、release CI 実行中。
  ここから先は全部 user 側ボール。**
  ① **リリース経路**: #171 (エクスポートダイアログの可読性) → develop、
  #172 (リリース準備) → develop、#173 (`develop` → `main`) → `main` = `29413b4`、
  タグ `v0.8.0` push 済 → release CI run `31784033330` 実行中。
  Windows exe + MSI / macOS dmg + `SHA256SUMS.txt` を publish する。
  **タグ push だけで完結する** (v0.5.0 以降、publish ジョブが release オブジェクトを
  自力で view-or-create するようになったため。旧 v0.3.0 の落とし穴は解消済み)。
  ② **リリース前に埋めた穴**: `CHANGELOG.md` の `[Unreleased]` が空、`docs/roadmap.md` が
  v0.7.0 を現行として説明したままだった。どちらも**タグ後には埋められない**場所なので
  #172 で先に埋めた。
  ③ **エージェント側のミス**: DESIGN.md の追記 (`128f18e`) を #171 の push 後に commit して
  マージに乗せ損ねた。rebase + cherry-pick (`19d0564`) で復旧済み。
  ④ **release CI は 5 ジョブすべて緑**、`v0.8.0` は 08-14 08:44Z に publish 済
  (`dbboard-desktop_0.8.0_x64-setup.exe` / `_universal.dmg` / `.app.tar.gz` /
  MCP の win・mac / `latest.json` / `SHA256SUMS.txt`)。
  **DL ページは `releases/latest` を指しているだけなので、サイト側の変更は不要** —
  publish された時点で自動的に v0.8.0 になる。
  **user 側ボール = ① 公開 `.exe` の PII 目視確認 (CI はやらない)、
  ② baseline §24 の security-reviewer を回すかの判断 (推奨。ただし今回は既存経路の
  UI 改善で新しい外向き通信は無い)、③ 検証シート 001/002/003 (全部 `未実施`。
  Firestore エミュレータが動いている間は 001 の 2〜9 行目が実施可能。1・10・11 行目は
  対象環境が無いので `未実施` のまま)、④ 姉妹リポへ `.claude/tools/dbboard.md` を貼る
  (08-09 から継続)、⑤ ~468 コミットの history 書き換え判断 (08-09 から継続)、
  ⑥ #161 の 3 点観察** — ⑥ が引き続きいちばん詰まっている。
  なお Firestore エミュレータを止めるときは
  `docker compose -f docker/firestore-emulator/compose.yaml down`。)

- 日付: 2026-08-14 (**open PR = 0。滞留は完全に解消した。残っているのは全部 user 側ボール。**
  ① **#159 と #169 をマージし、open PR がゼロになった。** develop = `8dd3ac5`。
  #159 = 文書ストアをガイドに書く (`site/index.html` の OGP 衝突は説明文 #159 側 /
  プレビュー画像 develop 側で組み合わせ済み)、#169 = 08-13 のセッション記録 2 ファイル。
  ② **#159 の push で 1 往復ロスした。私 (エージェント) のブランチ名取り違え。**
  PR の head は `docs/document-stores-in-guides` (**stores**) だったが、
  `git checkout -B docs/document-store-guides ...` の**第 1 引数はローカル名**なので、
  作業が別名ブランチに乗った。そのまま push すると PR に紐づかない新規ブランチができ、
  #159 は `CONFLICTING` のまま残る。**復旧は refspec 指定の push**
  (`git push origin <ローカル名>:<PR の head 名>`) — ローカル名が違う限り
  `git push` 単体は `push.default=simple` に弾かれる。
  ③ **libSQL テアダウン segfault は pre-push (release) でも出る。** 今回は
  `dbboard-server` の `tests/http.rs` が `0xc0000005` で落ちた。単独で回すと
  **12/12 緑**で、プロセス終了時のクラッシュにすぎない (`dbboard-connect` 経由で
  libsql をリンクしているため)。CLAUDE.md 唯一の bypass 該当・baseline §35 どおり
  `--no-verify` で push し、CI 4 ジョブ緑を最終ゲートとして確認した。
  **user 側ボール = ① 姉妹リポへ `.claude/tools/dbboard.md` を貼る (08-09 から継続)、
  ② ~468 コミットの history 書き換え判断 (`pii-scan` identity 赤の唯一の原因・
  08-09 から継続)、③ #161 の 3 点観察** (ボタンの色 / カーソル形状 /
  一度別の場所をクリックしてからだと効くか) — **ここが今いちばん詰まっている。**
  次のエージェント側タスク = #161 の観察結果を受けて修正。原因が特定できていない段階で
  当て推量のテストは書かない。)
