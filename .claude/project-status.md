# プロジェクトステータス (内部メモ・日本語)

このファイルは作業中のセッション状態を記録する内部用ドキュメント。
外部公開向けの内容ではないため日本語で書く。セッション終了時に更新する。

## 最終更新

- 日付: 2026-08-26 (**v0.13.0 を切った (`5022112` / タグ `v0.13.0`)。公開済み。
  併せて、開発機がこの Windows PC から Mac mini へ移ることになったので引き継ぎを書いた。**

  ### v0.13.0

  見出しは "Knowing what changed, and letting an agent tidy up"。
  中身は前日 (2026-08-25) に来た 2 件で、追加した機能はこの版では増やしていない。

  - バージョン確認画面に更新内容 (ADR-0137)
  - MCP から目印と並び替え (ADR-0136)

  **なぜ 0.14 まで溜めずに切ったか。** 枠は予約であって締切ではない (ADR-0110 / ADR-0122) ので、
  次の 3 件 (`lib.rs` 分割 / MCP export / 手書きクエリ結果の編集) を同じ枠に積むこともできた。
  積まなかった理由は 3 つ:

  1. どれも v0.13 の見出し「何が変わったかを知る・整理を任せる」に**当てはまらない**。
     見出しと中身がずれた版は、後から CHANGELOG を読んでも何が出たのか答えられない。
  2. **ADR-0135 の「更新が止まっている」告知は 0.12 → 0.13 の更新でしか初めて動かない。**
     目印を書く側が 0.12.0 で出たので、読む側が実地で動くのはこの版が最初になる。
     ここを跨がせないと、告知は一度も通らないまま次の版に埋もれる。
  3. タグを打っていないコードは**誰にも届いていない**。待っているのは収集係と dbboard-web 側。

  手順は `release-cut.mjs` → `cargo check --workspace` (Cargo.lock を動かす) →
  `docs/roadmap.md` の v0.13 行を削除 → commit → `git tag v0.13.0`。
  roadmap の行を消すのは、出した後は CHANGELOG が答えるので、
  同じ問いに答えが 2 つあると計画の方が嘘をつき始めるため (`release-plan.test.mjs` が検出する)。

  検証: `release-plan.test.mjs` 7/7、`release-notes.mjs CHANGELOG.md 0.13.0` が 2 件とも描画、
  `release-due.mjs` = `nothing unreleased yet`、pre-commit 全 green・pii-scan clean。
  **`--no-verify` なし。**

  §24 のリリース前セキュリティ確認は**手で実施した** (このセッションは Agent tool を
  自発的に呼ばない設定のため)。ADR-0136 の差分と `dbboard-mcp` の書き込み経路を読み、
  (a) 新しい 2 verb はネットワーク面を増やさない、(b) 応答に秘密が乗らない
  (返すのは alias 射影済みの一覧のみ)、(c) 書き込みは呼び出しごとに
  `ConnectionAdmin::open` する既存経路 (ADR-0133) で、パレット / タグ長 / 添字の検証は
  `dbboard-config` 側にあり呼び出し元エラーへ写る、を確認。**新規の指摘なし。**

  ### タグ push の直後に CI が赤くなった (原因と対処・§23 PDCA)

  `929f27c` (ドキュメントだけの commit) で `ci` の frontend job が落ちた。
  `changelog.test.ts` が**バンドルされた CHANGELOG の最新リリースを `'0.12.0'` と直書き**して
  いたため、0.13.0 を切った時点で赤くなる。**コードは 1 行も触っていない。**

  - **Plan**: 版を上げるたびに落ちるなら、それはテストではなく作業。期待値を毎回書き換える
    テストは、唯一発火する場面が常に誤報になる。
  - **Do**: `package.json` の版と比べる形に変えた (`87a976b`)。About ダイアログが実際に
    依存している不変条件はこちら — **最新のリリース節は、いま動いているビルドの版**であること。
    `release-cut.mjs` が両方を同じ編集で動かすので、ずれたら本当に異常。
  - **Check**: `pnpm check` 0 errors / 325 files、`pnpm test` 632 passed。
  - **Act**: **frontend のテストは CI でしか走らない** (pre-commit は cargo 側だけ) ので、
    フロントだけを触った版上げは手元が緑でも赤くなりうる。切る前に `pnpm test` を回す。
    なお `release.yml` はテストを走らせないので、**公開そのものは止まっていない**。

  ### 公開の結果 (タグ `v0.13.0`)

  `release` ワークフロー 17m25s で success。draft でも prerelease でもない。
  資産 9 件：Windows NSIS `x64-setup.exe` + `.sig` / macOS universal `.dmg` /
  `dbboard-desktop.app.tar.gz` + `.sig` / MCP バイナリ 2 本 (windows-x86_64 / macos-universal) /
  `SHA256SUMS.txt` / `latest.json`。
  `latest.json` は `version 0.13.0`、platforms は darwin-aarch64 / darwin-x86_64 /
  windows-x86_64 の 3 つ。**既存の 0.12.0 からの自動更新がここを見る** —
  ADR-0135 の「更新が止まっている」告知が実際に踏まれるのもこの世代から。

  ダウンロードページは再デプロイ不要 (ADR-0047)。`site/` は読み込み時に
  Releases API を引くデータ駆動なので、`pages` は `site/**` が変わったときしか走らない。

  **残っているのは公開 exe の PII 目視だけ** (v0.11.0 / v0.12.0 / v0.13.0)。
  これは AI がやれないし、公開リポなので催促もしない (ベースルール §38)。

  ### 引っ越し (この Windows PC → Mac mini)

  記録は `.claude/next-actions.md` 冒頭の「引っ越し」節が正本。ここでは要旨だけ。

  **git が運ばないものが 3 つある。** リポジトリは clone すれば済むが、

  - `.pii-denylist` — untracked (`.gitignore:58`)。**遮断する語そのものは untracked 側にしかない**。
    無いまま作業すると pre-commit の pii-scan は**黙って通る**方に落ちる。
    中身が PII そのものなので **user が書く** (雛形は `.pii-denylist.example`)。
    CI 側の `PII_DENYLIST` secret は GitHub にあるので引っ越しの影響を受けない。
  - **エージェントの memory 27 ファイル (132KB)** — git 管理外なので**消したら復元できない**
    (baseline §33)。Mac ではプロジェクトキーが変わり自動では引き継がれない。
    うち 2 件は**リポに書けない情報の唯一の写し**。user が手でコピーする。
  - **git hooks** — clone 後に `sh scripts/install-hooks.sh`。cargo-husky は ADR-0119 で外したので
    誰も自動ではやらない (`hook_install_drift.rs` が遅れを検出する)。

  **Mac で消える地雷**: libSQL の teardown segfault (`0xc0000005`)、Norton の隔離、
  `link.exe 1318` / `os error 112` (ディスク枯渇)、scrypt の 256MiB×並列、
  起動中の release exe がロックして `cargo build --release` が落ちる件、GitHub Desktop の force push。
  **ただしスクリプトと直列クレート一覧は消さない** — CI の windows runner が同じ道を通るので、
  手元で踏まなくなっただけで危険が消えたわけではない。

  **効かなくなるルール**: `scripts/pii-scan.sh` の `windows-home-path` 規則は Windows の
  ホームパスだけを見ており、Mac の `/Users/<名前>` は素通りする。
  実ユーザー名は tracked file に書けないので、足すなら `.pii-denylist` 側 = **user**。

  **できるようになること / できなくなること**: `.dmg` と universal build が手元で確認できる
  ようになる代わりに、**Windows の `.exe` は手元でビルドできない**。
  公開物の PII 目視スキャンは **CI 成果物をダウンロードして**行う形に変わる。

  **AI がやれていないこと**: push (develop + タグ `v0.13.0`)、公開後の exe 目視スキャン、
  memory と `.pii-denylist` の移送、Mac 側での初回セットアップ。→ すべて **user 側**。)

