# 次のアクション案内 (user 側ボール常設ファイル)

このファイルは「セッションが切れても、開いた瞬間に user 側のボールがわかる」状態を維持するためのもの。
セッション終了時 / 状況が変わった時に必ず更新する。**日本語可・内部用。**

---

## 最終更新

- 日付: 2026-07-16
- develop tip: `bb9f46f` (PR #72 = 内々配布ガイド + gitignore merged)
- **進行中の目標: 収集担当への内々配布 (Windows-only)。** store-a
  (Cloudflare D1) / store-b (Aurora DSQL IAM) / store-c
  (Supabase) の 3 接続を収集する担当に dbboard デスクトップを渡す。
  ※ id は中立サンプル名。実際の店舗名との対応は非公開メモリ側にのみ保持。
- **ハンドオフ前項目 = 全て develop 入り済:**
  - ✅ テーブル右クリック簡易SQL (PR #59)
  - ✅ About/バージョン + ヘルプメニュー (PR #60)
  - ✅ 段階B トークン自動リフレッシュ (ADR-0037 / PR #61)
  - ✅ 収集セットアップ pack (PR #63) — テンプレ + cmdkey 手順 + ガードテスト
  - ✅ Help メニューに公式 GitHub リンク (PR #65)
  - ✅ **接続設定の暗号化バンドル export/import (ADR-0038 / PR #68)** —
    パスフレーズ暗号 `.dbbx` 1 ファイルで全接続 + secret を移送。収集
    ハンドオフの「ファスト経路」= テンプレ + cmdkey 3 手シードの代替。
  - ✅ **エラー表示の統一 = コピー可能・日英併記 (ADR-0039 / PR #70)** —
    アプリ側エラーを「日本語訳 + 原文英語」併記 + Copy ボタン。ハンドオフ
    ユーザが英文を AI/検索に貼れる。SQL/プロバイダ本文は原文のまま。
  - ✅ **起動時アップデート確認 (ADR-0040 / PR #71)** — GitHub Releases
    API を 1 回 best-effort GET し、新版があれば Help メニューに通知 +
    リリースノート + DL リンク。更新は手動、失敗時サイレント、
    `DBBOARD_NO_UPDATE_CHECK` でオプトアウト。
  - ✅ **内々配布ガイド一式 (PR #72)** — メンテナ runbook
    (`docs/maintainer/internal-distribution.md`) + テスター onboarding
    (`docs/internal-testing.md`) + `.gitignore` (`*.dbbx` / `/dist/` /
    `connections.toml`)。
- **#14 = ハンドオフ用 release exe。現 develop `bb9f46f` は #70/#71/#72
  を含み、配布ガイドの説明 (コピー可能エラー + 更新通知) と実装が一致する
  理想の再ビルド地点。この develop から `cargo build --release` を取り直す
  こと。残るは物理引き渡しと実 secret (またはバンドル + パスフレーズ) の
  受け渡しのみ。**

## モード

**in-use / continuous-improvement (menu-not-sequence)** — 2026-06-24 以降。
今は収集担当への配布に向けたハンドオフ準備が実利用ドリブンの主軸で、
その最後の 1 手 (#14) が残っている。

---

## user 側のボール (= 次に着手する時の選択肢)

### **★ 最優先: #14 の物理引き渡し (要 exe 再ビルド)**

**引き渡し前に最新 develop `bb9f46f` から `cargo build --release` を
取り直す**こと。この develop は #70 (コピー可能エラー) / #71 (更新通知) /
#72 (配布ガイド) を含み、配布ガイドの記述と exe の挙動が一致する。
※ ビルド前に **dbboard のウィンドウを閉じる** (実行中だと exe ロックで
os error 5)。**残るは担当機への物理引き渡しと実 secret / バンドルの
受け渡しのみ。**

担当へ渡すもの:
1. `target\release\dbboard.exe` (最新 develop `bb9f46f` から再ビルド)
2. secret の受け渡し、以下いずれか:
   - **推奨 (ADR-0038)**: 手元の dbboard で 3 接続を Export し、暗号化
     `.dbbx` 1 ファイルを渡す。**パスフレーズは別経路** (口頭 / 別チャネル)。
     担当機側は Import 1 回で接続 + secret が入る。テンプレも cmdkey も不要。
   - **旧手順**: `docs\collector-setup\` 一式 + 実 secret 3 種
     (Cloudflare API token / AWS secret access key / Supabase URL) を
     **別経路で安全に**、担当機で cmdkey シード。

- バンドル経路の担当機セットアップ: exe 起動 → Connections ウィンドウ →
  Import → `.dbbx` 選択 + パスフレーズ入力。`docs/connections.md` の
  "Moving connections between machines" 節参照。
- 旧 cmdkey 経路は `docs/collector-setup/README.md` に沿う
  (config 配置 → cmdkey で 3 secret シード → 起動)。**secret は
  一切ファイルに書かない。**
- MSI で渡したい場合のみ WiX 手順 (下記) だが、exe 単体で十分。

### 参考: MSI 実ビルド手順 (配布したくなったら)

- PR #52 で MSI **ソース**は揃済。human 手順:
  1. WiX Toolset v3 をインストール (candle.exe / light.exe を PATH に)
  2. `cargo install cargo-wix`
  3. `cd apps/dbboard && cargo wix` → `target\wix\dbboard-0.1.0-x86_64.msi`
- **exe 単体配布なら不要** = `target\release\dbboard.exe` をそのまま渡せる。

### この doc-sync の後 (agent 側)

- 本 chore (`chore/post-pr72-doc-sync`) をマージしたら、次に動くのは
  #14 の完了報告か、新しい摩擦レポート待ち。
- ロードマップ menu-not-sequence の未着手候補: Export results (CSV/JSON) /
  Saved queries / Schema diff / Group D-2 (ADR-0029 function-calling,
  planning ball あり)。

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
