# 次のアクション案内 (user 側ボール常設ファイル)

このファイルは「セッションが切れても、開いた瞬間に user 側のボールがわかる」状態を維持するためのもの。
セッション終了時 / 状況が変わった時に必ず更新する。**日本語可・内部用。**

---

## 最終更新

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

- ※ 2026-07-29 以前のエントリは `.claude/archive/next-actions-2026-07.md` へ全文退避
  (baseline §31、退避日 2026-08-05)。

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
