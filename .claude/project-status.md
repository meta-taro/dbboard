# プロジェクトステータス (内部メモ・日本語)

このファイルは作業中のセッション状態を記録する内部用ドキュメント。
外部公開向けの内容ではないため日本語で書く。セッション終了時に更新する。

## 最終更新

- 日付: 2026-08-25 その2 (**サイドバーの横区切りを上下に動かせるようにした。ADR-0131。push は user。**

  **別ブランチ** `feature/sidebar-panel-split` = `feature/connection-order` (#222) の上。
  #222 はもう CI green で画面も見てもらっているので、同じ枠をもう一度見てもらわないため。

  **依頼**: 「左の DB 接続とテーブルの区切りの横ライン、上下に移動できて W クリックで定位置。
  接続の登録数で状況変わりますが、良しなに」。

  **設計の本体は最後の一文**。登録数で変わるものに「定位置」があるとすれば、それは数値では
  なく規則の方なので、保存する値を `number | null` にした。**null = 一度も掴んでいない**で、
  その間は高さが接続の登録数から決まる (1 行 30px・下限 60px・上限 420px・サイドバーの半分を
  超えない)。だから接続を足せば黙って広がるし、一度掴めばその瞬間から user の指定が勝つ。
  **W クリックは定数を書き戻すのではなく null に戻す** — 3 件のときと 30 件のときで
  「ちょうどいい高さ」は同じ数値ではない。

  **分担は縦の splitter (ADR-0083) をそのまま踏襲**。数値の規則だけ
  `apps/desktop/src/lib/layout/panel-split.ts` の純関数に切って**テスト 19 本** (先に落ちるのを
  確認済み)、ポインタの配線・`ResizeObserver`・`localStorage` の呼び出しは `Sidebar.svelte` に残す。
  DOM のテストは 1 本も書いていない。`role="separator"` + `aria-valuenow` + 矢印キー (16px) +
  `Home` でリセットも縦と同じ。

  **ついでに前からあったバグが消えた**: 接続一覧に高さも `overflow` も無かったので、
  接続が増えるとテーブル一覧が窓の下へ押し出されていた。区切りに上限が付き、
  一覧側が `overflow-y: auto` になったので、どちらも消えた。**型検査にも単体テストにも
  映らない種類の壊れ方**で、区切りを動かせるようにしなければ気づかないままだった。

  **見た目は変えていない**: 掴み代は 7px だが `margin: -3px 0` で前の 1px 罫線と同じ間隔に
  収まり、線そのものは `::after` の 1px。hover / フォーカス / ドラッグ中だけアクセント色になる。

  **検証**: `pnpm check` 0 error 0 warning (319 ファイル) / vitest 36 ファイル **597 pass**
  (+19) / `scripts/*.test.mjs` 5 本すべて green / pre-commit ゲート
  (pii-scan → fmt → clippy → check → cargo test) を `--no-verify` なしで通過。
  `pnpm tauri build` も通し、exe を差し替えて起動してある。

  **§31 の棚卸しも実施**: `.claude/next-actions.md` が 454 行で 400 行トリガを踏んだので、
  2026-08-24 の その5 / その6 / その7、候補 A、接続名サニタイズ節、完了済みの user 側 5 行を
  `.claude/archive/next-actions-2026-08.md` へ**全文退避** (要約していない)。400 行に戻した。

  **AI がやれていないこと**: 画面で見ること、push、merge。→ **user 側**
  (§38 の public リポ除外により、こちらから催促はしない)。)

- 日付: 2026-08-25 (**目印を一覧から直に付けられるようにし、色を行の左先頭へ移した。ADR-0130。push は user。**

  ブランチは引き続き `feature/connection-order` (#222)。**目印そのものが #222 の中身**なので
  別ブランチにはしていない — 分けると同じ枠を 2 回見てもらうことになる。

  **きっかけは初日の使用**。ADR-0126 の目印には 2 つ不満が出た。
  ① 色を変えるのに編集フォームを開く必要がある。フォームには DSN・トンネル・MCP 権限も
  入っているので、**見た目だけの決定に接続まるごとの保存**が要る。6 件塗り分ければ 6 回。
  ② 目印が行の**末尾**にある。名前の終わる位置は行ごとに違うので列にならず、
  一覧を上から目で走らせる役には立たない。

  **やったこと 3 つ**:
  ① `ConnectionAdmin::set_mark` — 色とタグだけを書く。両方を書く前に両方を検証するので、
  タグが不正なときに色だけ入る状態にならない。既に同じ内容なら**ファイルを書き直さない**。
  テスト 10 本 (先に落ちるのを確認済み)。
  ② `set_connection_mark` コマンド → `api.ts` → `workspace.setMark`。
  ③ `MarkPicker.svelte` — 接続行を右クリックで開く色見本のポップオーバー。
  見本を 1 つ押せばその場で塗って閉じる。タグは Enter か閉じるときに 1 回だけ書く
  (1 文字ごとだと打鍵のたびにファイル書き込みになる)。

  **色は行頭 3px の帯**。未設定の行でも幅は取るので、印を付けても名前が横にずれない。
  選択行の左端の 2px は inset shadow で行の**外周**、帯は padding の内側なので重ならない。
  サイドバーのピル (`ConnectionMark`) からは丸だけ落とした (`dot={false}`) — 1 行に色見本が
  2 つ出るのは ADR-0126 が守ろうとした不変条件ではない。接続マネージャ側は行頭の帯が無いので
  そのまま。

  **ADR-0126 を 1 点だけ緩めた**: `set_mark` は**タグ無しの色を受け付ける** (フォームは
  今も拒む)。0126 が裸の色を禁じたのは、色覚・グレースケール・スクリーンリーダのどれにも
  何も伝わらないから。**行頭の帯は接続名を持つ行の上に乗る**ので意味は既にそこにある。
  `markFor` は変えていないので、タグの無い色は今も色名がタグの位置に出る (打てという合図)。

  **ついでに直した**: 絞り込み中の掴み手が、掴めないのにホバーで光り握り拳カーソルを出す件。
  `.row:hover .grip` (0,4,0) が `.grip:disabled` (0,3,0) に勝ち、`:active` は同点で後勝ちだった。
  順序ではなく `:not(:disabled)` で塞いだ (順序は次の編集で崩れる)。

  **検証**: `pnpm exec svelte-check` 317 files / 0 errors / 0 warnings、`vitest` 578 passed
  (35 files)、`cargo fmt --check` / `clippy -D warnings` / `cargo check --all-targets` /
  `sh scripts/cargo-test-serialised.sh` すべて緑。CHANGELOG は `## [Unreleased]` に 2 件
  (目印の直接編集・掴み手カーソル) を足して計 5 件。

  **AI がやれていないこと**: 画面で見ること。**user 側のボール**はサイドバー —
  接続行を右クリックして色見本が出るか / 選んだ色が行の左先頭に帯で出るか /
  無印の行と名前の位置が揃っているか。それと **#222 の merge** と **push**。

  **次**: サイドバーの横区切りを上下ドラッグ + W クリックで既定位置 (2026-08-24 依頼)。
  **別ブランチ**にする — #222 は既に green で人も見ている。)

- 日付: 2026-08-24 その2 (**接続に色 + 短いタグの目印を付けた。commit `4e0e6d9`、push は user。**

  ブランチは引き続き `feature/connection-order` (#216 の上)。これで 0026 の D が閉じ、
  **v0.12「接続一覧を操れるようにする」の中身は 4 つとも実装済**になった。

  **何のためのものか**: いま繋いでいるのがどのサーバかを、一目で言えるようにする。
  これまでの答えは接続名だけで、名前は説明的であることを目的に付けるので**見分けるのには
  向いていない** — "shop-a" と "shop-a (staging)" は隣に並ぶと同じものに見える。

  **計画 (0026 D) の 3 番目の条件を広げた。** 計画は「色だけでは目印にならない
  (色覚・グレースケールのスクリーンショットで消える)」とだけ書き、非色成分の案として
  「短いタグ文字列」か「アイコンの形」を並べていた。**タグを採り、さらに必須にした。**
  色名をテキストで出す案は条件を字面どおり満たすが役に立たない — 接続の隣に「赤」と
  出ても、その接続の名前より情報が少ない。運用者が書く `prod` / `dev` が、色が
  立て替えているだけの意味そのものを運ぶ。

  したがって **フォームは色だけのマークを保存しない**。ただし手で書いた config が
  色だけを持っていた場合は、**色名にフォールバックして描く** — 描かないと、ファイルには
  色があるのに行は無印に見える。この 2 つは矛盾ではなく、`markNeedsTag()` (保存側) と
  `markFor()` (描画側) に分けてそれぞれテストしてある。

  **中身**: パレットは 8 色、`tokens.css` に 1 箇所だけ定義し**全テーマに値を持つ**
  (`:root` / `prefers-color-scheme: dark` / `data-theme` の light・dark)。config に入るのは
  hex ではなく**色の名前**なので、後からテーマを差し替えられるし、変な値がファイルに残らない。
  タグは 12 文字まで。`color` と `tag` はどちらも `mcp_alias` (ADR-0088) と同じ 3 状態
  (`None`=据置 / `Some(v)`=設定 / `Some("")`=消去) で編集経路を通る。

  **壊れ方を先に塞いだ 2 つ**:
  ① マークアップは `ConnectionMark.svelte` 1 つに閉じた。サイドバーと接続マネージャが
  別々に描いていると、片方だけ直して**半分のマーク**になる。構造的に不可能にした。
  ② `crates/dbboard-config/tests/mark_drift.rs` が、Rust 側パレット / フロント側パレット /
  テーマトークン / 2 つのタグ長制限の 4 つを突き合わせる。どれかがずれたらビルドで落ちる。

  **テスト**: `marks.test.ts` 18 件、`mark_drift.rs` 4 件、Rust 側の正規化と `TagTooLong`。
  いずれも**先に落ちるのを確認してから**実装。`pnpm check` 314 files / 0 errors、
  `pnpm test` 568 passed (34 files)、`cargo fmt --check` / `clippy -D warnings` /
  `sh scripts/cargo-test-serialised.sh` すべて exit 0。pre-commit ゲート通過
  (`--no-verify` なし)。

  **途中で潰した 2 つの clippy**: `store.rs` の round-trip テストが `color`/`tag` の追加で
  `too_many_lines` (112/100) になったので `#[allow]` ではなく `plain(id, name, kind)`
  ヘルパに畳んだ。`lib.rs` の `to_add_draft` が `too_many_arguments` (8/7) になったので
  `MarkInput { color, tag }` 構造体にした — このファイルの既存の `#[allow]` は
  「引数リストがワイヤ契約そのもの」という理由で正当化されており、内部ヘルパには当てはまらない。

  **ドキュメント**: ADR-0126、DESIGN.md の identity colours 節、0026 の D に「計画から
  何を変えたか」を追記。

  **CHANGELOG はまだ書いていない**。理由は下の 2026-08-24 の項と同じ — 唯一の
  `## [Unreleased]` は v0.11 の枠で、これは v0.12 の中身。v0.11.0 を切った**後**に書く。

  **AI がやれていないこと**: 画面で見ること。**user 側のボール**は接続フォーム
  (色セレクトの左端に色が出ているか、タグ入力とプレビューが 1 行に収まっているか) と、
  サイドバー・接続マネージャの一覧 (マークが名前を押しのけていないか)。)

- 日付: 2026-08-24 (**接続一覧 #192 の 3 条件がすべて実装できた。commit 2 本、push は user。**

  **ブランチは `feature/connection-order`。`feature/duplicate-and-repair-connection`
  (PR #216) の上に積んである** — 依存する `ConnectionManager.svelte` の分割が #216 の中に
  あり、user がこれから 10 枚見て merge するブランチを今いじると、見た直後に中身が変わるため。
  したがって **#216 の merge が先**で、この 2 本はその後。

  **① ▲▼ で並び替え (`917aa09`)**。`[[connections]]` は TOML の array of tables =
  **順序はもう保存されている**ので、並び替えは Vec を並べ替えて書き戻すだけ。スキーマ変更も
  `CONFIG_VERSION` の bump も不要で、古いビルドでも読め、`.dbbx` バンドルにもそのまま乗る。
  `ConnectionAdmin::move_to(id, index)`。範囲外の index は **clamp せず `IndexOutOfRange`
  で error** — clamp すると operator が指していない場所に置かれる (ADR-0016 と同じ判断)。
  同じ index への移動は no-op でファイルを書き直さない。keyring に触らないのでアダプタの
  キャッシュも evict しない。Tauri 側 `move_connection`、フロント側 `moveTarget()`。

  **② 名前 / id で絞り込み (`8f38abb`)**。`filterConnections()` は空白区切りの語を
  **すべて**含む行だけ返す (2 語目が絞り込みになる)。**kind はわざと対象外** — 「my」で
  "my shop" を探すと MySQL の行が全部返り、絞り込みの逆になる。id を対象に含めたのは、
  ログやエラーメッセージから貼れる唯一のハンドルだから。空クエリのときは**同じ配列参照を
  返す** (keyed `{#each}` が毎キーストロークで作り直されないように)。

  **計画になかった衝突を 1 つ見つけた**: ①と②は干渉する。▲▼ は*保存された*リストの中で
  動くので、行が隠れている間は見えない行を飛び越える。**絞り込み中は ▲▼ を disabled** に
  し、tooltip で理由を出した。「見えている次の行の下へ」は別の機能で、黙って違う答えを
  返すより disabled の方がまし。

  **テスト**: `move_to` 5 件 (Rust)、`moveTarget` 4 件・`filterConnections` 6 件 (vitest)。
  いずれも**先に落ちるのを確認してから**実装。`cargo check --all-targets --all-features` と
  `sh scripts/cargo-test-serialised.sh` が exit 0、`pnpm check` 311 files / 0 errors、
  `pnpm test` 546 passed (33 files)。pre-commit フックは 2 本とも通過 (`--no-verify` なし)。

  **CHANGELOG はまだ書いていない。意図的。** 唯一の `## [Unreleased]` は v0.11
  「接続の複製と修復」の枠で、`release-cut.mjs` はその本文をまるごと切ったバージョンへ
  移す。並び替えと絞り込みは **v0.12「接続一覧を操れるようにする」**の中身なので、
  v0.11.0 を切って `## [Unreleased]` が v0.12 の枠になってから書く。先に書くと
  v0.11.0 の内容として下に落ちる。**#192 の close も develop に入ってから**。

  **AI がやれていないこと**: 画面で見ること。→ user 側: 接続マネージャの一覧 1 枚
  (▲▼ が行の右端に収まっているか、絞り込み入力が下のテーブル検索と見分けが付くか、
  絞り込み中に ▲▼ が灰色になるか)。)

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
