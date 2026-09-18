# 0032 — 保存したクエリ

- 状態: 実装済み（ADR-0147）。v0.16 の 3 項目のうち 2 つ目
- 枠: `docs/roadmap.md` Near slots の **v0.16 — Everyday work**
- 関連: ADR-0147（保存先と規則）、ADR-0146（同じ枠の 1 つ目 = JSON エクスポート）、
  ADR-0045（`annotations.toml` = 一番近い前例）、ADR-0087（MCP verb を足す条件）、
  ADR-0017（`history.jsonl` — **書き手がいない**）

## なぜ localStorage ではないのか

エディタは既に実行したものを覚えている（接続ごと・50 件上限・重複排除、`$lib/history/`）。
保存クエリも同じ場所に置くのは 1 行で済むが、**それは間違った 1 行**。

- **履歴は道具が勝手に記録するもの**で、設計上使い捨て（上限あり・重複排除・隣に Clear ボタン）
- **保存クエリはその逆**。人が意図して作った成果物で、午後いっぱいかけて絞り込んだ 1 本のこともある

`localStorage` は前者に合っていて後者に合わない。「サイトデータを消去」で消え、
config ディレクトリのバックアップからは見えず、**別のマシンへ移るとき付いてこない** —
このプロジェクトは実際に一度それをやって、git の外にあるものがどうなるか学んだところ。

## 調べて分かったこと（ADR に書いた）

設計上はもう 1 つ履歴がある — ADR-0017 の `history.jsonl`（レコードスキーマは
`dbboard-web` がミラー、issue 0003）。**これを書くコードはもう存在しない。**
reader / writer は egui クレート `dbboard-ui` にあり、ADR-0089 の引退で一緒に消えた。
`dbboard-config` は今もパスを解決し、`docs/roadmap.md` は今も完了扱いで `[x]` を付けている。
**誰も書かないファイルに相乗りはできない。** → **issue 0033** に切り出した

## 決めたこと（ADR-0147 に全文）

1. **`saved-queries.toml`** を config ディレクトリに（`connections.toml` / `annotations.toml` の隣）。
   `secure_fs` の user-only、アトミックな全書き換え、「ファイルが無い = 空のストア」、
   重複と未知バージョンの**大きな声での拒否** — すべて `annotations` と同じ
2. **接続 id で紐付け、接続にスコープする** — 接続名を変えてもクエリは残る（注釈と同じ）
3. **上書きには答えが要る** — `add` は既存名を拒否し、呼び出し側が確認してから `replace` を呼ぶ。
   `save_query` は `overwrite` を取り、衝突には `duplicate-name` という**正確な文字列**で答える
   （フロントが「人に聞く」と「失敗を報告する」を区別できるように）
4. **MCP tool にはしない** — エージェントは既に任意の文を組み立てて実行できるので能力は増えない。
   増えるのは「人の私的な作業メモがエージェントの文脈に入ること」だけ（ADR-0087 の逆）
5. **ファイルは挿入順、画面は新しい順** — 挿入順は diff が読める。各エントリの `saved_at` で
   表示側が並べ替える

## 採らなかった案

- **履歴ストアの再利用** — 形は同じでライフサイクルが逆。意図して残すものを使い捨ての場所に
  置くのが、捨てられ方そのもの
- **接続に属さない、どこでも実行できるクエリ** — 実需はある（dev と prod で同じレポート）。
  **意図的に先送り**。ファイル形式に対して additive（id の無いスタンザ、またはコピー動作）なので、
  誰も踏んでいないうちに当て推量で作ると間違った半分を固める
- **履歴の 50 件のような上限** — 履歴に上限があるのは、頼まれずに書くから。ここは頼まれずに書かない

## 完了条件

- [x] `crates/dbboard-config/src/saved_queries.rs` — ファイル形状 / parse / load / atomic save /
      `SavedQueryAdmin`（`add` / `replace` / `remove` / `queries`）
- [x] Rust ユニットテスト **13 本**（未存在ファイル、再オープン、接続スコープ、重複拒否、
      置換の位置保持、空名前・空 SQL、trim、最後の 1 本で stanza を刈る、
      ファイル内重複、未知バージョン、重複接続 id、**保存失敗時に in-memory を進めない**）
- [x] Tauri コマンド 3 本（`list_saved_queries` / `save_query` / `delete_saved_query`）
- [x] `$lib/queries/saved.ts` — `defaultQueryName` / `byRecency`（純粋）+ テスト **10 本**
- [x] `SavedQueries.svelte`（QueryPanel は 736/800 行なので独立コンポーネント）
- [x] i18n（en / ja）12 キー
- [x] ADR-0147、issue、roadmap、`docs/desktop-parity.md`、CHANGELOG

## 残り（v0.16 の 3 つ目）

- Schema diff between two connections
