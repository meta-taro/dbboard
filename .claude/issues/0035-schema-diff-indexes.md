# 0035 — schema diff の残り半分（インデックスと制約）

- 状態: **完了**。外部キー = ADR-0151、インデックス = ADR-0152（user が B + C を選択）
- 枠: `docs/roadmap.md` Near slots の **v0.17 — The half that was deferred**（3 項目の 3 つ目）
- 関連: ADR-0148（保留の理由）、ADR-0149（画面）、ADR-0049（dump の DDL）、
  issue 0034（schema diff 本体・完了）

## ADR-0148 が保留した理由

> インデックスは取得手段がエンジンごとに違い、**11 個中いくつが答えられるか未確認**。
> 一律に手に入る半分（列と主キー）を先に出す。

その「未確認」を確認した。

## 調査結果（2026-09-09 実測）

インデックスの知識が今どこにあるかというと、**`table_ddl`（ADR-0049 の dump 用）だけ**。
そして `table_ddl` を実装しているのは **6 クレート中 3 つ**:

| アダプタ | `table_ddl` | インデックスの扱い |
|---|---|---|
| `dbboard-postgres` | ✅ | `table_ddl.rs` が**制約・独立したインデックス・所有インデックス**を出している |
| `dbboard-mysql` | ✅ | 実装あり |
| `dbboard-d1` | ✅ | `CREATE INDEX` を出す（自動生成インデックスの除外も実装済み） |
| `dbboard-turso` | ❌ | `sqlite_master` は読んでいるが DDL は出していない |
| `dbboard-firestore` | ❌ | そもそも DDL の概念が違う |
| `dbboard-mongodb` | ❌ | インデックスはあるが（`createIndexes`）、モデルが別 |

**接続 kind でいうと 10 種のうち**、postgres 系（postgres / neon / supabase / aurora-dsql-iam）+
mysql + d1 が答えられ、**turso / turso-remote / firestore / mongodb が答えられない**。

## つまり何が問題か

**「インデックスを比較する」には、比較できる形でインデックスを取る手段が要る。**
今あるのは `table_ddl`（**文字列**）だけ。

- **案 A: DDL テキストを比較する** — 安いが、**整形の違いを差分として報告する**。
  ADR-0148 が「同じだと言って間違える方が高い」と決めた向きの逆側に振り切れていて、
  ノイズで本物を埋める（列順を差分にしない、と決めたのと同じ理由で却下すべき）
- **案 B: `list_indexes` を trait に足す**（構造化）— 正しいが、**アダプタ 6 つ中 3 つは
  新規実装**。postgres / mysql / d1 は SQL の知識が既に `table_ddl` にあるので移植で済む。
  turso は `sqlite_master` から取れる。**firestore / mongodb は「インデックス」の意味が違う**
- **案 C: 答えられるアダプタだけ比較し、答えられない側は「比較していない」と言う** —
  ADR-0148 の「差 0 件のときも比較範囲を明示する」という既存の態度と一貫する

## まだ決めていないこと（人の判断）

1. **A / B / C のどれか**（私の見立てでは **B + C の組み合わせ**。構造で取り、取れないエンジンは
   その旨を画面に出す）
2. **firestore / mongodb をどう扱うか** — 「インデックス」を無理に同じ名前で並べるのか、
   別概念として出さないのか。**ここは contract に触る可能性がある**（`Capabilities` に
   `has_list_indexes` を足すなら additive だが、`dbboard-web` へのミラー要否が生じる）
3. ~~**制約（外部キー・一意・CHECK）を同じ便でやるか**~~ — **外部キーは実装した**（ADR-0151）。
   `foreign_keys` は ADR-0054 以来 trait にあり、**4 アダプタ（turso 含む＝10 kind 中 8）**が
   実装済みで、contract にも触らずに済んだ。一意制約と CHECK は未着手

## やっていないこと

- 実装。**設計を決める前にコードを書かない**（ADR-0148 の書き出しと同じ立場）
- `Capabilities` への追加。contract 層に触るので、決めてから
