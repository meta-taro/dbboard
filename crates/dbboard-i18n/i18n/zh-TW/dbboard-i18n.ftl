app-title = dbboard

tables-heading = 資料表
tables-empty = (無資料表)
tables-context-select = 選取所有資料列
tables-context-count = 計算資料列數

sql-heading = SQL
sql-run-button = 執行

history-title = 歷史紀錄 ({ $count })
history-empty = (無近期查詢)

result-heading = 結果
result-empty = (請執行查詢)
result-affected = OK (影響列數：{ $rows })
result-copy-all = 複製
result-copy-all-hint = 將整個結果以 TSV 複製到剪貼簿（可貼到試算表）
result-export-csv = 儲存 CSV…
result-export-error = 無法儲存 CSV 檔案
result-copy-selected = 複製選取列
result-copy-selected-hint = 將選取的列以 TSV 格式複製到剪貼簿
result-export-selected-csv = 儲存選取列為 CSV…
result-clear-selection = 清除選取
result-selected-count = 已選取 { $count } 列
result-select-row-hint = 點擊選取該列（Ctrl / Shift 多選）

error-prefix-connection = 連線錯誤
error-prefix-query = 查詢錯誤
error-prefix-schema = 結構錯誤
error-prefix-type-conversion = 型別轉換錯誤
error-prefix-capability = 不支援此功能

connections-window-title = 連線
connections-restart-hint = 變更將於 dbboard 下次啟動時生效。
connections-list-empty = (未設定連線)
connections-add-button = 新增
connections-edit-button = 編輯
connections-delete-button = 刪除
connections-save-button = 儲存
connections-cancel-button = 取消
connections-confirm-delete = 確認刪除此連線？
connections-field-id = ID
connections-field-name = 名稱
connections-field-kind = 類型
connections-field-turso-path = 資料庫路徑
connections-field-d1-account = 帳號 ID
connections-field-d1-database = 資料庫 ID
connections-field-d1-base-url = 基礎 URL（選填）
connections-field-d1-token = API 權杖
connections-field-pg-url = 連線 URL
connections-replace-token = 替換權杖
connections-replace-url = 替換 URL
connections-connect-button = 連線
connections-active-marker = （目前）
connections-switch-error = 無法連線

language-menu = 語言
help-menu = 說明
help-docs-hint = 設定與連線指南請參閱 README.md 與 docs/。

ai-menu-button = AI 助理
ai-panel-title = AI 助理
ai-mode-explain = 解釋 SQL
ai-mode-suggest = 產生 SQL
ai-input-explain = 要解釋的 SQL：
ai-input-suggest = 描述您想要的查詢：
ai-send-button = 送出
ai-busy = 正在等候提供者回應……
ai-empty = （尚無回應 — 請在上方輸入提示並送出）
ai-error-prefix-configuration = AI 設定錯誤
ai-error-prefix-network = AI 網路錯誤
ai-error-prefix-provider = AI 提供者錯誤
ai-error-prefix-quota = AI 配額已超出
ai-error-prefix-cancelled = AI 請求已取消

# ADR-0026 Phase 4 Stage 2 Group B：串流 + 協同取消 + 權杖計量器。
ai-cancel-button = 取消
ai-cancelled-message = 已取消。
ai-tokens-meter = 權杖：輸入 { $tin } / 輸出 { $tout }

# ADR-0025 Phase 4 Stage 2 Group A slice (b)：AI 供應商設定視窗。
ai-settings-menu-button = AI 供應商
ai-settings-window-title = AI 供應商
ai-settings-list-empty = （未設定 AI 供應商）
ai-settings-add-button = 新增
ai-settings-edit-button = 編輯
ai-settings-delete-button = 刪除
ai-settings-save-button = 儲存
ai-settings-cancel-button = 取消
ai-settings-use-button = 使用
ai-settings-confirm-delete = 刪除此 AI 供應商？
ai-settings-active-marker = （使用中）
ai-settings-field-id = ID
ai-settings-field-name = 名稱
ai-settings-field-kind = 類型
ai-settings-field-model = 模型（選填）
ai-settings-field-api-key = API 金鑰
ai-settings-replace-api-key = 取代 API 金鑰
ai-settings-kind-anthropic = Anthropic
ai-active-with-name = 使用中：{ $name }

ai-include-details = 包含欄位詳細資訊
ai-prefetching = 正在取得資料表結構…
ai-prefetch-warning = 有 { $count } 個資料表無法取得結構，將略過並繼續。

# ADR-0030 result grid: truncated long / multi-line cell values.
cell-expand-hint = 顯示完整值
cell-full-text-title = 儲存格值
cell-copy = 複製

# ADR-0030 auto-limit guard for bare SELECTs.
auto-limit-checkbox = LIMIT { $count }
auto-limit-hint = 為沒有 LIMIT 的 SELECT 追加 LIMIT，避免無限掃描凍結介面。可自行寫 LIMIT 或取消勾選以覆寫。

# ADR-0031 structure tab.
tab-results = 結果
tab-structure = 結構
structure-empty = (點擊資料表以檢視其結構)
structure-loading = 正在取得資料表結構…
structure-no-columns = (無欄位)
structure-col-ordinal = #
structure-col-name = 名稱
structure-col-type = 型別
structure-col-nullable = 空
structure-col-pk = 鍵
structure-col-default = 預設值
