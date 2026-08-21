# プロジェクトステータス (内部メモ・日本語)

このファイルは作業中のセッション状態を記録する内部用ドキュメント。
外部公開向けの内容ではないため日本語で書く。セッション終了時に更新する。

## 最終更新

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
  open PR = 0、develop = main = `3d8434b`。**

  **2026-08-20 security-reviewer 実行：HIGH 1 / MEDIUM 1。HIGH は #209 (`bc98d60`) で
  対処、MEDIUM は #210 に分離して未着手。**

  **やったこと**: (a) v0.9.0 のリリース (#197 / タグ `v0.9.0` / `main` = `49634f3`)。
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
  `Ubuntu-font-1.0` が、egui クライアントを畳んだ `a2d92fa` 以降**死んだ許可のまま
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
  develop = `b97cda2`。**

  **やったこと**: (a) MCP に `set_editor_sql` / `run_query` / `open_ai_panel` /
  `open_ai_settings` (#185)、`get_ui_locale` / `set_ui_locale` (#182)、
  `capture_window` (#184) を追加。
  (b) クエリツールバーの反応が履歴のキャッシュで止まる不具合を修正 (#183)。
  (c) シート 003 を user が実施し 10 行すべて `OK` (#186)。
  (d) 三つのシートから `実施日` / `担当` の列を削除 (#187)。
  (e) Issue #181 起票。(f) #180 を `b97cda2` に rebase。

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

- 日付: 2026-08-16 その2 (**v1.0 ゲート 4 (コード署名) を「買わない」側で閉じた (#178)。
  残り 3 ゲート、全部 user 側。open PR = 0。develop = `d4c0c7f`。**

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
  #173 `develop` → `main` のリリース PR。`main` = `2a9b1e8`、タグ `v0.8.0` を push 済。

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
  `c316e9b` として復旧。**push 済みブランチに追加 commit を積んだら、その PR が
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

> 2026-08-09 〜 2026-07-31 のセッションログは、baseline §31 に基づき
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
