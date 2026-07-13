app-title = dbboard

tables-heading = テーブル
tables-empty = （テーブルなし）

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

error-prefix-connection = 接続エラー
error-prefix-query = クエリエラー
error-prefix-schema = スキーマ取得エラー
error-prefix-type-conversion = 型変換エラー
error-prefix-capability = 機能非対応

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
connections-active-marker = （接続済み）
connections-switch-error = 接続できませんでした

language-menu = 言語

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
