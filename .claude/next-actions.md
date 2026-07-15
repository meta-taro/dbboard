# 次のアクション案内 (user 側ボール常設ファイル)

このファイルは「セッションが切れても、開いた瞬間に user 側のボールがわかる」状態を維持するためのもの。
セッション終了時 / 状況が変わった時に必ず更新する。**日本語可・内部用。**

---

## 最終更新

- 日付: 2026-07-15
- develop tip: `fc087ff` (PR #65 Help→GitHub リンク merged)
- **進行中の目標: 収集担当への内々配布 (Windows-only)。** store-cabaret
  (Cloudflare D1) / store-lovehotel (Aurora DSQL IAM) / Vegas Gift
  (Supabase) の 3 接続を収集する担当に dbboard デスクトップを渡す。
- **ハンドオフ前項目 = 全て develop 入り済:**
  - ✅ テーブル右クリック簡易SQL (PR #59)
  - ✅ About/バージョン + ヘルプメニュー (PR #60)
  - ✅ 段階B トークン自動リフレッシュ (ADR-0037 / PR #61)
  - ✅ 収集セットアップ pack (PR #63) — テンプレ + cmdkey 手順 + ガードテスト
  - ✅ Help メニューに公式 GitHub リンク (PR #65)
- **#14 = ハンドオフ用 release exe をビルド済み・目視確認済み
  (2026-07-15、develop `fc087ff` から `target\release\dbboard.exe`、
  15.6 MB、Help→Project on GitHub の動作も確認)。残るは物理的な
  引き渡しと実 secret の受け渡しのみ。**

## モード

**in-use / continuous-improvement (menu-not-sequence)** — 2026-06-24 以降。
今は収集担当への配布に向けたハンドオフ準備が実利用ドリブンの主軸で、
その最後の 1 手 (#14) が残っている。

---

## user 側のボール (= 次に着手する時の選択肢)

### **★ 最優先: #14 の物理引き渡し (ビルドは完了済み)**

exe は develop `fc087ff` からビルド済み・目視確認済み
(`target\release\dbboard.exe`、15.6 MB、自己完結・VC++ 再頒布不要、
ADR-0032)。**残るは担当機への物理引き渡しと実 secret の受け渡しのみ。**

担当へ渡すもの:
1. `target\release\dbboard.exe`
2. `docs\collector-setup\` 一式 (`connections.template.toml` + `README.md`)
3. 実 secret 3 種 (Cloudflare API token / AWS secret access key /
   Supabase URL) を **別経路で安全に** (ファイル・チャット・メール本文に
   絶対載せない)

- 担当機側のセットアップは `docs/collector-setup/README.md` に沿う
  (config 配置 → cmdkey で 3 secret シード → 起動)。**secret は
  一切ファイルに書かない。**
- ソース無変更で再ビルドすると `Finished in ~1s` で既存 exe を再利用する
  (タイムスタンプが古くても develop tip と一致していれば有効)。
- MSI で渡したい場合のみ WiX 手順 (下記) だが、exe 単体で十分。

### 参考: MSI 実ビルド手順 (配布したくなったら)

- PR #52 で MSI **ソース**は揃済。human 手順:
  1. WiX Toolset v3 をインストール (candle.exe / light.exe を PATH に)
  2. `cargo install cargo-wix`
  3. `cd apps/dbboard && cargo wix` → `target\wix\dbboard-0.1.0-x86_64.msi`
- **exe 単体配布なら不要** = `target\release\dbboard.exe` をそのまま渡せる。

### この doc-sync の後 (agent 側)

- 本 chore (`chore/post-pr63-doc-sync`) をマージしたら、次に動くのは
  #14 の完了報告か、新しい摩擦レポート待ち。
- ロードマップ menu-not-sequence の未着手候補: Export results (CSV/JSON) /
  Saved queries / Schema diff / Group D-2 (ADR-0029 function-calling,
  planning ball あり)。

---

## ⚠️ 未判断の申し送り

- **リポジトリが public** で、実業務の接続名 (store-cabaret /
  store-lovehotel / vegas-gift / "Cabaret") が既にマージ済みテスト
  フィクスチャ + collector pack に入っている。secret 値は無いが、
  リポジトリ全体を名前サニタイズするかは maintainer 判断待ち。

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
