# 0030 — `export_then_import` が稀に `Bundle(Corrupt)` で落ちる

- 状態: 未解決（再現せず。次に起きたときに診断できる形にした）
- 関連: ADR-0125（稀な失敗の扱い）、ADR-0055（PII scan の bypass 禁止）、#196 / ADR-0140（束の設計）

## 観測（2026-09-02、1 回だけ）

`feature/what-the-numbers-actually-said` の push で pre-push が止めた:

```
admin::tests::export_then_import_into_empty_store_restores_entries_and_secrets
panicked at crates/dbboard-config/src/admin.rs:4569: import: Bundle(Corrupt)
test result: FAILED. 399 passed; 1 failed; 1 ignored
```

**この branch は `dbboard-config` に触っていない**（docs と bench のテストのみ）。
同日中に release テストを 4 回通しており、いずれも 1,498 passed / 0 failed。

## 調べたこと（すべて外れ）

- **単独実行**: pass。並列の全体実行でだけ出た。
- **繰り返し**: `cargo test --release -p dbboard-config --lib` を 6 回連続 → 6 回とも
  400 passed / 0 failed。**再現しない**。頻度はおおよそ 10 回に 1 回未満。
- **scrypt の work factor**: age は暗号化する機械の速さで work factor を決め、復号側は
  重すぎるものを `ExcessiveWork` で拒む（それが `Corrupt` に丸められていた）。しかし
  age はこの値を**プロセス内でキャッシュ**するので、同一プロセスの暗号化と復号がずれる
  余地がない。8 スレッドで負荷をかけて実測しても idle・負荷中とも `log_n = 19` で、
  idle 時に作った束は負荷中でも開けた。**この説は否定された。**
- **テスト間の共有状態**: 束は毎回その場で生成。改竄テストは自前の複製を壊しており、
  `source_bundle()` にキャッシュは無い。`kdf_guard()` がテスト時は暗号化・復号を直列化
  しているので、scrypt の 64MiB が同時に何本も立つこともない。

## この issue でやったこと

原因が分からないまま推測で直すことはしない。代わりに、**次に起きたときに原因が分かる形**にした:

- `BundleError::Corrupt` が 3 つの別々の失敗を 1 語に潰していたのをやめ、
  `Corrupt { stage, reason }` にした。`stage` は `Header` / `FileKey` / `Body`、
  `reason` は age 自身の言葉。次に落ちたログには**どの検査が拒んだか**が残る。
- ついでに見つかった誤診も直した: 途中で切れた束を `age` は I/O エラーとして返すが、
  `decrypt_bundle` が読むのは `&[u8]` で、**故障し得る装置が存在しない**。
  `bundle I/O error` は起こり得ないことの診断だったので、`Corrupt { stage: Header }` に
  変えた。束は別の機械へ運ぶためのものなので、「コピーが途中で終わった」は
  この形式が現実に最も出会いやすい失敗。

## 次に起きたら

ログの `stage` を見る。

- `Header` → 束のバイト列が age として壊れている。生成側（`encrypt_bundle`）を疑う。
- `FileKey` → 構造的な鍵の取り出し失敗。`reason` が `ExcessiveWork` なら work factor 説が
  復活する（その場合は work factor を固定する方向。移動先の機械で開けない問題と同根）。
- `Body` → AEAD 不一致。生成と検証の間でバイト列が変わっている。

再現手段が無いまま閾値や retry を足さないこと。ADR-0125 の retry は
**症状が特定されていた**（Windows の libSQL teardown、`0xc0000005`）から書けたもので、
ここはまだその段階に無い。
