app-title = dbboard

tables-heading = 数据表
tables-empty = (无数据表)
tables-context-select = 选择所有行
tables-context-count = 统计行数

sql-heading = SQL
sql-run-button = 运行

history-title = 历史记录 ({ $count })
history-empty = (无最近查询)

result-heading = 结果
result-empty = (请运行查询)
result-affected = OK (受影响行数: { $rows })
result-copy-all = 复制
result-copy-all-hint = 将整个结果以 TSV 复制到剪贴板（可粘贴到电子表格）
result-export-csv = 保存 CSV…
result-export-error = 无法保存 CSV 文件
result-copy-selected = 复制选中行
result-copy-selected-hint = 将选中的行以 TSV 格式复制到剪贴板
result-export-selected-csv = 保存选中行为 CSV…
result-clear-selection = 清除选择
result-selected-count = 已选 { $count } 行
result-select-row-hint = 点击选择该行（Ctrl / Shift 多选）

error-prefix-connection = 连接错误
error-prefix-query = 查询错误
error-prefix-schema = 模式错误
error-prefix-type-conversion = 类型转换错误
error-prefix-capability = 不支持此功能

connections-window-title = 连接
connections-restart-hint = 更改将在下次启动 dbboard 时生效。
connections-list-empty = (未配置连接)
connections-add-button = 添加
connections-edit-button = 编辑
connections-delete-button = 删除
connections-save-button = 保存
connections-cancel-button = 取消
connections-confirm-delete = 确认删除此连接？
connections-field-id = ID
connections-field-name = 名称
connections-field-kind = 类型
connections-field-turso-path = 数据库路径
connections-field-d1-account = 账户 ID
connections-field-d1-database = 数据库 ID
connections-field-d1-base-url = 基础 URL（可选）
connections-field-d1-token = API 令牌
connections-field-pg-url = 连接 URL
connections-replace-token = 替换令牌
connections-replace-url = 替换 URL
connections-connect-button = 连接
connections-reconnect-button = 重新连接
connections-active-marker = （当前）
connections-switch-error = 无法连接

language-menu = 语言
help-menu = 帮助
help-docs-hint = 有关设置和连接指南，请参阅 README.md 和 docs/。

ai-menu-button = AI 助手
ai-panel-title = AI 助手
ai-mode-explain = 解释 SQL
ai-mode-suggest = 生成 SQL
ai-input-explain = 要解释的 SQL：
ai-input-suggest = 描述您想要的查询：
ai-send-button = 发送
ai-busy = 正在等待提供方响应……
ai-empty = （暂无响应 — 请在上方输入提示并发送）
ai-error-prefix-configuration = AI 配置错误
ai-error-prefix-network = AI 网络错误
ai-error-prefix-provider = AI 提供方错误
ai-error-prefix-quota = AI 配额已超出
ai-error-prefix-cancelled = AI 请求已取消

# ADR-0026 Phase 4 Stage 2 Group B：流式 + 协作取消 + 令牌计数器。
ai-cancel-button = 取消
ai-cancelled-message = 已取消。
ai-tokens-meter = 令牌：输入 { $tin } / 输出 { $tout }

# ADR-0025 Phase 4 Stage 2 Group A slice (b)：AI 提供商设置窗口。
ai-settings-menu-button = AI 提供商
ai-settings-window-title = AI 提供商
ai-settings-list-empty = （未配置 AI 提供商）
ai-settings-add-button = 添加
ai-settings-edit-button = 编辑
ai-settings-delete-button = 删除
ai-settings-save-button = 保存
ai-settings-cancel-button = 取消
ai-settings-use-button = 使用
ai-settings-confirm-delete = 删除此 AI 提供商？
ai-settings-active-marker = （活动）
ai-settings-field-id = ID
ai-settings-field-name = 名称
ai-settings-field-kind = 类型
ai-settings-field-model = 模型（可选）
ai-settings-field-api-key = API 密钥
ai-settings-replace-api-key = 替换 API 密钥
ai-settings-kind-anthropic = Anthropic
ai-active-with-name = 活动：{ $name }

ai-include-details = 包含列详细信息
ai-prefetching = 正在获取表结构…
ai-prefetch-warning = 有 { $count } 个表无法获取结构，将忽略并继续。

# ADR-0030 result grid: truncated long / multi-line cell values.
cell-expand-hint = 显示完整值
cell-full-text-title = 单元格值
cell-copy = 复制

# ADR-0030 auto-limit guard for bare SELECTs.
auto-limit-checkbox = LIMIT { $count }
auto-limit-hint = 为没有 LIMIT 的 SELECT 追加 LIMIT，避免无限扫描冻结界面。可自行写 LIMIT 或取消勾选以覆盖。

# ADR-0031 structure tab.
tab-results = 结果
tab-structure = 结构
structure-empty = (点击表以查看其结构)
structure-loading = 正在获取表结构…
structure-no-columns = (无列)
structure-col-ordinal = #
structure-col-name = 名称
structure-col-type = 类型
structure-col-nullable = 空
structure-col-pk = 键
structure-col-default = 默认值
