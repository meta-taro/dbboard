# 次のアクション案内 (user 側ボール常設ファイル)

このファイルは「セッションが切れても、開いた瞬間に user 側のボールがわかる」状態を維持するためのもの。
セッション終了時 / 状況が変わった時に必ず更新する。**日本語可・内部用。**

---

## 最終更新

- 日付: 2026-08-19 その2 (**v0.9.0 を出した。タグ push まで完了、release CI 実行中。
  あわせて CI が遅い件を計測して潰し、#194 を実装した。未 push は 5 本に増えた。**
  ① **リリース経路**: #197 (`develop` → `main`) マージ済 → `main` = `49634f3`、
  タグ `v0.9.0` (annotated・`49634f3` を指す) push 済 → release run `32265722883`。
  タグ push だけで完結する (v0.5.0 以降)。**残るのは公開 `.exe` の PII 目視確認だけ。**
  ② **タグ push に `--no-verify` を使った (baseline §35 の記録)。** pre-push の
  `cargo build --release` が `LNK1104: dbboard_mcp.exe を開くことができません` で落ちた。
  原因は `target/release/dbboard-mcp.exe` が **3 プロセス動いていた**こと (PID 12340 /
  17524 / 31856、いずれも親が生きている `claude` = **別セッションの MCP サーバー**)。
  他セッションを落とす判断はしていない。タグが指す `49634f3` は**すでに origin/main に
  あるコミット**で、その SHA に対する CI は当日 14:26Z に `ci` / `pii-scan` とも緑。
  つまり pre-push が確かめようとしたことはリモート側で既に済んでおり、タグ push は
  新しい内容を 1 バイトも送っていない。環境要因 + CI 緑確認済 = §35 の条件どおり。
  ③ **CI が遅い件は Rust のせいではなかった (ADR-0114、`ci/faster-verification` =
  `6609e97`)。** 16m46s のジョブの内訳を実測すると、apt install **572s** /
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
  ④ **#194 を実装した (`fix/export-ref-ownership` = `c373778`、ADR-0113)。**
  他接続の keychain スロットを名乗る entry を**エクスポート時**に検出する。ref は
  `admin.rs` の `keyring_ref()` 1 箇所でしか作られず、`update` は `id` を書き戻すので
  **rename 経路が存在しない** = 「自分の id から導出されない ref を持つ entry は不正」が
  **ストアを引かずに entry 単体で**判定できる。インポート側 (ADR-0038) より強い。
  **拒否はしない・警告する**のが ADR-0113 の決定 — バックアップを最も必要とする
  操作者にとって、成功行を警告で押しのけたら「失敗した」と読めてしまう。
  ⑤ **未 push = 5 本。** `ci/faster-verification` (`6609e97`) と
  `fix/export-file-names` (`943c26e`) は独立、残り 3 本は stack
  (`feat/turso-remote` `8a9bf2a` → `fix/import-report-reasons` `601ff61` →
  `fix/export-ref-ownership` `c373778`)。**`ci/faster-verification` を先に入れると、
  以降の全 run が 9 分の apt ダウンロードを払わなくなる**ので優先度が高い。
  **user 側ボール = ① 公開 `.exe` の PII 目視確認、② 5 本の push (→ PR は私が作る)、
  ③ #180 / #189 のマージ、④ v1.0 の残り 3 ゲート (下記 候補 0)、⑤ 従来からの継続分。**
  次のエージェント側タスク = push 後の PR 作成のみ。)

