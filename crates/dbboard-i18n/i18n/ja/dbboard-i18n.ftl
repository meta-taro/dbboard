app-title = dbboard

tables-heading = テーブル
tables-empty = （テーブルなし）
tables-context-select = 全行を SELECT
tables-context-count = 行数をカウント

sql-heading = SQL
sql-run-button = 実行

history-title = 履歴 ({ $count })
history-empty = （実行履歴なし）

result-heading = 結果
result-empty = （クエリを実行してください）
result-affected = OK ({ $rows } 行に影響)
result-copy-all = コピー
result-copy-all-hint = 結果全体を TSV でクリップボードにコピー（スプレッドシートに貼り付け可）
result-export-csv = CSV 保存…
result-export-error = CSV ファイルを保存できませんでした
result-copy-selected = 選択行をコピー
result-copy-selected-hint = 選択した行を TSV でクリップボードにコピー
result-export-selected-csv = 選択行を CSV 保存…
result-clear-selection = 選択解除
result-selected-count = { $count } 行選択中
result-select-row-hint = クリックで行を選択（Ctrl / Shift で複数選択）

error-prefix-connection = 接続エラー
error-prefix-query = クエリエラー
error-prefix-schema = スキーマ取得エラー
error-prefix-type-conversion = 型変換エラー
error-prefix-capability = 機能非対応

# ADR-0039 エラー表示の統一。アプリ側のエラーは「日本語訳 + 元の英文」を
# 併記し、いずれも選択・コピー可能にする。ここは訳のキー。英文は各エラー型の
# Display から取得する。error-copy-button は訳と英文の両方をコピーする。
error-copy-button = コピー
error-copy-hint = このエラー（訳と元の英文）をクリップボードにコピー
error-original-label = 原文

# SecretError（接続ストア・AI プロバイダストア共通）。
secret-error-not-found = この接続の secret が保存されていません（参照: { $reference }）。先にこのマシンの資格情報ストアに登録してください。
secret-error-backend = secret ストアの操作に失敗しました（参照: { $reference }）: { $detail }

# ConfigError — 接続ストアの読み込み・検証・編集エラー。
config-error-parse = 設定ファイルを解析できませんでした: { $detail }
config-error-unsupported-version = サポート外の設定バージョンです: { $found }（対応はバージョン { $expected } のみ）。
config-error-duplicate-id = 接続 id が重複しています: { $id }
config-error-io = 設定ファイルへのアクセスに失敗しました: { $detail }
config-error-serialize = 設定の書き出しに失敗しました: { $detail }
config-error-no-config-dir = ユーザーごとの設定ディレクトリを特定できませんでした。
config-error-not-found = 指定した id の接続が見つかりません: { $id }
config-error-kind-mismatch = 接続 { $id } の種類は編集では変更できません。削除して追加し直してください。

# BundleError — 接続設定の暗号化バンドル export / import（ADR-0038）。
config-error-bundle-passphrase-short = パスフレーズは { $min } 文字以上にしてください。
config-error-bundle-serialize = バンドルの内容を準備できませんでした: { $detail }
config-error-bundle-incorrect-passphrase = パスフレーズが違います。
config-error-bundle-corrupt = ファイルが壊れているか、dbboard で作成されたものではありません。
config-error-bundle-unsupported-version = サポート外のバンドルバージョンです: { $found }。
config-error-bundle-invalid-payload = バンドルの内容が dbboard の形式として不正です: { $detail }
config-error-bundle-io = バンドルファイルへのアクセスに失敗しました: { $detail }

# AiSettingsError — AI プロバイダストアの読み込み・検証・編集エラー。
ai-settings-error-parse = AI プロバイダ設定ファイルを解析できませんでした: { $detail }
ai-settings-error-unsupported-version = サポート外の AI プロバイダ設定バージョンです: { $found }（対応はバージョン { $expected } のみ）。
ai-settings-error-duplicate-id = AI プロバイダ id が重複しています: { $id }
ai-settings-error-unknown-active-id = アクティブな AI プロバイダ id が不明です: { $id }
ai-settings-error-io = AI プロバイダ設定ファイルへのアクセスに失敗しました: { $detail }
ai-settings-error-serialize = AI プロバイダ設定の書き出しに失敗しました: { $detail }
ai-settings-error-no-config-dir = ユーザーごとの設定ディレクトリを特定できませんでした。
ai-settings-error-not-found = 指定した id の AI プロバイダが見つかりません: { $id }
ai-settings-error-kind-mismatch = AI プロバイダ { $id } の種類は編集では変更できません。削除して追加し直してください。

connections-window-title = 接続
connections-restart-hint = 変更は dbboard の次回起動時から有効になります。
connections-list-empty = （接続が登録されていません）
connections-add-button = 追加
connections-edit-button = 編集
connections-delete-button = 削除
connections-save-button = 保存
connections-cancel-button = キャンセル
connections-confirm-delete = この接続を削除しますか？
connections-field-id = ID
connections-field-name = 名前
connections-field-kind = 種別
connections-field-turso-path = データベースパス
connections-field-d1-account = アカウント ID
connections-field-d1-database = データベース ID
connections-field-d1-base-url = ベース URL（任意）
connections-field-d1-token = API トークン
connections-field-pg-url = 接続 URL
connections-replace-token = トークンを置換
connections-replace-url = URL を置換
connections-connect-button = 接続
connections-reconnect-button = 再接続
connections-active-marker = （接続済み）
connections-switch-error = 接続できませんでした

