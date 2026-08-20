# 次のアクション案内 (user 側ボール常設ファイル)

このファイルは「セッションが切れても、開いた瞬間に user 側のボールがわかる」状態を維持するためのもの。
セッション終了時 / 状況が変わった時に必ず更新する。**日本語可・内部用。**

---

## 最終更新

- 日付: 2026-08-20 (**v0.10.0 を出した。タグ push まで完了、release CI 実行中。
  リリース前セキュリティレビューを回したら、設定してあるのに誰も走らせていない
  check が 1 つ見つかった。open PR = 0。**
  ① **リリース経路**: #207 (リリース準備) → develop、#208 (ロードマップ帳簿) →
  develop、#209 (セキュリティ) → develop、#211 (`develop` → `main`) マージ済 →
  `main` = `3d8434b`、`main..develop` = 0。`main` の CI は 4 ジョブ + pii-scan すべて緑。
  タグ `v0.10.0` (annotated) push 済 → release run `32366885046`。
  **今回は `--no-verify` を使っていない。** タグ push の直前に `target/release` を
  掴むプロセスを確認して 0 だったため、pre-push の `cargo build --release` が
  v0.9.0 のときのように `LNK1104` で落ちなかった。**ロック確認を push 前の手順に
  入れたことが効いた**ので、次回も同じ順序で行う。
  ② **`cargo deny` は設定してあったのに、どのワークフローも走らせていなかった
  (ADR-0117)。** `CLAUDE.md` にセキュリティ体制の一部として名前が書いてあるのに、
  赤いまま何か月も経っていた (advisory 21 件・ライセンス 4 件)。見つかったのは
  リリース前レビューがたまたま手で叩いたから。**失敗する check よりも、
  誰も走らせない check の方が問題だった。** `develop` / `main` への push と PR で
  走る `deps` ジョブを足した。GTK もフロントエンドビルドも要らない
  (`cargo metadata` はコンパイルせずグラフを解決する) ので `rust` から独立させ、
  cargo-deny をピン留めしてキャッシュ = **同一ブランチで 15 秒**。
  ③ **`deps` ジョブは初回実行でいきなり 1 件見つけた。** `deny.toml` の
  `OFL-1.1` / `Ubuntu-font-1.0` が、egui クライアントを畳んだ `a2d92fa` 以降
  **死んだ許可のまま残っていた**。ADR-0117 が「ignore リストは見える形で腐る」と
  主張する PR の中で、その主張が最初の機会に自分で発火した形。
  ④ **直せないものは advisory 1 件につき 1 エントリで理由を書いた。**
  v0.10.0 が remote transport を入れたことで `hyper-rustls` 0.25 が入り、
  `rustls-webpki` 0.102 と `h2` 0.3 が固定される。修正は全部それらが到達できない
  メジャー系列に載っている。理由は「ここでは到達しない」「到達するが狭い」
  「この製品の操作ではない」を区別して書いた。**6 件中 4 件は libsql が上がった
  瞬間に消える。** 一括抑制はしない (次の 1 件も隠れるため)。
  **リリースは止めない**判断。上流に修正版が無く、唯一の逃げ道である libsql 0.10 は
  pre-release で、pre-release の DB エンジンを署名済みバイナリに入れる方がリスクが大きい。
  ⑤ **CSP は分離した (#210)。** レビューは `"csp": null` も MEDIUM で挙げたが、
  検証したら**フロントエンドに HTML 注入口が 0 件** (`{@html}` / `innerHTML` 無し) で
  急ぎではなく、しかも**直接 `fetch` も 0 件** (通信は全部 Rust 側) なので
  `connect-src` は軽い。一方 `app.html` のテーマ適用インラインスクリプトと
  SvelteKit のハイドレーションスクリプトがあるため `script-src` は
  `svelte.config.js` の `kit.csp` 設定が要る。**タグを打つ日に混ぜる変更ではない。**
  ⑥ **エージェント側の訂正 1 件 (記録)**: `deps` は 15 秒と伝えたが #211 では 1m52s
  かかった。**GitHub Actions のキャッシュはブランチスコープ**で、feature ブランチで
  作ったキャッシュは `main` を base にした PR から復元できない。`develop` に
  キャッシュができた今は定常 15 秒。
  **user 側ボール = ① 公開 `.exe` の PII 目視確認 (CI はやらない。このマシンに
  `.pii-denylist` が無いのでスキャナは本名を見ていない)、② v1.0 の残り 3 ゲート
  (下記 候補 0)、③ §36 の改善要望 3 件の記入 (下記)、④ 従来からの継続分。**
  次のエージェント側タスク = **無し**。)

- ※ 2026-08-19 その2 〜 2026-08-03 のエントリは
  `.claude/archive/next-actions-2026-08.md` へ全文退避 (baseline §31、退避日 2026-08-09 /
  2026-08-14 / 2026-08-16 / 2026-08-20 の 4 回)。さらに古いもの (2026-07-29 以前) は
  `.claude/archive/next-actions-2026-07.md`。


## モード

**in-use / continuous-improvement (menu-not-sequence)** — 2026-06-24 以降。
配布 (#14) は 2026-07-16 に完了済、DL ページも live。今は「配布済 exe を担当が
実際に使うか」を update-check で観測しつつ、次の実利用改善 (下記の user 側ボール) を
**ロードマップ順ではなく摩擦順**に進めるフェーズ。**ロードマップは順序ではなく献立**で、
実際に使って出た困りごとが常に優先する。

**最新リリース = v0.10.0 (2026-08-20)。** アダプタは 11 kind
(ワイヤ id は 9 — `turso-remote` は `turso` として名乗るのでコントラクト非破壊)。
v1.0 の定義は「機能が出揃うこと」ではなく **`docs/api-contract.md` を壊さない約束**
(ADR-0011) なので、エンドポイントやフラグの追加は additive = 1.0 を妨げない。
実際にゲートになるのは候補 0 の 3 つだけ。

---

## user 側のボール (= 次に着手する時の選択肢)

### ★★ 候補 0: v1.0 の残り 3 ゲート (`.claude/issues/0021-v1-0-criteria.md`)

4 つのうち **ゲート 4 (署名) は 2026-08-16 に ADR-0106 で決着済**。残り 3 つは
いずれも baseline §38 の「人にしかできない工程」で、**エージェント側から代われない**。
1.0 を出す気があるなら、ここが最優先。

1. **#161 の 3 点観察** — Run ボタンがクリックに反応しない。観察するのは
   ボタンの色 / カーソル形状 / 一度別の場所をクリックしてからだと効くか。
   原因が特定できていない段階で当て推量のテストは書かない、が方針。
2. **コントラクトを `dbboard-web` へミラー** — **user 判断で保留中**。
   凍結の**前**にやる必要がある (凍結後にミラーすると、web 側が別物を
   実装していた場合に破壊的変更が要る)。ミラー不要なら
   [[feedback-explicit-no-op-brief]] のとおり**その旨を明示的に言う**。
   黙って保留すると web 側が待ち続ける (過去に 3 週間止めた)。
3. **検証シート 001 / 002 の実施** — **003 は 2026-08-19 に完了** (10 行すべて OK、#186)。
   残るのは 002 (MongoDB) → 001 (Firestore) の順。どちらも Docker が要る。
   人が動かして `結果` を埋める (baseline §22)。**エージェントが `OK` を
   書き込むことは禁止**。`実施日` / `担当` の列は #187 で落としたので記入不要。
   環境的に無理な行は `未実施` のまま残してよい。
   002 は `docker run -d --rm -p 27117:27017 mongo:8` で足りる。
   001 のエミュレータ停止は
   `docker compose -f docker/firestore-emulator/compose.yaml down`。
   **シートに資格情報を書かない** (サービスアカウント鍵のパス / MongoDB URI は
   シートに入れない)。**接続名も入れない** (実接続名はリポにもシートにも入れない)。
   **⚠ ディスクが 9.5 GB しか無い。** `target/release` が 23 GB を占めており、
   Docker イメージを引くと足りなくなる可能性がある。実施前に空きを確認する。

### 候補 A-4: `.github/dependabot.yml` で actions の追従を自動化 (小・要否は user 判断)

#176 で `actions/checkout` を v4 → v6 に上げたが、**最新は v7.0.1** で 1 メジャー
遅れのまま。手で追うと今回のように気づかず何メジャーも離れる。`github-actions`
エコシステムを dependabot に登録すれば PR が自動で来る。

**トレードオフ: PR が増える。** 今日「push が多い」と感じた直後にこれを入れると
体感が悪化しうるので、要否は user 判断として保留した。入れるなら `weekly` +
`open-pull-requests-limit` を小さくするのが現実的。

### ★ 候補 A: 実利用摩擦の次テーマ (menu-not-sequence)

直近 3 PR (DL ページ / ソート / MSI ショートカット) はいずれも実利用で挙がった
摩擦。次も同様に「実際に使って気づいた困りごと」を摩擦順に拾う。未着手候補は
Saved queries / Schema diff (下記 候補 E。Export results は CSV/JSON 済)。新しい
write 経路を伴うものは着手前に ADR。

**いま open で、着手できるもの:**

- **#196** — パスフレーズをワイヤに乗せない MCP エクスポート verb。
  **#201 が develop に入ってブロックが外れた** (`ExportReportDto { exported,
  foreign_refs }` が揃った)。
- **#192** — 接続リストの UX / 並べ替え。
- **#181** — **11 ロケール中 9 つが 334 キー中 30 キー (9%) しか訳されていない。**
  切替は動くが中身が英語のまま。003 のシートを通す過程で見えたもので、
  シートが通ったことより大きい。
- **#195** — `dbboard-mcp.exe` に更新経路が無い。

### 候補 A-5: §36 改善要望シートの記入 (**user のみ・エージェント代筆禁止**)

`docs/feedback/improvement-request.tsv` に未記入の気づきが 3 件ある。
**baseline §36 により、行を起こすのは実際に使った人**で、エージェントは
代筆も要約もしない (`状態` と `関連Issue` の 2 列だけ触ってよい)。
「何をしていたか」「どうなったか」の 2 つが埋まっていれば受け付けるので、
原因や直し方は書かなくてよい。「どうなってほしいか」は分からなければ空欄で構わない。

覚えとしての 3 件 (**これは私の控えであって、シートの行ではない**):
① ダブルクリックで開くダイアログが読み取り専用になる、
② MCP 経由で AI パネルを閉じる手段が無い、
③ md-business で連続更新が畳まれる + フリーズする。

### 候補 A-2: README に MSI アンインストール残留の明文化 (小・任意)

MSI アンインストールは `%APPDATA%\dbboard\dbboard\` の設定と Windows 資格情報
マネージャーのエントリを残す (仕様)。ユーザに口頭で伝えた `cmdkey` +
フォルダ削除のクリーンアップ手順を README か `docs/` に明文化する小 chore。

### 候補 A-3: アップデート通知の「変更点」が定型文 — **完了 (v0.10.0 / ADR-0115)**

`latest.json` の `notes` をタグ名から組み立てていたため、通知の「変更点」が
`dbboard v0.8.0. See the release page for the full changelog.` のような定型文になり、
何が変わったかが読めなかった。`scripts/release-notes.mjs <changelog> <bare-version>` で
`CHANGELOG.md` の当該バージョン節から**最初の `###` までのリード段落**を抜いて入れる。

**次のリリースで気をつける点**: 通知に出るのは**リード段落だけ**なので、
`CHANGELOG.md` を書くときに**その段落だけで意味が通る**ようにする
(`### Added` 以降を読まないと分からない書き方をすると、通知が中途半端になる)。
v0.9.0 → v0.10.0 の配信が、この経路の初めての実地確認になる。

### 候補 B: git 履歴の一括サニタイズ (human ボール・破壊的・未実行)

**1 回の rewrite で 2 つが同時に片付く。** どちらも「ファイルなら次のコミットで
直せるが、既に公開された過去コミットは書き換えないと消えない」もの:

1. **実店舗名** — 過去コミットに残る (実名は**非公開メモリと `.pii-denylist`
   のみ**。ここには書かない。対応表からローカルで `replacements.txt` を作る)。
   バイナリは CI ビルドで名前を含まないためリリースは塞がない。
2. **コミット identity** — 公開済コミットの一部の author/committer が個人 Gmail
   (ADR-0084)。**未公開のローカル 28 コミットは書き換え済** (2026-07-31、
   force-push 不要だったので実行した)。以後の新規コミットも noreply で clean。
   残るのは origin 上の分だけ。**「468 コミット」と書いていたのは 2026-08-09 時点の
   `main` の総数**で、現在は 585 (v0.10.0 時点)。汚染されているのはその一部なので、
   着手時に `git log --format='%ae %ce' origin/main | sort -u` で実数を数え直す。

手順は `docs/maintainer/history-sanitize-runbook.md` (Step 1-3 = 文字列置換、
Step 3b = `--mailmap` で identity、Step 4 = force-push)。全ハッシュ変更・既存
クローン/PR/フォーク破損のため **human 実行**。fork 0 / star 0 なので実効性は
ある = 検討する理由になるが、勝手に実行する理由にはならない。

**順序を間違えると全部無駄になる:** rewrite + force-push は**未 push の
ローカル作業を全部 push してから**やり、その後クローンを捨てて re-clone する。
先に rewrite すると、残った未書き換えのローカルコミットを次の `git push` が
そのまま remote に戻して再汚染する (git から見れば単なる新規コミットなので
警告も出ない)。`git pull --rebase` では直らない。runbook の「Ordering」節参照。
**2026-08-20 時点で open PR は 0 本**なので、壊れる PR は無い (以前ここに書いてあった
#125 は CLOSED)。着手前に `gh pr list --state open` で 0 を確認すること。

### 候補 C: release.yml の publish 自己作成化 — **完了 (v0.5.0)**

publish ステップが `gh release view <tag> || gh release create <tag>` になり、
タグ push だけでリリースが完結するようになった。v0.8.0 / v0.9.0 のリリースは
この経路で実行済み。**残るのは公開 `.exe` の PII 目視確認だけで、これは CI がやらない
人間の作業。**[[project-release-ci-needs-release-object]]。

### 候補 D: cargo-deny の既存ドリフト対応 — **完了 (v0.10.0 / ADR-0117)**

**「commit フックではないので緊急ではない」と書いたまま何か月も放置し、
その間ずっと赤かった。** リリース前セキュリティレビューが手で叩いて初めて
現状 (advisory 21 件・ライセンス 4 件) が分かった。#209 で `deps` ジョブを
CI に足し、直せたもの (`h2` → 0.4.17、死んだライセンス許可 2 件の削除) は直し、
直せないもの (libsql が `hyper-rustls` 0.25 経由で固定する `rustls-webpki` 0.102 /
`h2` 0.3 系) は **advisory 1 件につき 1 エントリで理由を書いて** `deny.toml` に記録した。

**残っているのは「libsql を上げる」1 点だけ。** 6 件中 4 件がそれで消える。
libsql 0.10 は pre-release なので、**stable が出たら `deny.toml` の
`rustls-webpki 0.102` 節と `h2` 節を再検査する** (`ignore` エントリのコメントにも
「Revisit at every libsql bump」と書いてある)。

### 候補 D-2: Tauri の CSP を有効にする (#210・小〜中)

`tauri.conf.json` の `"csp": null` = ポリシーを一切注入していない。
**急ぎではない** — 検証したところフロントエンドに HTML 注入口は 0 件
(`{@html}` / `innerHTML` 無し)。**着手時は前提を再検証すること**、
その後の変更で注入口が増えている可能性がある。

難所は `script-src`。`app.html` にテーマ適用のインラインスクリプトがあり
(初回描画前に `data-theme` を当てて色のちらつきを消す意図的なもの)、
SvelteKit のハイドレーションスクリプトもインラインなので、
素の `script-src 'self'` では両方止まる。`apps/desktop/svelte.config.js` の
`kit.csp` (hash / nonce モード) を設定する必要がある。現在は未設定。
`connect-src` は軽い (直接 `fetch` が 0 件で通信は全部 Rust 側)。

### 候補 E: 既存ロードマップ機能バックログ

未着手: Saved queries / Schema diff / Export results は済 (CSV/JSON) /
Group D-2 (ADR-0029 function-calling, `feature/adr-0029-function-calling` に
planning ball)。実利用の摩擦順に着手。新 write 経路は着手前に ADR。

### 参考: 配布済 exe の使用シグナル確認 / 再配布

- **使用確認**: `gh release view v0.3.0 --json assets --jq
  '.assets[].downloadCount'` (匿名 update-check の GET 自体は観測不可、
  資産 DL 数のみ)。
- **新版を配布したくなったら**: 次バージョンを bump → develop → main にマージ →
  **タグ push だけ**。Release CI が Win (NSIS setup.exe + `.sig`) / macOS
  (universal `.dmg` + `.app.tar.gz` + `.sig`) / MCP バイナリ / `latest.json` /
  `SHA256SUMS.txt` を publish する。配布済 exe が起動時に検知する。
  **⚠ 以前ここに「リリースオブジェクトを先に `gh release create` しておくこと」と
  書いてあったが、これは v0.3.0 時点の話で今は誤り。** v0.5.0 以降、publish ジョブが
  `gh release view || gh release create` で自力で用意する (候補 C 参照)。
  **タグ push の前に `target/release` を掴むプロセスを確認する** — 他セッションの
  `dbboard-mcp.exe` が生きていると pre-push の release ビルドが `LNK1104` で落ちる
  (v0.9.0 で実際に落ちて `--no-verify` になった。v0.10.0 では事前確認して回避した)。
  公開後に exe を実接続名で目視スキャン。
- **バンドルは CI が作る**。ローカルで MSI / `.dmg` を手作りする手順は
  **もう要らない** (`apps/dbboard` は `apps/desktop` になり、cargo-wix /
  cargo-bundle 経路は Tauri のバンドラに置き換わった)。手順が要る場合は
  README の該当節を正本とする。
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
