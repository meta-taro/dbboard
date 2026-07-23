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
result-sort-hint = 点击排序；Ctrl / Shift 添加排序层级

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
theme-menu = 主题
theme-auto = 自动
theme-light = 浅色
theme-dark = 深色
help-menu = 帮助
help-docs-hint = 有关设置和连接指南，请参阅 README.md 和 docs/。
help-repo-link = GitHub 上的项目
help-ai-about-title = 关于 AI 助手
help-ai-about-body = AI 助手会用通俗的语言解释 SQL 语句，并根据你输入的描述起草 SQL 查询；在给出建议时还会读取你的表名和列名。它绝不会执行 SQL，绝不会写入你的数据库，也绝不会将数据行发送到任何地方——在你将草稿复制到编辑器并自行执行之前，什么都不会发生。需要 API 密钥，密钥保存在操作系统的凭据管理器中。

ai-menu-button = AI 助手
ai-panel-title = AI 助手
ai-scope-hint = 解释 SQL 并根据描述起草查询。它绝不会执行 SQL 或更改数据——一切都由你自己检查并执行。
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
structure-col-note = 备注
structure-note-hint = 添加备注…
structure-table-note = 表备注

edit-save-button = 保存
edit-discard-button = 放弃
edit-staged-count = { $count } 处未保存修改
edit-set-null = 设为 NULL
edit-revert-cell = 还原单元格
edit-cell-hint = 双击编辑 · 右键设为 NULL
edit-save-unexpected-rows = 已停止保存：应为 1 行，却影响了 { $rows } 行

# ADR-0049 backup (logical dump).
backup-button = 备份…
backup-button-hint = 将此数据库的表导出为 SQL 文件
backup-planning = 正在准备备份…
backup-warn-title = 大型数据库
backup-warn-body = 此数据库所有表共有 { $rows } 行。导出可能需要一段时间并生成较大的文件。
backup-warn-continue = 仍然备份
backup-warn-cancel = 取消
backup-dialog-title = 备份另存为
backup-progress-title = 正在备份
backup-progress-table = 第 { $done } / { $total } 个表
backup-progress-rows = { $done } / { $total } 行
backup-progress-current = 当前：{ $table }
backup-cancel-button = 取消
backup-done-title = 备份完成
backup-done-summary = 已导出 { $tables } 个表，{ $rows } 行。
backup-done-cancelled = 备份已取消 — 文件包含部分导出内容。
backup-done-failures = 有 { $count } 个表无法读取，已跳过。
backup-done-truncations = 有 { $count } 个表被中途截断。
backup-failed-title = 备份失败
backup-close-button = 关闭

# ADR-0051：逻辑恢复（导入）。
restore-button = 恢复…
restore-button-hint = 将 SQL 文件应用到此数据库
restore-planning = 正在读取文件…
restore-dialog-title = 选择要恢复的 SQL 文件
restore-warn-title = 目标非空
restore-warn-body = 此数据库已有 { $tables } 个表。恢复 { $statements } 条语句可能会失败或覆盖现有数据。
restore-warn-continue = 仍然恢复
restore-warn-cancel = 取消
restore-progress-title = 正在恢复
restore-progress-statements = 语句 { $done } / { $total }
restore-cancel-button = 取消
restore-done-title = 恢复完成
restore-done-summary = 已应用 { $statements } 条语句：{ $ddl } 条结构，{ $data } 条数据。
restore-done-cancelled = 恢复已取消 — 目标包含部分恢复内容。
restore-done-failures = 有 { $count } 条语句失败，已跳过。
restore-close-button = 关闭
restore-failed-title = 恢复失败

# ADR-0050: persisted, user-editable backup warn threshold.
backup-settings-menu = 备份
backup-threshold-label = 警告阈值（行数）
backup-threshold-hint = 转储前，当总行数超过此值时显示大型数据库警告。保存到 ui-settings.toml。
