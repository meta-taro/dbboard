app-title = dbboard

tables-heading = 数据表
tables-empty = (无数据表)

sql-heading = SQL
sql-run-button = 运行

history-title = 历史记录 ({ $count })
history-empty = (无最近查询)

result-heading = 结果
result-empty = (请运行查询)
result-affected = OK (受影响行数: { $rows })

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
connections-active-marker = （当前）

language-menu = 语言

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