# ADR-0038: 接続設定全体の暗号化エクスポート／インポート。
# バンドルはパスフレーズで暗号化され（age scrypt + ChaCha20-Poly1305）、
# インポート時に既存の ID はスキップして報告する。
connections-export-button = エクスポート…
connections-import-button = インポート…
connections-export-heading = 接続設定をエクスポート
connections-import-heading = 接続設定をインポート
connections-export-passphrase-hint = パスフレーズを設定してください。このファイルをインポートする際に必要になります（復元はできません）。
connections-import-passphrase-hint = このファイルのエクスポート時に設定したパスフレーズを入力してください。
connections-passphrase = パスフレーズ
connections-passphrase-confirm = パスフレーズ（確認）
connections-passphrase-mismatch = パスフレーズが一致しません。
connections-export-do = エクスポート
connections-import-do = インポート
connections-choose-file = ファイルを選択…
connections-no-file-chosen = （ファイル未選択）
connections-bundle-filter = dbboard バンドル
connections-export-ok = 接続設定をエクスポートしました
connections-import-imported = インポート
connections-import-skipped = スキップ

language-menu = 言語
theme-menu = テーマ
theme-auto = 自動
theme-light = ライト
theme-dark = ダーク
help-menu = ヘルプ
help-docs-hint = セットアップと接続の手順は README.md と docs/ を参照してください。
help-repo-link = GitHub のプロジェクトページ
# ADR-0040: 起動時のアップデート確認。新しいリリースがあるときだけヘルプメニューに
# 表示する。更新は手動 (リンクからリリースページを開く)。
help-update-available = アップデートがあります: { $version }
help-update-link = 新しいバージョンを入手
help-update-notes = 変更点

ai-menu-button = AI アシスタント
ai-panel-title = AI アシスタント
ai-mode-explain = SQL を説明
ai-mode-suggest = SQL を生成
ai-input-explain = 説明したい SQL：
ai-input-suggest = 生成したいクエリを記述：
ai-send-button = 送信
ai-busy = プロバイダの応答を待機中…
ai-empty = （未応答 — 上部にプロンプトを入力して送信してください）
ai-error-prefix-configuration = AI 設定エラー
ai-error-prefix-network = AI ネットワークエラー
ai-error-prefix-provider = AI プロバイダエラー
ai-error-prefix-quota = AI 利用上限超過
ai-error-prefix-cancelled = AI リクエストがキャンセルされました

# ADR-0026 Phase 4 Stage 2 Group B：ストリーミング + 協調的キャンセル
# + トークンメーター。`ai-cancel-button` は実行中に Send の代わりに
# 表示される（ストリーミング / アトミック 両パス）。トークンメーターは
# `{ $tin }` 入力 / `{ $tout }` 出力。
ai-cancel-button = キャンセル
ai-cancelled-message = キャンセルされました。
ai-tokens-meter = トークン：入力 { $tin } / 出力 { $tout }

# ADR-0025 Phase 4 Stage 2 Group A スライス (b)：AI プロバイダ設定ウィンドウ。
ai-settings-menu-button = AI プロバイダ
ai-settings-window-title = AI プロバイダ
ai-settings-list-empty = （AI プロバイダが登録されていません）
ai-settings-add-button = 追加
ai-settings-edit-button = 編集
ai-settings-delete-button = 削除
ai-settings-save-button = 保存
ai-settings-cancel-button = キャンセル
ai-settings-use-button = 使用
ai-settings-confirm-delete = この AI プロバイダを削除しますか？
ai-settings-active-marker = （使用中）
ai-settings-field-id = ID
ai-settings-field-name = 名前
ai-settings-field-kind = 種別
ai-settings-field-model = モデル（任意）
ai-settings-field-api-key = API キー
ai-settings-replace-api-key = API キーを置換
ai-settings-kind-anthropic = Anthropic
ai-active-with-name = 使用中：{ $name }

ai-include-details = カラム詳細を含める
ai-prefetching = テーブルスキーマを取得中…
ai-prefetch-warning = { $count } 個のテーブルの詳細を取得できませんでした。取得できた分のみで続行します。

# ADR-0030 result grid: truncated long / multi-line cell values.
cell-expand-hint = 全文を表示
cell-full-text-title = セルの値
cell-copy = コピー

# ADR-0030 auto-limit guard for bare SELECTs.
auto-limit-checkbox = LIMIT { $count }
auto-limit-hint = LIMIT なしの SELECT に LIMIT を付けて、無制限スキャンで UI が固まるのを防ぎます。自分で LIMIT を書くかチェックを外せば上書きできます。

# ADR-0031 structure tab.
tab-results = 結果
tab-structure = 構造
structure-empty = (テーブルをクリックして構造を表示)
structure-loading = テーブルを取得中…
structure-no-columns = (列なし)
structure-col-ordinal = #
structure-col-name = 名前
structure-col-type = 型
structure-col-nullable = Null
structure-col-pk = キー
structure-col-default = 既定値

edit-save-button = 保存
edit-discard-button = 破棄
edit-staged-count = { $count } 件の未保存編集
edit-set-null = NULL に設定
edit-revert-cell = セルを元に戻す
edit-save-unexpected-rows = 保存を中止しました: 1 行のはずが { $rows } 行に影響
