# 0027 — 認知拡大・公開ローンチ

- 状態: 未着手（原文のまま保管済み。着手順と担当の切り分けだけ決めた）
- 出典: 共有された企画草案「dbboard 認知拡大施策」。原文は
  `.claude/plans/2026-08-14-awareness-and-launch.md` に全文のまま置いてある。
  この issue は要約ではなく、原文に対して「誰がやるか」「いつやるか」だけを足したもの。
- 関連: `docs/roadmap.md` の枠（この企画はどの枠にも入らない。下記「枠を持たない理由」）

## 中心メッセージ

原文 §1 より:

> Local database client with a built-in MCP server for AI agents.

いまの README 冒頭は「A high-performance desktop database client for modern
serverless and distributed databases.」で、MCP は下のほうまで出てこない。
原文 §5 が求めている「3 秒で分かる状態」にはなっていない。

## 誰がやるか

エージェント側（実装・文章）:

1. README 冒頭の作り直し（原文 §5 のチェックリスト 10 項目）
2. Architecture 図・MCP Tool 例・Claude Code での利用例
3. DEV.to 記事の英語ドラフト（原文 §8）— 投稿はしない、下書きまで

人間側（アカウント・公開行為）:

4. GitHub Topics の追加（原文 §7 と現状の差分は下記）
5. MCP Directory 登録（原文 §4）
6. r/ClaudeAI → r/opensource → Show HN → DEV.to → Product Hunt（原文 §10 の順）

投稿は全部人間側。baseline の「公開する行為は人間」に加えて、アカウントが
本人のものなので代行のしようがない。

### GitHub Topics の差分

登録済み: cloudflare-d1 / database / database-client / desktop-app / mcp /
mcp-server / mysql / neon / postgresql / rust / sql-client / sqlite /
supabase / tauri / turso

原文 §7 にあって未登録: `model-context-protocol` `claude` `claude-code`
`ai-agent` `developer-tools` `local-first`

リポジトリ設定の変更なので人間が実行する:

```sh
gh repo edit meta-taro/dbboard --add-topic model-context-protocol,claude,claude-code,ai-agent,developer-tools,local-first
```

## Demo GIF は本番画面で撮らない

原文 §6 の 30 秒 GIF は、そのまま撮ると**実在の接続名が全部映る**。
既存のスクリーンショットと同じ扱いにする:

- 接続名は store-a / store-b のようなプレースホルダにする
- 収集係の PC ではなく、スクラッチの DB で撮る
- 公開前に 1 コマずつ目で見る（`scripts/pii-scan.sh` は画像を読めない）

これは原文には書かれていない。ADR-0055 の適用範囲が画像に及んでいなかっただけで、
及ばない理由はない。

## 枠を持たない理由

これはコードではないので `docs/roadmap.md` の枠（1 枠 = 1 リリース）には載せない。
載せると「認知拡大が終わるまでリリースが出ない」枠ができてしまう。
リリースの流れとは独立に、上の 1〜3 から順に進める。