- 日付: 2026-08-25 その6 (**「更新で何が変わったか」をアプリ内に出した (ADR-0137) / 目印と
  並び替えを MCP に開けた (ADR-0136)。commit 3 本が develop に未 push。**

  どちらも 2026-08-25 に user から出た要望そのまま。
  「md-business に比べて、アップデートによって何が変わったのか記載がない。
  それはバージョン確認画面に記載してほしい」「今回のタグ・色付け・順番入れ替えは、
  MCP いれてエージェントも操作可能にしてください。整理整頓を AI がやってくれたら
  助かる人はいるはずです」。

  ### バージョン確認画面に更新内容 (ADR-0137)

  出どころは `CHANGELOG.md` 1 本。第 2 の一覧を作らない。
  `?raw` でビルド時にバンドルへ焼き込む — インストールされた版の隣にリポジトリは無いし、
  実行時にファイルを読むと「その版が何を出したか」ではなく
  「いまの作業ツリーが何と言っているか」を表示してしまう。

  - `apps/desktop/src/lib/about/changelog.ts` — パーサ。Tauri も fs も触らない純関数。
    整形規則は `scripts/release-notes.mjs` と同一 (リンク→本文、`**`/`*`/backtick 除去)。
  - `apps/desktop/src/lib/about/bundled.ts` — `?raw` import と 1 回だけのパース。
  - `AboutDialog.svelte` — 版のプルダウン + 見出し + 変更一覧。`[Unreleased]` は出さない
    (誰も動かしていない版なので)。**changelog に載っていない版は何も出さない** —
    読み手が動かしている版について自信たっぷりに間違えるより、黙っている方がまし。
  - 変更履歴は英語のまま。ja ロケールのときだけ「変更履歴は英語で書かれています」と出す。
    和訳するとリリースごとの手間が倍になるので、これは別の判断として残す。

  この 1 件は user 自身の体験から出ている: 0.10 → 0.12 と飛んだので **0.11 が何だったのかは
  アプリのどこにも残っていなかった**。更新ダイアログは更新の瞬間に 1 度しゃべるだけで、
  飛ばした版の記録にはならない。

  ### 検証

  `pnpm check` 0 errors / 324 files、フロント `vitest run` **38 files 632 tests pass**、
  `pnpm build` 成功 + 焼き込みを実測で確認
  (`grep -rlo "A connection list you can steer" build/_app/immutable/` が 1 件ヒット)。
  `release-plan.test.mjs` 7/7、`release-notes.test.mjs` fail 0。
  pre-commit の 4 コマンドすべて green、`--no-verify` なし。

  `release-due.mjs` は **2 unreleased entries — a release may be cut
  (0.12.0 -> 0.13.0: Knowing what changed, and letting an agent tidy up)**。
  枠は `docs/roadmap.md` に v0.13 として取ってある (速度は v0.14、日々の作業は v0.15 へ繰り下げ。
  ADR-0110/0122 のとおり枠は番号を振り直さない)。

  ### §31 の棚卸し

  414 行 > 400 行だったので、2026-08-24 の 2 エントリを
  `.claude/archive/project-status-2026-08.md` へ**全文退避**し、
  元の位置には 1 行の案内だけ残した (退避は承認不要・削除は要承認)。320 行になった。

  **user 側に残っているもの**: `git push origin develop` (未 push は
  `f200c02` docs / `81ef4e9` feat MCP / `c55825b` feat About + 本エントリの docs commit)。
  タグを切るかどうかは ADR-0121 のとおり user の判断。

- 日付: 2026-08-25 その5 (**#222 / #225 / #223 を merge して v0.12.0 を切った。
  commit は `57afbfe` 1 本、タグは `v0.12.0`。push とタグ push は user。**

  ### merge (user が実施・10:21–10:22Z)

  3 本とも **squash ではなく `--merge`**。#225 は #222 の上に積んであったので、
  #222 を squash すると #225 が乗っている履歴の方を書き換えることになり、
  base の自動 retarget の後で衝突する。順番は #222 → #225 → #223。
  merge 後の open PR はゼロ。`b00a871` の CI は success。

  ### 0.12.0 のカット

  `node scripts/release-cut.mjs` → CHANGELOG の見出し・workspace version・
  manifest 2 つ → `cargo check --workspace` で `Cargo.lock` を動かす →
  **`docs/roadmap.md` の枠表から v0.12 の行を削除** → commit → `git tag v0.12.0`。

  枠表の行を消すのは飾りではない。`scripts/release-plan.test.mjs` は
  「出したはずの版に枠がまだ残っている」を drift として落とす (7 本中 1 本が赤になる)。
  出した後は CHANGELOG が「何が入ったか」に答えるので、同じ問いに答えが 2 つあると
  計画の方が先に嘘をつき始める — v0.11 のときと同じ扱いにした。

  中身は 11 件。並び替え / 絞り込み / 名前表示 / 色 + タグの目印 (#192)、
  一覧から直に目印 (ADR-0130)、サイドバーの横区切り (ADR-0131)、
  ダイアログの取り回し (ADR-0132)、外から書かれた接続の上書き修正 (ADR-0133)、
  MCP からの接続登録 (ADR-0134)、止まった自動更新の告知 (ADR-0135)。

  ### 検証

  pre-commit の 4 コマンドすべて green (pii-scan clean / `fmt` / `clippy -D warnings` /
  `check` / 直列テスト)。`--no-verify` なし。`release-plan.test.mjs` 7/7、
  `release-due.mjs` は `nothing unreleased yet`。

  **user 側に残っているもの**: `develop` の push と **タグ `v0.12.0` の push**
  (打った瞬間がリリース・ADR-0121)。publish job は release object を自分で
  view-or-create するので、タグ push だけで公開まで行く。公開後の exe の目視 PII チェックと、
  7d/7e/7f/7g の画面確認は未実施のまま — §38 により催促はしない。

  ### 公開まで完了 (同日 11:32Z)

  user が両方 push。`origin/develop` = `c430d6b`、タグ = `57afbfe`。
  release workflow (run 32841019679) は 5 job すべて success で、**v0.12.0 は公開済**。
  資産 9 点、`latest.json` は 0.12.0 / `dbboard-desktop_0.12.0_x64-setup.exe` を指している。

  **つまりノート PC の v0.10 には次の起動で 0.12.0 が出る。** インストールが止まる原因は
  0.12.0 でも直っていない (直したのは沈黙の方) ので、また止まる可能性はある。ただし
  今度は止まったこと自体が次の起動で画面に出る — ADR-0135 の判定は「走っている版 ==
  パンくずの `from`」なので、0.10 から 0.12 を試して 0.10 のままなら条件に合う。

  merge 直後の `#225` の ci が 1 件 cancelled だが、11 秒後に `#223` の merge が
  同じ workflow を起動したための concurrency 自動キャンセル。後続 `f45ed29` の ci は success。

  ### ノート PC の自動更新 — 1 回落ちて、2 回目で入った (同日夜・user 報告)

  日中に失敗していた 0.10 → 0.11 が、夜に**同じインストーラで成功した**。2 回目は
  インストーラが起動して最後まで入っている。0.11.0 の資産は 8/24 の公開以来変わらないので
  **配布物は白**。ネットワークでもない — 回線で落ちるならダウンロード段で `UpdateNotice` が
  「失敗」を出して**アプリは残る** (`UpdateNotice.svelte:58-63`)。アプリごと消えたのは
  検証を通ってインストーラに制御が渡った後。1 回目と 2 回目の差は
  **インストーラのプロセスが起動したかどうか**なので、疑わしいのは実行を止める側
  (署名なし exe に対する AV の初回判定・別インスタンス・UAC)。**未確証**。

  再現しない 1 回きりの失敗で、リトライで解消した。恒久策は Authenticode 署名
  (`docs/roadmap.md:792`。有料証明書と repo secrets が要るので user 判断待ち)。

  **上の記述を訂正する**: 「また止まっても次の起動で画面に出る」は
  **0.11 → 0.12 には効かない**。パンくずを書くのは*走っている側*のアプリ
  (`apps/desktop/src-tauri/src/lib.rs:570` の `record_update_attempt`) で、
  そのコードが入ったのは 0.12.0。0.11 は何も書き残さないので、ここで止まっても無言のまま。
  お知らせが実際に効き始めるのは **0.12 → 0.13 から**。

  現在 user は 0.11、0.12 の案内が出ている状態。


> 2026-08-25 その4 〜 2026-08-25 (その1) のセッションログ (止まった更新の告知、
> ダイアログの取り回しと MCP 接続登録、サイドバーの横区切り、一覧から直に目印) も、
> 2026-08-26 に同じ場所へ全文退避した。

> 2026-08-24 のセッションログ (接続一覧 #192 の 3 条件、色 + タグの目印) も、
> 2026-08-25 に同じ場所へ全文退避した。
> 2026-08-21 その2 〜 2026-08-20 のセッションログ (history 書き換えと force push、
> 公開リポの実名掃除、v0.10.0 のリリース) も、2026-08-25 に同じ場所へ全文退避した。

> 2026-08-19 のセッションログ (検証シート 003 のために MCP へ 7 動詞を足した回) も、
> 2026-08-25 に同じ場所へ全文退避した。

> 2026-08-16 その2 〜 2026-07-31 のセッションログは、baseline §31 に基づき
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
