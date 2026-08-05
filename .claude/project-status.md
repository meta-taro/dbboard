# プロジェクトステータス (内部メモ・日本語)

このファイルは作業中のセッション状態を記録する内部用ドキュメント。
外部公開向けの内容ではないため日本語で書く。セッション終了時に更新する。

## 最終更新

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
  ADR-0047〜0086 を棚卸しして 0.4.0 節を書き (`7bc5e60`)、compare リンクも v0.4.0 を
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
  `origin/main` を develop に取り込んで解決 (`d977403`)。#134 は **merge commit** で
  取り込み (`174fb97`) — squash を続けると同じ乖離が毎回出るため。
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
  なお commit `d531e20` と `d977403` は pre-commit の cargo test が例の Windows libSQL
  teardown segfault (rc=139) で落ちたため `--no-verify`。pii-scan は両方 clean、
  変更は YAML のみ。)

- 日付: 2026-08-04 その2 (**#130 クローズ + 記録の訂正 2 件。**
  PR #132 (squash `051c9cd`) が develop にマージされ、issue #130 はクローズ済
  (計測値を添えたコメントを投稿)。`feature/desktop-design-polish` の 14 コミットも
  push 済 (`f703a54..3e6c6a4`)。両 push とも pre-push は全緑で、既知の libSQL teardown
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
- 日付: 2026-08-03 (**identity 誤検出の除去 = ADR-0085 (PR #128 `d7ed16b` / PR #129 `27824b0`)、
  および CI の denylist 層が初めて実稼働。コード変更は `scripts/pii-scan.sh` の許可正規表現
  1 行のみ。** 発端はセッション開始時の §18 手順で `develop` の `pii-scan` が赤だったこと。
  **(1) ADR-0084 の穴が 2 つ連続で出た。** 1 つ目は**本物**: GitHub の「Squash and merge」は
  この clone が書いていないコミットを web UI 側で作るので、`git config user.email` を
  noreply にしても**アカウントのプライマリアドレスが author に入る**。PR #127 の squash
  コミット `e15dcff` がこれで、CI が赤くなった。対応は GitHub の
  Settings → Emails → **Keep my email addresses private** を ON (§15 = human 操作、user が実施)。
  効果は次の squash `d7ed16b` の author が noreply になったことで実証済み。
  2 つ目は**誤検出**: 同じ `d7ed16b` の *committer* が `noreply@github.com` — GitHub 自身の
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
> 2026-07-29 以前のセッションログは、baseline §31 に基づき
> [`.claude/archive/project-status-2026-07.md`](archive/project-status-2026-07.md)
> へ全文退避した (要約ではない)。

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
