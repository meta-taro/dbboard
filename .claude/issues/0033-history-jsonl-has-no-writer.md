# 0033 — `history.jsonl` を書くコードが存在しない

- 状態: open。**v1.0 の contract 凍結より前に決める必要がある**
- 見つけた経緯: issue 0032（saved queries）の保存先を探していて、相乗りできるか調べた副産物
- 関連: ADR-0017（`history.jsonl` のスキーマ）、ADR-0027 / issue 0010（AI 呼び出しを v:2 で記録）、
  ADR-0089（egui クライアントの引退）、ADR-0011（v1.0 = contract の凍結）、
  issue 0003（`dbboard-web` 側のスキーマミラー）

## 事象

`~/.config/dbboard/history.jsonl`（ADR-0017）に **読み書きするコードがワークスペースに 1 行も無い。**

一次情報:

```
$ grep -rn "default_history_path|history\.jsonl" crates apps
crates/dbboard-config/src/store.rs:578   → パスを返すだけ
crates/dbboard-config/tests/storage.rs   → そのパスが正しいことのテスト
crates/dbboard-config/tests/config_dir_override.rs → 同上
（残りはすべて doc コメント内の言及）
```

**書き手はどこへ行ったか**: reader / writer は `dbboard-ui`（egui クレート）にあった。
issue 0010 の記録に `dbboard-ui::history` v:2 reader + writer とある。
そのクレートは **ADR-0089 で egui クライアントごと削除された**。
Tauri クライアントの履歴は webview の `localStorage`（`$lib/history/`）で、
別物・別スキーマ・接続ごと 50 件上限。

## なぜ放置できないか

3 つの文書が、存在しない機能を存在すると言っている。

1. **`docs/roadmap.md`** — 「AI calls recorded in `history.jsonl` with schema v:2 bump」が
   **`[x]`（完了）**のまま。完了したのは事実だが、その後**削除された**ことが書かれていない
2. **`dbboard-web` がスキーマをミラーしている**（issue 0003）。web 側は
   「desktop が吐く形」として v:2 を実装している。**desktop はもう何も吐いていない**
3. **ADR-0011 により v1.0 は「contract を凍らせること」**。`docs/api-contract.md` 自体には
   history の記述は無い（確認済み）が、**issue 0003 の共有スキーマは contract 層の約束**として
   扱われてきた（`CLAUDE.md` の Pacing Note）

## 選択肢（決めるのは人間）

| | 案 | 意味すること |
|---|---|---|
| a | **実装を戻す** — Tauri 側で `history.jsonl` を書く | `dbboard-web` のミラーが再び本物になる。`localStorage` 側と二重管理になるので、どちらを正本にするか決める必要がある |
| b | **contract から外す** — ADR-0017 を superseded にし、web 側にも伝える | 一番正直。ただし web 側が実装済みのものを捨てることになるので、**両リポの合意が要る**（Pacing Note） |
| c | **「予約済み・未実装」と明記する** | 一番安い。roadmap の `[x]` に取り消し線と理由を足し、ADR-0017 に「writer は ADR-0089 で消えた」と追記するだけ |

**c は a / b のどちらを選んでも先にやるべき**こと。文書が実態と食い違っている状態は、
選択肢を検討する間も誤解を生み続ける。

## やらないこと

- この issue の中で勝手に a / b を選ぶこと。**web 側の実装を捨てるかどうかは contract 層の判断**で、
  両リポの ADR が要る（`CLAUDE.md` の Sibling Repository / Pacing Note）
