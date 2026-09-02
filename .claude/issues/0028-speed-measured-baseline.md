# 0028 — v0.14「Speed, measured」: 最適化の前に計測を置く

- 状態: 完了 (2026-09-02)
- 枠: `docs/roadmap.md` Near slots の **v0.14 — Speed, measured**
- 関連: ADR-0141（本 issue で追加）、ADR-0125（Windows libSQL teardown の再試行）

## Context

roadmap の v0.14 枠はこう書かれている:

> Startup, connect-and-browse, large result sets. Measurement lands before
> any optimisation, so the numbers are comparable afterwards

つまりこの枠の成果物は **速くなったこと** ではなく、**速さが言えるようになったこと**
である。順序に意味がある。先に最適化を入れてしまうと、比較対象の数字が存在しない
まま「速くなった気がする」で終わり、次に遅くなったときも気づけない。

現状、ワークスペースにベンチマークは **一つもない**。`benches/` ディレクトリも
criterion 等の依存もゼロ。README 冒頭は "A high-performance desktop database
client" と名乗っているが、その主張を裏づける数字は一つも記録されていない。

## 計測する三点（roadmap の文言に対応）

### 1. Startup

`apps/desktop/src-tauri/src/lib.rs` の `run()` が窓を出す前に必ず行う仕事:

- `dbboard_config::default_path()`
- `ConnectionAdmin::open(path, secrets)` — `connections.toml` の解析
- `AnnotationsAdmin::open_default()` — `annotations.toml` の解析
- `ai::AiState::bootstrap(&secrets)` — keyring に触りうる
- `McpService::with_default_paths(secrets)`

いずれも Tauri ランタイムを立てずに呼べるので、そのまま計測できる。
keyring アクセス（macOS なら Keychain）は体感に効く可能性があり、
ここが実測されていないのが現状の最大の空白。

### 2. Connect-and-browse

`TursoAdapter::connect_local(":memory:")` に対して、UI が
「接続してテーブルを開く」までに実際に呼ぶ順序をそのまま:

connect → `list_tables` → `describe_table` → `foreign_keys` → 初回ページの `query`

ネットワークを挟まないので、ここで出る数字は **adapter とスキーマ処理のコスト**
であって回線の速さではない。リモート DB の数字は別途必要だが、それは再現できない
ので baseline には入れない。

### 3. Large result sets

`MAX_RESULT_ROWS` = 10,000。UI へは `QueryResult` が Tauri IPC を JSON で渡る
（`Row` は `#[serde(transparent)]` の裸配列）ので、実際に効くのは:

- 行の materialise
- `serde_json` 直列化 ← **IPC コストの実体はここ**
- `sorted_row_order`（`sort.rs`）

## 方針: 計測基盤は自前（criterion を入れない）

判断は ADR-0141 に書く。要点:

- 既存の dev-dependencies は tokio / tempfile / wiremock / serde_json / tower だけ。
  proptest も insta も入っていない。criterion は plotters / rayon 等を引き込み、
  `cargo clippy --all-targets` が CI で毎回それをコンパイルする。
- リリース系ツール（`release-*.mjs`）も自前 + 自前テストという先例がある。
- 必要なのは統計的厳密さではなく **「起動は 200ms か 2s か」** が言えること。
  warmup + N 回 + 中央値 / p95 で足りる。

## 数字を CI のゲートにはしない

CI ランナーの数字は騒がしく、閾値を置けば必ず flaky になる。ADR-0125 が
「稀な失敗をどう扱うか」で既に一度苦労している。

代わりに **計測点の集合** を drift テストで固定する:
`docs/performance-baseline.md` に載っている計測点と、コードが実際に測る計測点が
食い違ったら失敗する。数字は人が見る、点が消えたことは機械が見る。
`toolchain_pin_drift.rs` / `hook_install_drift.rs` / `release-plan.test.mjs` と
同じ「覚えておくのではなく検査する」型。

## Acceptance

- [x] `crates/dbboard-bench`（`publish = false`）が追加され、三群を測る
- [x] `Stats`（中央値 / p95 / min / max）と Markdown 整形に失敗するテストが先にある
- [x] `docs/performance-baseline.md` が生成され、機械・toolchain・日付を併記する
- [x] baseline に載る計測点とコードの計測点が食い違うと落ちるテストがある
- [x] `scripts/libsql-serialised-crates.txt` に `dbboard-bench` が入る
      （`dbboard-turso` に到達するため。`serialised_teardown.rs` が要求する）
