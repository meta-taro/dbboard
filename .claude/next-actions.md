# 次のアクション案内 (user 側ボール常設ファイル)

このファイルは「セッションが切れても、開いた瞬間に user 側のボールがわかる」状態を維持するためのもの。
セッション終了時 / 状況が変わった時に必ず更新する。**日本語可・内部用。**

---

## 順番 (これが「順次やる予定」の一覧・唯一の正)

下の「候補」節は選択肢の並び (menu) で、順番ではなかった。**順番はここ。**
上から順に着手する。差し込みが入ったら**この表を書き換えてから**着手する。

**残っている実装企画は 28 件**(+ バグ 2・保留 4・対象外 3)、これとは別に**もらった構想が 8 フェーズ**。内訳は下の表がそのまま全部で、構想 8 つも「日程に載せない」とだけ書いて中身を伏せるのはやめ、名前を出してある。
数え漏らしを防ぐため、出典は roadmap の未チェック箱 / open issue / ローカル issue の
3 つに `.claude/plans/` を加えた 4 つだけに限り、それ以外の場所に企画を置かない。

### どのバージョンで何が出るか

**枠を先に取った** (2026-08-22 / ADR-0122)。正本は `docs/roadmap.md` の "Release plan"。
これまでは「溜まったら出す」(ADR-0110) だけで、**次に何が出るのか / 出たものが何だったのか**の
両方が答えられなかった。仕組み (いつ出すか) は据え置き、**中身の予約**を足した。

**枠は予約であって締切ではない。** 間に合わなかった中身は**次の枠へ送る**。
リリースを止めないし、番号を振り直さない。

