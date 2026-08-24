# プロジェクトステータス (内部メモ・日本語)

このファイルは作業中のセッション状態を記録する内部用ドキュメント。
外部公開向けの内容ではないため日本語で書く。セッション終了時に更新する。

## 最終更新

- 日付: 2026-08-21 その2 (**history 書き換えを実行し、force push まで完了した (push は user)。
  公開履歴から実名と個人メールが消えた。GitHub 側に消せない残りが 1 種類ある。**

  **1 パスでは終わらない。3 パス要る。** `--replace-text` は **blob の中身だけ**で
  commit message を触らない。1 パス目が「成功」と出た後、2026-07 の commit message が
  実名を 2 件抱えたまま残っていた。`--mailmap` が author/committer (ADR-0084)、
  `--replace-message` が commit / tag message。**検証は ref ではなく
  `git cat-file --batch-all-objects` で全オブジェクトを 1 回走査する** —
  到達不能な残骸も含めて 0 件、旧アドレスのリテラル一致も 0 件。

  **`refs/pull/` は消せない。** heads + tags = 644 commit で清潔だが、`--all` = 1320 commit。
  **197 本の PR ref にぶら下がった旧 commit 676 本**が実名と旧メールを持ったまま残る。
  **GitHub は `refs/pull/` への書き込みを拒否する**ので `push --force --mirror` は
  失敗する (explicit refspec で回避)。**`for-each-ref 'refs/pull/*'` は 1 件も
  マッチしない** — 3 階層なので `'refs/pull/'` が要る。消せるのは GitHub Support への
  依頼だけで、依頼はアカウント所有者から出す必要がある = **user 側ボール**。

  **削除して作り直す案は却下。** PR 214 本と、ADR が番号で参照している issue が消える。
  塞げるのは「PR ref を意図的に列挙した人だけが辿れる穴」で、fork 0 / star 0 / watcher 0。
  clone・`git log`・blame・Web UI・tag・release はすべて清潔になっている。

  **追跡ファイル内の旧ハッシュ参照 427 件 (217 ユニーク / 21 ファイル) を付け替えた。**
  1 回目は**対応表そのものが誤りで、217 件全部が実在しないハッシュ**になっていた。
  原因は 2 つ。**filter-repo の `commit-map` は 2 回目以降に自動合成される** —
  最後のパスの map が既に original→final で、手で連鎖させるとどこにも無い中間ハッシュが
  できる。**`git rev-parse --short=7 <40桁>` はオブジェクトの実在を検証しない** —
  それらしい短縮形が返るのでエラーにならない。検証を `cat-file -e "${short}^{commit}"` に
  変えて作り直し、**HEAD の blob に同じ置換を当て直して作業ツリーとバイト比較**して確定
  (改行正規化後 21/21 一致・369 insertions / 369 deletions)。

  **ローカルの後始末**: stale な 48 branch は**削除せず `refs/pre-rewrite/` へ退避**した。
  到達可能なまま `push --all` の対象外になり、§30 の削除ゲートも踏まない。
  `refs/heads` は `develop` のみ。**`C:\claude\_dbboard-rewrite\pristine.git` と
  `C:\claude\_dbboard-prerewrite-backup\` は実名入り** — 絶対に push しない。
  前者は旧→新ハッシュ対応表の唯一の出所なので消さない。

  `docs/maintainer/history-sanitize-runbook.md` を実態に合わせて全面改訂した
  (3 パス構成 / `refs/pull` の 2 つの罠 / `--mirror` が失敗すること / `pii-scan.sh` の
  自己テスト固定文字列がメールドメインの grep を誤爆させること / このリポには
  再設定すべきブランチ保護がそもそも無いこと)。

  **未完 (user 側)**: ① GitHub Support への `refs/pull/` + 到達不能オブジェクト purge 依頼、
  ② `develop` / `main` に保護もルールセットも無い件の扱い、③ 公開 `.exe` の PII 目視確認。

- 日付: 2026-08-21 (**公開リポに実接続名が出ていた。全 issue + 全 PR を掃除したが、
  GitHub の編集履歴に旧版が残っており、そこは画面からしか消せない。権限が足りず未完。**

  **やったこと**: (a) issue 16 件 + PR 196 件を dump して実名を grep、**7 箇所**
  (issue #193 / #131 / #161 の本文、#142 と #161 のコメント、PR #63 / #58 の本文) を
  プレースホルダへ置換し、置換した旨の注記を付けた。置換後に再 grep して 0 件を確認。
  (b) #193 で約束していたフォローアップを **#213** として起票。(c) 編集履歴の
  リビジョン削除をブラウザから試み、権限で止まった。

  **`gh issue list` は PR を返さない。** issue だけ見ると PR #63 / #58 を落とす。
  掃除のコマンドは `gh issue list` と `gh pr list` を **両方** `--state all` で
  dump して grep する形にした (実名の一覧はリポジトリ外の memory にしかない)。

  **本文を直しても消えていない。** GitHub は編集すると旧版が `edited` の
  プルダウンから誰でも読める。**リビジョン削除に API の口が無く** (`gh` にも REST にも
  無い)、画面から 1 件ずつ消すしかない。7 箇所ぶん残っている。**ここを消さない限り、
  今日の作業は表示が変わっただけ。**

  **止まった理由は権限**: `gh api repos/meta-taro/dbboard/collaborators/dokokade/permission`
  = `read` (admin / maintain / push すべて false)。リビジョン削除には write が要る。
  接続されている Chrome 拡張は `dokokade` が 1 つとログアウトが 2 つで、**`meta-taro` で
  入っている窓が 1 つも繋がっていなかった。**サインインは user 側の作業
  (エージェントは認証情報を入力しない)。

  **`.pii-denylist` がこのマシンに無いので、`pii-scan` は実名を見ていない。**
  今回の 7 箇所は CI では検出されない種類のもので、見つかったのは手で grep したから。
  公開アナウンスの前に毎回この掃除を回す。

  **#213 の中身**: #194 は foreign keyring ref の**検出**まで。#213 はその手前と後ろ —
  **同じ資格情報を使う接続をもう 1 つ作る正規の手順が無い** (だから `connections.toml` を
  手で触ることになり、壊れた状態が黙って生まれる)、**できてしまったものを直す手順も無い**
  (編集画面から入れ直しても ref は故意に維持される。削除して追加し直すしかないが、
  トークンは OS の資格情報ストアからアプリ経由で読み出せない)。

  **未完 (user 側)**: ① `meta-taro` の窓でサインイン → 7 箇所のリビジョン削除、
  ② 旧コミットに残る実名の history 書き換え (`git filter-repo` + force push・破壊的)。

- 日付: 2026-08-20 (**v0.10.0 を公開した。リリース前セキュリティレビュー (baseline §24) を
  回したところ、設定してあるのに誰も走らせていない check が 1 つ出てきた。
  open PR = 0、develop = main = `7540b90`。**

  **2026-08-20 security-reviewer 実行：HIGH 1 / MEDIUM 1。HIGH は #209 (`8d506b7`) で
  対処、MEDIUM は #210 に分離して未着手。**

  **やったこと**: (a) v0.9.0 のリリース (#197 / タグ `v0.9.0` / `main` = `3527998`)。
  (b) v0.10.0 のリリース準備 #207、ロードマップの帳簿修正 #208、
  セキュリティ #209 を develop へ。(c) リリース PR #211 (`develop` → `main`) をマージ。
  (d) タグ `v0.10.0` を push、release run `32366885046`。(e) Issue #210 を起票。
  (f) `.claude/next-actions.md` の棚卸し (baseline §31、425 行 → 288 行)。

  **HIGH の中身 — 「失敗する check」ではなく「誰も走らせない check」だった**:
  `cargo deny` はこのリポジトリに前から設定してあり、`CLAUDE.md` にもセキュリティ体制の
  一部として名前が書いてあった。**どのワークフローも走らせていなかった。**
  赤いまま何か月も経っていた (advisory 21 件・ライセンス 4 件)。見つかったのは
  リリース前レビューがたまたま手で叩いたからにすぎない。`.claude/next-actions.md` の
  候補 D には「commit フックではないので緊急ではない」と書いたまま放置してあった。

  対処は 3 段。**構造**: `develop` / `main` への push と PR で走る `deps` ジョブ
  (GTK もフロントエンドビルドも要らないので `rust` から独立、cargo-deny をピン留めして
  キャッシュ = 同一ブランチで 15 秒)。**直せたもの**: `h2` → 0.4.17、ライセンス許可の
  追加 2 件・削除 2 件。**直せないもの**: advisory 1 件につき 1 エントリで理由を書いて
  `deny.toml` に記録 (ADR-0117)。

  **`deps` ジョブは初回実行で 1 件見つけた**: `deny.toml` の `OFL-1.1` /
  `Ubuntu-font-1.0` が、egui クライアントを畳んだ `af17200` 以降**死んだ許可のまま
  残っていた**。ADR-0117 が「ignore リストは見える形で腐る」と主張する PR の中で、
  その主張が最初の機会に自分で発火した。

  **リリースは止めない判断**: v0.10.0 が remote transport を入れたことで
  `hyper-rustls` 0.25 が入り、`rustls-webpki` 0.102 と `h2` 0.3 が固定される。
  修正は全部それらが到達できないメジャー系列に載っている。唯一の逃げ道である
  libsql 0.10 は pre-release で、**pre-release の DB エンジンを署名済みバイナリに
  入れる方がリスクが大きい**。理由は「ここでは到達しない」「到達するが狭い」
  「この製品の操作ではない」を区別して書いた。**6 件中 4 件は libsql が
  上がった瞬間に消える。**

  **MEDIUM (CSP) は分離した**: `"csp": null` = ポリシー未注入。検証したところ
  フロントエンドに HTML 注入口は 0 件 (`{@html}` / `innerHTML` 無し) で急ぎではなく、
  一方 `app.html` のテーマ適用インラインスクリプトと SvelteKit のハイドレーション
  スクリプトがあるため `script-src` は `kit.csp` の設定が要る。**タグを打つ日に
  混ぜる変更ではない**ので #210 に出した。

  **今回は `--no-verify` を使っていない**: タグ push の直前に `target/release` を
  掴むプロセスを確認して 0 だったため、pre-push の `cargo build --release` が
  v0.9.0 のときのように `LNK1104` で落ちなかった。**ロック確認を push 前の手順に
  入れたのが効いた**ので、次回も同じ順序で行う。

  **エージェント側の訂正 1 件 (記録)**: `deps` は 15 秒と伝えたが #211 では 1m52s
  かかった。**GitHub Actions のキャッシュはブランチスコープ**で、feature ブランチで
  作ったキャッシュは `main` を base にした PR から復元できない。

  **残り (user 側)**: ① 公開 `.exe` の PII 目視確認 (CI はやらない。このマシンに
  `.pii-denylist` が無いのでスキャナは本名を見ていない)、② v1.0 の残り 3 ゲート、
  ③ §36 改善要望シートの記入 (**エージェント代筆禁止**)。
  **ディスクが 9.5 GB しか無い**ので、検証シート 002 / 001 で Docker を使う前に確認。)

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