- [x] ADR-0141 を `docs/decisions.md` に追記
- [x] CHANGELOG の `[Unreleased] — Speed, measured` に項目が入る

## Notes

- この issue の完了は「最適化した」ではない。最適化は数字が出てから、
  別の作業として枠に載せる（v0.14 に入るか v0.15 に流れるかは trigger 次第）。
- baseline の数字は機械依存。ファイルには測った機械を明記し、
  他機の数字と直接比べないことを本文に書く。

## 結果 (2026-09-02)

初回計測は `docs/performance-baseline.md`。読み方の要点:

- **In-process は遅くない。** browse 5 点はすべて 1 桁マイクロ秒。
- **IPC の JSON は容疑者ではなかった。** 10,000 行の `serde_json` が 927µs に対し、
  同じ行をドライバから取り出す `query_10k` が 4.4ms。速くするなら materialise 側から。
- **拾った 2 つ**: `truncate_rows` が 9,900 行の破棄に 570µs (全直列化の半分超)、
  `annotations.toml` の解析が `connections.toml` の 6 倍 (1.0ms vs 173µs)。

いずれも「次にどこを最適化するか」の材料であって、この issue の範囲ではない。
最適化は数字が出てから別の作業として枠に載せる。

## 訂正 (2026-09-02 その2 / ADR-0142)

上の3点のうち **2点は所見として成立しなかった**。v0.15 で着手する前に確かめた結果:

- **`annotations.toml` の 6 倍は比較になっていなかった。** 2つのフィクスチャは同じ
  文書ではない。`connections.toml` は 20 エントリで **2,092 バイト**、
  `annotations.toml` は同じ 20 接続に加えてテーブル註 100 と列註 400 を持ち
  **38,952 バイト**。**18.6 倍のデータを 6 倍の時間で**解析しているので、
  バイトあたりでは annotations の方が*速い*。直接測ると `toml` は
  21 ns/B と 19.5 ns/B — 同じパーサが同じ速さで動いているだけだった。
  型への写し取りは 46µs 中の 2µs、ファイル読みは 10µs。
- **`truncate_rows` の 570µs は解放コスト。** 中身は `Vec::truncate` 一行で、
  9,900 行ぶんの行 Vec と `String` を壊す費用。関数を書き直しても動かない。
  払わない方法は「作らない」ことだけで、**3つのアダプタは既にそうしている** —
  libSQL は `run_select_capped` が上限で取り出しを止め、Postgres はサーバ側カーソル、
  MySQL はストリームを途中で捨てる。後続の `truncate_rows` は帯であって機構ではない。
- **`query_10k` の 4.4ms の内訳。** 同じ 10,000 行を 1 / 2 / 4 / 8 列で測ると
  1.80 / 2.21 / 3.06 / 4.40 ms の直線。傾き **37 ns/値**、切片 **143 ns/行**。
  1/3 が行あたり（driver の `next()` と行 Vec）、2/3 が値あたり（取り出しと
  `String` 確保）。どちらもコードの誤りではなく、この表現の値段。

**さらに、計測機のノイズ床が所見より広い。** 同じ実験を2回走らせると 44µs と 78µs
（1.5〜1.8 倍）になる。M1 の P / E コアのどちらに載るかで変わる。1回の run の中では
安定している（p95 は中央値の数%以内）が、**リリース間の比較は run 間の比較**なので、
「6 倍」はその境界のすぐ外、「20% 速くなった」は完全に内側。

→ 最適化の当て先は materialise を速くすることではなく、**50 行しか見せない画面のために
10,000 行を作らないこと**（ページング）。issue 0029 へ。

## 積み残し (ADR-0141 の Consequences にも記載)

- プラットフォーム鍵束の起動コストは未計測。おそらく起動経路の単独最大だが、
  ダイアログを出さずに測る方法が無い。
- 起動は `run()` の config 仕事までで、Tauri の窓は含まない。
  "time to first pixel" はアプリ内部からの計測が要る。