| Version | 見出し | 中身 |
|---|---|---|
| **v0.11** | 接続の複製と修復 | 複製・修復 (#213)、webview CSP (#210)、Windows 検証クラッシュ修正、リリース判定の自動報告 |
| **v0.12** | 接続一覧を操れるようにする | 順番・検索・id でなく名前・色の目印 (#192)。先に `ConnectionManager.svelte` 1,614 行の分割 |
| **v0.13** | 速度 (まず計測) | 起動 / 接続して見るまで / 大きな結果セット。**最適化の前に計測**を入れて後から比較できるようにする |
| **v0.14** | 日々の作業 | JSON エクスポート、保存クエリ、スキーマ差分 (Phase 5 の残り) |
| **v1.0** | HTTP contract の凍結 | 機能リリースではない。#161、`docs/api-contract.md` の dbboard-web への反映、検証シート 001–003 の人による実施 (ADR-0011)。9% 訳の 9 ロケール (#181) を同乗 |

新規アダプタ (DuckDB / SQL Server / Redis / ClickHouse / Elasticsearch / Oracle) は
**わざと枠を持たせない**。どれも独立した追加なので、出来た時点で開いている枠に乗る。
順番は需要が決める。

**1.0 より後 = 構想 8 フェーズの帯。** 1 帯 = 複数リリース。**順番は固定・幅は未定**
(0025 の見立てで全体は複数年)。帯 N は帯 N−1 が閉じたら開く。

| 帯 | Phase | 中身 |
|---|---|---|
| 1 番目 (v1.1 から) | 1 | Adapter Capability API / 実行履歴 / Activity Timeline / メトリクス / チャート |
| 2 番目 | 2 | MySQL・PostgreSQL・SQLite の DB 固有機能 |
| 3 番目 | 3 | バックアップ / リストア / 検証 / S3・R2・MinIO / Storage Explorer |
| 4 番目 | 4 | スケジューラ (定期クエリ / バックアップ / メンテナンス / 保持期間) |
| 5 番目 | 5 | トポロジ (レプリケーション / ラググラフ / ヘルス) |
| 6 番目 | 6 | 負荷試験 / 性能メトリクス / Before-After / レポート |
| 7 番目 | 7 | 移行 (Pre-flight / 互換性 / 検証 / データ比較 / 性能比較 / ロールバック) |
| 8 番目 | 8 | AI (互換性アドバイザ / Cross-DB 変換 / 各種解析 / MCP 高度化) |

**日付は書かない。代わりに実測を置く**: v0.4.0 → v0.10.0 は **16 日で 7 リリース**
(2026-08-04 → 08-20)。この速度なら近い枠は数日〜1 週間。約束ではなく観測値。

**「で、何が出たの？」への答え**は CHANGELOG の見出しに入れる:

```
## [0.11.0] — Connection repair and duplication
```

push のたびに次の枠の見出しも出る:

```
[changelog] 6 unreleased entries — a release is due (0.10.0 -> 0.11.0: Connection repair and duplication)
```

- 1 件以上 → 出してよい / **3 件以上 → 出すべき**
- `### Added` か `### Changed` があれば minor、無ければ patch
- **いまは 6 件 = v0.11.0 が「出すべき」状態。** #216 が develop に入った時点で切れる。
- v1.0 だけは中身で決まらない (`docs/api-contract.md` の非互換変更 = major)。
  条件は `.claude/issues/0021-v1-0-criteria.md`。**機能の数では上がらない**ので、
  下の 28 件は 1 件も v1.0 の条件ではない。

**枠が埋まったら、次の枠の実装より先に出す。** 作っただけでは誰も使えない。
待っているのは収集係と dbboard-web 側で、その人たちに届くのはタグを打った瞬間だけ。
「出すべき」が立っているのに次の実装に入るのは、実装が進んでいるように見えて
外から見た進捗はゼロのまま増えていく状態なので、下の表でも**リリースを 1 番に置いてある**。

### エージェント側 (上から順・29 件)

**直近 — 実利用の摩擦**

| # | やること | 出典 |
|---|---|---|
| 1 | #216 が develop に入る → **v0.11.0 を切る** = `node scripts/release-cut.mjs` + cargo check + commit (tag push は user) | CHANGELOG 5 件 |
| 2 | 接続リスト C — hover に id ではなく名前を出す | #192 / 0026 |
| 3 | 接続リスト A — ▲▼ で並び替え (`move_to`)。スキーマ変更不要 | #192 / 0026 |
| 4 | 接続リスト B — リスト上の絞り込み入力 → **ここで #192 closed** | #192 / 0026 |
| ~~5~~ | ~~`ConnectionManager.svelte` 1614 行を分割~~ → **完了** (1,617 → 662 行、#216 の中)。ついでに `ResultGrid.svelte` も 1,132 → 553 行 (ハードリミット超過ファイルは 0 件になった) | 0026 F |
| 6 | 接続リスト D — 色マーク。パレット確定済 (2026-08-22) | 0026 D |
| 7 | Structure タブの描画を切り出し + 毎フレームの clone を落とす | 0016 |
| 8 | MCP に export 動詞 — パスフレーズを線に載せない | #196 |
| 9 | 手書きクエリの結果も、行が特定できるなら編集可に | 0022 |

**次 — Phase 5 の残り (roadmap の未チェック箱)**

| # | やること | 出典 |
|---|---|---|
| 10 | 結果を JSON でエクスポート | roadmap Phase 5 |
| 11 | クエリの保存 (Saved queries) | roadmap Phase 5 |
| 12 | 2 接続間のスキーマ差分 | roadmap Phase 5 |

**次 — 速度 (数字を出すのが先。5 件は 1 セット)**

| # | やること | 出典 |
|---|---|---|
| 13 | **まず計測**。コールド / ウォームで「使える状態」までの秒数を出す | roadmap Performance |
| 14 | 起動 — プロセス開始から最初の描画までどこに時間が行っているか | 〃 |
| 15 | 接続とブラウズ — スキーマ取得が余計な往復をしていないか | 〃 |
| 16 | 大きな結果セット — 仮想化済みだが取得側が律速していないか | 〃 |
| 17 | release プロファイルが配布物に適切か (LTO / codegen-units) | 〃 |
| — | (Phase 5 の「起動 1 秒未満」は 13〜17 の目標値。別項目ではない) | |

**中期 — 追加アダプター (順番は 0023 で確定済み)**

| # | やること | 出典 |
|---|---|---|
| 18 | DuckDB — `.duckdb` 接続、CSV / TSV / Parquet 直参照 | 0023 P1 |
| 19 | SQL Server / Azure SQL — SQL 認証 + TLS + 構造参照 | 0023 P1 |
| 20 | Redis / Valkey — **`KEYS` を通さない (SCAN 強制)** が必須要件 | 0023 P1 |
| 21 | ClickHouse | 0023 P2 |
| 22 | Elasticsearch / OpenSearch | 0023 P2 |
| 23 | Oracle — **要望が出てから**着手 | 0023 需要ベース |

**後 — 条件付き / 大物**

| # | やること | 出典 |
|---|---|---|
| 24 | 11 ロケールのうち 9 つが 9% (334 中 30 キー) | #181 |
| 25 | AI の function calling / tool use (`describe_table` を最初の tool に) | roadmap Phase 4 D-2 / ADR-0029 待ち |
| 26 | 接続リスト E — グループ / フォルダ。**3 が効いたなら不要** | 0026 E |
| 27 | Linux パッケージング (AppImage / `.deb`) | roadmap Packaging |
| 28 | 競合ウォッチの仕組み | 0024 |
| 29 | README 冒頭を「MCP 内蔵のローカル DB クライアント」に作り直す + Architecture 図 + Claude Code での利用例 | 0027 |

**バグ 2 件 (企画ではないが未解決・どちらも user の観察待ち)**

| | | |
|---|---|---|
| #161 | Run ボタンがクリックに反応しない (Ctrl+Enter は効く) | **v1.0 ゲート** |
| #193 | `.dbbx` インポートで新規接続が追加されない (v0.8.0 Windows) | 送信側マシンでの再現待ち |

**保留 4 件 (roadmap Phase 7+ Stretch・順位を付けていない)**
スキーマの可視化 / クエリ性能分析 / プラグイン機構 / エージェント型 AI ワークフロー

**対象外 3 件 (roadmap Out of Scope)**
モバイルクライアント (web 側が担当) / 接続のクラウド同期 / マルチユーザー・共有

**もらった企画 (Database Workspace 構想・8 フェーズ) — 日程には載せていないが隠さない**

出典は `.claude/plans/2026-08-17-database-workspace.md`（**追跡済み。gitignore には入れていない**）、
レビューが `.claude/issues/0025-database-workspace-expansion.md`。0025 の結論は
「1 人 + エージェントでは複数年。**日程ではなく選別フィルタとして持つ**」で、
だから上の 28 件の列には入れていない。ただし**列に入れないことと見えなくすることは別**なので、
中身をここに出す。次の要望が来たとき「幹か枝か」はこの 8 つに当てて判定する。

| Phase | 中身 |
|---|---|
| 1 | Activity Timeline / 実行履歴 / メトリクス収集 / チャート / Adapter Capability API |
| 2 | MySQL・PostgreSQL・SQLite の DB 固有機能 |
| 3 | バックアップ / リストア / 検証 / S3・R2・MinIO / Storage Explorer |
| 4 | スケジューラ (定期クエリ / バックアップ / メンテナンス / 保持期間) |
| 5 | トポロジ (MySQL・PostgreSQL レプリケーション / ラググラフ / ヘルス) |
| 6 | 負荷試験 / 性能メトリクス / Before-After / レポート |
| 7 | 移行 (Pre-flight / 互換性 / 検証 / データ比較 / 性能比較 / ロールバック / Readiness) |
| 8 | AI (互換性アドバイザ / Cross-DB 変換 / メトリクス・移行・トポロジ解析 / MCP 高度化) |

0025 が指摘した構造的な問題が 1 つあり、これは着手前に決着が要る:
**この 8 つの多くは「DB クライアント」ではなく「運用ツール」**で、`DatabaseAdapter` トレイトの
外側に別の軸（capability・スケジューラ・オブジェクトストレージ）を要求する。
Phase 1 の Adapter Capability API だけは他のどれを選んでも先に要るので、
**この構想に着手するなら入口は Phase 1 で確定**している。

### user 側でないと進まないもの (並行・順不同)

| やること | なぜエージェントには無理か |
|---|---|
| **いま: #216 を push → 画面で 10 枚見る → merge** | push は §6。接続ダイアログ 6 枚 (追加/編集/エクスポート/インポート/複製/修復) と結果グリッド 4 枚 (本体/値ポップアップ/JSON ツリー/展開エディタ)。`svelte-check` は参照が解決することしか言わず、**CSS が当たっているかは言わない** |
| **#161 Run ボタンの 3 点観察** | 画面の観察。**v1.0 ゲートの 1 つ** |
| #193 送信側マシンでの確認 | 手元に無い環境 |
| 検証シート 002 (MongoDB) → 001 (Firestore) | §22: 実施と合否は人間。**v1.0 ゲートの 1 つ** |
| contract を dbboard-web へ写す / 写さないと明言する | **v1.0 ゲートの 1 つ**。相手リポの判断 |
| 公開 exe の目視 PII チェック | `.pii-denylist` がこのマシンに無い |
| GitHub Support へ `refs/pull/` 削除依頼 (197 ref / 676 commit) | アカウント所有者のみ |
| develop / main のブランチ保護 | リポジトリ設定 |
| コード署名 (Authenticode) を買うかどうか | 費用の判断。roadmap Packaging の未チェック箱 |
| §36 改善要望シートの記入 | §36: 使った人が書く。代筆禁止 |
| `.github/dependabot.yml` を置くかどうか | 要否の判断 |

---

## 最終更新

- 日付: 2026-08-23 (**800 行のハードリミットを超えていた最後のファイルを分割して commit まで完了。
  push は user。#216 は 5 部構成になった。**
  `ResultGrid.svelte` 1,132 → **553 行**。v0.13「速度 (まず計測)」がこのファイルに乗るので、
  計測を入れる前に読める大きさにしておく必要があった。順に 4 手:
  ① 432 行の `<style>` を `$lib/styles/result-grid.css` へ (全セレクタに `.result-grid` 接頭辞)。
  **Svelte のスタイルはコンポーネント単位でスコープされる**ので、モーダル 2 つを子に出すと
  `.popup` 系の共通装飾を共有できなくなる。接頭辞は飾りではなく、素の `table` / `td` / `th`
  ルールが**アプリ全体を巻き込む**のを止めている。`ConnectionManager` の時と同じ手。
  ② `optionsFor` → `$lib/grid/enum.ts` の `enumOptions`、`charCount` → `$lib/grid/edit.ts`。
  **どちらも先に落ちるテストを書いた** (7 失敗を確認してから実装)。`charCount` は
  code point 数え = 絵文字 1 個が `varchar(500)` に対して 2 文字に見えていたのが直る。
  ③ 値ポップアップ → `CellViewer.svelte` (101 行)、展開エディタ → `ExpandedCellEditor.svelte` (90 行)。
  ④ **リファクタに紛れた挙動改善**: `treeClosed` を `CellViewer` に持たせた。
  今までは開くたびにグリッド側が手で空にしていたが、閉じるとダイアログごと unmount される
  = 誰も覚えていなくても同じリセットになる。**これが成立するのは backdrop が
  `position: fixed; inset: 0` だから** (開いている間は 2 個目のセルに手が届かない)。
  依存する前に確認した。
  **検証**: `pnpm check` 307 files / 0 errors / 0 warnings、`pnpm test` 536 passed (31 files)、
  移動・削除した識別子 9 件の孤児掃除すべて 0、pre-commit ゲート 2 commit とも green。
  **AI がやれていないこと**: 画面で見ること。`svelte-check` は参照が解決することしか言わない。
  → **user 側: グリッド本体 / 値ポップアップ / その中の JSON ツリー / 展開エディタの 4 枚**。
  exe は再ビルドして起動済み (`target/release/dbboard-desktop.exe`)。
  このファイル自身が 605 行あったので、2026-08-20〜21 のエントリを
  `.claude/archive/next-actions-2026-08.md` へ全文退避した (baseline §31)。473 行。)
- 日付: 2026-08-22 (**#210 webview の CSP を実装して commit まで完了。push は user。**
  `app.security.csp` は `null` だった = 「既定のポリシー」ではなく **CSP ヘッダを一切出さない**。
  今の frontend に注入口は無い (DB の値も AI の応答も HTML として描画していない) ので
  挙動は変わらない。**要る機能が来る前に置く**ための guard。
  調べて分かった点が 3 つ、いずれも tauri のソースを読んで確定した (推測ではない):
  ① **`script-src 'self'` で足りる。** tauri-codegen が inline script 2 本
  (テーマ適用と SvelteKit の bootstrap) を build 時に hash 化し、`set_csp` が実行時に
  追加する。ビルド済み exe に hash 2 本が埋まっていることを確認済み。
  ② **SvelteKit の `kit.csp` は無効のままにする。** 有効にすると `<meta>` で 2 枚目の
  ポリシーが出て交差し、しかも手書きのテーマ script を知らない = 壊れる。
  issue のチェックリストは「hash mode を有効に」と書いてあったが、それは誤り。
  ③ **`style-src` は `'unsafe-inline'` を残すしかない。** CodeMirror が `<style>` を
  作って `textContent` を代入する方式でテーマを載せるため、build 時に hash 化できない。
  ここが**何も報告されずに壊れる**: CSP3 は nonce/hash が付いた瞬間 `'unsafe-inline'` を
  無視し、tauri は `app.html` に `<style>` が 1 つでもあると nonce を入れる。
  → `app.html` に `<style>` が無いことを test で固定した (他 2 本と合わせて 3 本)。
  **状態**: PR #217 で `feature/duplicate-and-repair-connection` に merge 済み (840038a)。
  CSP は **#216 の中**にあり、#216 が develop に入った時点で本体に載る。
  push で `dbboard-mcp.exe` のロックに当たった (別セッションの MCP が掴んでいた)。
  **プロセスは止めず、exe を `dbboard-mcp.exe.inuse` に改名して退かした** —
  Windows は実行中の exe を削除できないが改名はできる。`.inuse` は
  そのセッションが終わったら消す。
  **AI がやれていないこと**: issue の「webview console を開いてアプリを一周する」。
  release build に devtools は無く、debug build は `pnpm dev` が要る (共有 PC で禁止)。
  → **画面での確認は user 側**。app は起動済み (target/release/dbboard-desktop.exe)。)
- ※ 2026-08-21 その2 〜 2026-08-03 のエントリは
  `.claude/archive/next-actions-2026-08.md` へ全文退避 (baseline §31、退避日 2026-08-09 /
  2026-08-14 / 2026-08-16 / 2026-08-20 / 2026-08-23 の 5 回)。さらに古いもの (2026-07-29 以前) は
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

> **これは選択肢の控え (menu) であって順番ではない。順番は冒頭の「順番」節を見る。**
> ここは各候補の背景・調査済みの事実を残すための場所として維持する。

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

### 候補 A-6: 認知拡大の公開ローンチ (`.claude/issues/0027-awareness-and-launch.md`)

企画草案は `.claude/plans/2026-08-14-awareness-and-launch.md` に全文で置いた
(共有されていたのに、これまでリポのどこにも入っていなかった)。

**エージェント側**は 0027 の 1〜3 (README 冒頭・図・記事下書き) = 上の表の 29 番。
**user 側**は投稿とアカウント操作で、代われない:

- GitHub Topics 6 個の追加 (コマンドは 0027 に用意した)
- MCP Directory 登録
- r/ClaudeAI → r/opensource → Show HN → DEV.to → Product Hunt (この順)

**Demo GIF は本番画面で撮らない。** 実在の接続名がそのまま映る。
スクリーンショットと同じくプレースホルダ接続で撮る (`pii-scan` は画像を読めない)。

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
