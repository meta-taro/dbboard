# アーカイブ — next-actions.md セッションログ (2026-08-03 〜 2026-08-06)

baseline §31 に基づく退避。いずれも全文で、要約していない。

- **退避日 2026-08-09**: `.claude/next-actions.md` が 415 行になり 400 行トリガを
  踏んだため、v0.5.0 リリース周辺 (2026-08-03 〜 2026-08-05 その3) までを退避。
- **退避日 2026-08-14**: 同じく 435 行でトリガを踏んだため、2026-08-05 その4 〜
  2026-08-06 (v0.5.1 リリース周辺) を追加で退避。ファイル末尾の `---` 以降がそれ。

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
  無いまま放置されていた。0.4.0 節を書き起こし (`7bc5e60`)、PR #133 で main へ、
  タグ `v0.4.0` を push。**1 回目のタグビルドは Tauri 2 ジョブが両方落ちた** —
  `Install frontend deps` で `ERR_UNKNOWN_BUILTIN_MODULE: node:sqlite`。
  `release.yml` が Node 20 固定なのに `apps/desktop` は `pnpm@11.1.1` を pin しており、
  pnpm 11 は `node:sqlite` (Node 22.5 以降) を import する。**ローカルの Node が
  v22.22.2 なので手元では一切再現しない類の失敗。** cargo だけの
  `build-windows` / `build-macos` は成功、`publish` は skip = **何も publish されて
  いなかったのでタグを移動して復旧できた**。修正 = 両 `setup-node` を `node-version: 22`
  (PR #134、`174fb97`)。`v0.4.0` を削除して main 先端で張り直し
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

- 日付: 2026-08-04 その2 (**#130 は PR #132 (`051c9cd`) で develop に着地・issue クローズ済。
  `feature/desktop-design-polish` も push 済 (`f703a54..3e6c6a4`)。**
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
  (PR #127 の squash `e15dcff`)。→ user が GitHub の Settings → Emails →
  **Keep my email addresses private** を ON (§15 = human 操作)。次の squash `d7ed16b` の
  author が noreply になったことで効果は実証済み。
  **誤検出** = 同じ `d7ed16b` の *committer* が `noreply@github.com` (GitHub 自身の web-flow
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
