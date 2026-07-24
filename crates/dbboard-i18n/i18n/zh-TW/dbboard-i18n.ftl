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
result-sort-hint = 點擊排序；Ctrl / Shift 新增排序層級

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
connections-reconnect-button = 重新連線
connections-active-marker = （目前）
connections-switch-error = 無法連線

language-menu = 語言
theme-menu = 佈景主題
theme-auto = 自動
theme-light = 淺色
theme-dark = 深色
help-menu = 說明
help-docs-hint = 設定與連線指南請參閱 README.md 與 docs/。
help-repo-link = GitHub 上的專案
help-ai-about-title = 關於 AI 助理
help-ai-about-body = AI 助理會用淺白的語言解釋 SQL 陳述式，並根據你輸入的描述草擬 SQL 查詢；在提供建議時也會讀取你的資料表與欄位名稱。它絕不會執行 SQL，絕不會寫入你的資料庫，也絕不會將資料列傳送到任何地方——在你將草稿複製到編輯器並自行執行之前，什麼都不會發生。需要 API 金鑰，金鑰會儲存在作業系統的認證管理員中。

ai-menu-button = AI 助理
ai-panel-title = AI 助理
ai-scope-hint = 解釋 SQL 並根據描述草擬查詢。它絕不會執行 SQL 或變更資料——一切都由你自己檢查並執行。
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
ai-settings-kind-openai = OpenAI
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
structure-col-note = 備註
structure-note-hint = 新增備註…
structure-table-note = 資料表備註

edit-save-button = 儲存
edit-discard-button = 捨棄
edit-staged-count = { $count } 項未儲存的編輯
edit-set-null = 設為 NULL
edit-revert-cell = 還原儲存格
edit-cell-hint = 雙擊編輯 · 右鍵設為 NULL
edit-save-unexpected-rows = 已停止儲存：應為 1 列，卻影響了 { $rows } 列

# ADR-0049 backup (logical dump).
backup-button = 備份…
backup-button-hint = 將此資料庫的資料表匯出為 SQL 檔案
backup-planning = 正在準備備份…
backup-warn-title = 大型資料庫
backup-warn-body = 此資料庫所有資料表共有 { $rows } 列。匯出可能需要一段時間並產生較大的檔案。
backup-warn-continue = 仍然備份
backup-warn-cancel = 取消
backup-dialog-title = 備份另存為
backup-progress-title = 正在備份
backup-progress-table = 第 { $done } / { $total } 個資料表
backup-progress-rows = { $done } / { $total } 列
backup-progress-current = 目前：{ $table }
backup-cancel-button = 取消
backup-done-title = 備份完成
backup-done-summary = 已匯出 { $tables } 個資料表，{ $rows } 列。
backup-done-cancelled = 備份已取消 — 檔案包含部分匯出內容。
backup-done-failures = 有 { $count } 個資料表無法讀取，已略過。
backup-done-truncations = 有 { $count } 個資料表被中途截斷。
backup-failed-title = 備份失敗
backup-close-button = 關閉

# ADR-0051：邏輯還原（匯入）。
restore-button = 還原…
restore-button-hint = 將 SQL 檔案套用到此資料庫
restore-planning = 正在讀取檔案…
restore-dialog-title = 選擇要還原的 SQL 檔案
restore-warn-title = 目標非空
restore-warn-body = 此資料庫已有 { $tables } 個資料表。還原 { $statements } 條陳述式可能會失敗或覆寫現有資料。
restore-warn-continue = 仍然還原
restore-warn-cancel = 取消
restore-progress-title = 正在還原
restore-progress-statements = 陳述式 { $done } / { $total }
restore-cancel-button = 取消
restore-done-title = 還原完成
restore-done-summary = 已套用 { $statements } 條陳述式：{ $ddl } 條結構，{ $data } 條資料。
restore-done-cancelled = 還原已取消 — 目標包含部分還原內容。
restore-done-failures = 有 { $count } 條陳述式失敗，已略過。
restore-close-button = 關閉
restore-failed-title = 還原失敗

# ADR-0050: persisted, user-editable backup warn threshold.
backup-settings-menu = 備份
backup-threshold-label = 警告閾值（列數）
backup-threshold-hint = 傾印前，當總列數超過此值時顯示大型資料庫警告。儲存至 ui-settings.toml。