- 日付: 2026-08-19 (**未 push のブランチが 3 本たまった。3 本とも中身は完成・検証済で、
  詰まっているのは push だけ。** develop = `9403077` (v0.9.0)、open PR = #180 / #189。
  ① **`fix/export-file-names` = `943c26e`** — エクスポートの既定ファイル名に日時を入れた。
  user 要望「ファイル名固定なのやめてほしい。日時いれるだけでだいぶ変わる」そのまま。
  ② **`feat/turso-remote` = `4bbdf63` + `8a9bf2a`** — issue #191。リモート Turso を
  2 つ目の kind として実装 (**ADR-0111**)。kind は 11 種になったが**ワイヤ id は 9 のまま**
  (`turso-remote` は `turso` として名乗る) = コントラクト非破壊。docs は別コミット。
  ③ **`fix/import-report-reasons` = `601ff61`** (②の上に stack) — user の指摘どおり、
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
  中身は `ci/checkout-v6` の `330cd59` として cherry-pick 済 (原 commit と patch 一致を確認、
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
  #172 (リリース準備) → develop、#173 (`develop` → `main`) → `main` = `2a9b1e8`、
  タグ `v0.8.0` push 済 → release CI run `31784033330` 実行中。
  Windows exe + MSI / macOS dmg + `SHA256SUMS.txt` を publish する。
  **タグ push だけで完結する** (v0.5.0 以降、publish ジョブが release オブジェクトを
  自力で view-or-create するようになったため。旧 v0.3.0 の落とし穴は解消済み)。
  ② **リリース前に埋めた穴**: `CHANGELOG.md` の `[Unreleased]` が空、`docs/roadmap.md` が
  v0.7.0 を現行として説明したままだった。どちらも**タグ後には埋められない**場所なので
  #172 で先に埋めた。
  ③ **エージェント側のミス**: DESIGN.md の追記 (`128f18e`) を #171 の push 後に commit して
  マージに乗せ損ねた。rebase + cherry-pick (`c316e9b`) で復旧済み。
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
  ① **#159 と #169 をマージし、open PR がゼロになった。** develop = `7569cd5`。
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

- ※ 2026-08-13 その2 〜 2026-08-03 のエントリは
  `.claude/archive/next-actions-2026-08.md` へ全文退避 (baseline §31、退避日 2026-08-09 /
  2026-08-14 / 2026-08-16 の 3 回)。さらに古いもの (2026-07-29 以前) は
  `.claude/archive/next-actions-2026-07.md`。


## モード

**in-use / continuous-improvement (menu-not-sequence)** — 2026-06-24 以降。
配布 (#14) は 2026-07-16 に完了済、v0.3.0 公開済、DL ページも live。今は
「配布済 exe を担当が実際に使うか」を update-check で観測しつつ、次の実利用改善
(下記の user 側ボール) を摩擦順に進めるフェーズ。直近は結果グリッドのソート漏れと
MSI ショートカット漏れを補完し、次いで maintainer 要望の**論理バックアップ
(ダンプ)** を ADR-0049 として実装・着地 (PR #108)。

---

## user 側のボール (= 次に着手する時の選択肢)

### ★★ 候補 0: v1.0 の残り 3 ゲート (`.claude/issues/0021-v1-0-criteria.md`)

4 つのうち **ゲート 4 (署名) は 2026-08-16 に ADR-0106 で決着済**。残り 3 つは
いずれも baseline §38 の「人にしかできない工程」で、**エージェント側から代われない**。
1.0 を出す気があるなら、ここが最優先。

1. **#161 の 3 点観察** — Run ボタンがクリックに反応しない。観察するのは
   ボタンの色 / カーソル形状 / 一度別の場所をクリックしてからだと効くか。
   原因が特定できていない段階で当て推量のテストは書かない、が方針。
2. **コントラクトを `dbboard-web` へミラー** — 凍結の**前**にやる必要がある。
   凍結後にミラーすると、web 側が別物を実装していた場合に破壊的変更が要る。
3. **検証シート 001–003 の実施** — 一度も実施されていない。人が動かして
   `結果` `実施日` `担当` を埋める (baseline §22)。**エージェントが `OK` を
   書き込むことは禁止**。環境的に無理な行は `未実施` のまま残してよい。
   Firestore エミュレータの停止は
   `docker compose -f docker/firestore-emulator/compose.yaml down`。

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

### 候補 A-2: README に MSI アンインストール残留の明文化 (小・任意)

MSI アンインストールは `%APPDATA%\dbboard\dbboard\` の設定と Windows 資格情報
マネージャーのエントリを残す (仕様)。ユーザに口頭で伝えた `cmdkey` +
フォルダ削除のクリーンアップ手順を README か `docs/` に明文化する小 chore。

### 候補 A-3: アップデート通知の「変更点」が定型文のまま (小・実利用で判明)

v0.8.0 の配信で判明。0.7.0 側に出た通知の「変更点」が
**`dbboard v0.8.0. See the release page for the full changelog.`** という定型文で、
実際に何が変わったかが読めない。出どころは `.github/workflows/release.yml:287` —
`latest.json` の `notes` をタグ名から組み立てているため、中身がタグごとに変わらない。

ステータスバーのチップ (ADR-0101) は「何が待っているか」を伝えるためのものなので、
定型文だと**チップの存在理由が半分死ぬ**。`CHANGELOG.md` から当該バージョンの節を
抜いて `notes` に入れれば済む。CHANGELOG は今回から実際に埋まっているので材料はある。
更新の判断材料になる場所なので、次のリリース前に直しておく価値がある。

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

### 候補 C: release.yml の publish 自己作成化 — **完了 (v0.5.0)**

publish ステップが `gh release view <tag> || gh release create <tag>` になり、
タグ push だけでリリースが完結するようになった。v0.8.0 / v0.9.0 のリリースは
この経路で実行済み。**残るのは公開 `.exe` の PII 目視確認だけで、これは CI がやらない
人間の作業。**[[project-release-ci-needs-release-object]]。

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
