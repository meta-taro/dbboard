app-title = dbboard

tables-heading = Таблицы
tables-empty = (нет таблиц)

sql-heading = SQL
sql-run-button = Выполнить

history-title = История ({ $count })
history-empty = (нет недавних запросов)

result-heading = Результат
result-empty = (выполните запрос)
result-affected = OK (затронуто строк: { $rows })
result-copy-all = Копировать
result-copy-all-hint = Скопировать весь результат в буфер обмена как TSV (вставьте в таблицу)
result-export-csv = Сохранить CSV…
result-export-error = Не удалось сохранить файл CSV

error-prefix-connection = Ошибка подключения
error-prefix-query = Ошибка запроса
error-prefix-schema = Ошибка схемы
error-prefix-type-conversion = Ошибка преобразования типа
error-prefix-capability = Функция недоступна

connections-window-title = Подключения
connections-restart-hint = Изменения вступят в силу при следующем запуске dbboard.
connections-list-empty = (нет настроенных подключений)
connections-add-button = Добавить
connections-edit-button = Изменить
connections-delete-button = Удалить
connections-save-button = Сохранить
connections-cancel-button = Отмена
connections-confirm-delete = Удалить это подключение?
connections-field-id = ID
connections-field-name = Имя
connections-field-kind = Тип
connections-field-turso-path = Путь к базе
connections-field-d1-account = ID аккаунта
connections-field-d1-database = ID базы
connections-field-d1-base-url = Базовый URL (необязательно)
connections-field-d1-token = API-токен
connections-field-pg-url = URL подключения
connections-replace-token = Заменить токен
connections-replace-url = Заменить URL
connections-connect-button = Подключиться
connections-active-marker = (активно)
connections-switch-error = Не удалось подключиться

language-menu = Язык

ai-menu-button = ИИ-ассистент
ai-panel-title = ИИ-ассистент
ai-mode-explain = Объяснить SQL
ai-mode-suggest = Предложить SQL
ai-input-explain = SQL для объяснения:
ai-input-suggest = Опишите нужный запрос:
ai-send-button = Отправить
ai-busy = Ожидание ответа поставщика…
ai-empty = (Ответа пока нет — введите запрос выше и нажмите «Отправить»)
ai-error-prefix-configuration = Ошибка конфигурации ИИ
ai-error-prefix-network = Сетевая ошибка ИИ
ai-error-prefix-provider = Ошибка поставщика ИИ
ai-error-prefix-quota = Превышена квота ИИ
ai-error-prefix-cancelled = Запрос ИИ отменён

# ADR-0026 Phase 4 Stage 2 Group B: потоковая передача + кооперативная
# отмена + счётчик токенов.
ai-cancel-button = Отмена
ai-cancelled-message = Отменено.
ai-tokens-meter = Токены: { $tin } вход / { $tout } выход

# ADR-0025 Phase 4 Stage 2 Group A slice (b): окно настроек провайдеров ИИ.
ai-settings-menu-button = Провайдеры ИИ
ai-settings-window-title = Провайдеры ИИ
ai-settings-list-empty = (нет настроенных провайдеров ИИ)
ai-settings-add-button = Добавить
ai-settings-edit-button = Изменить
ai-settings-delete-button = Удалить
ai-settings-save-button = Сохранить
ai-settings-cancel-button = Отмена
ai-settings-use-button = Использовать
ai-settings-confirm-delete = Удалить этого провайдера ИИ?
ai-settings-active-marker = (активен)
ai-settings-field-id = Идентификатор
ai-settings-field-name = Имя
ai-settings-field-kind = Тип
ai-settings-field-model = Модель (необязательно)
ai-settings-field-api-key = API-ключ
ai-settings-replace-api-key = Заменить API-ключ
ai-settings-kind-anthropic = Anthropic
ai-active-with-name = Активный: { $name }

ai-include-details = Включить сведения о столбцах
ai-prefetching = Получение схем таблиц…
ai-prefetch-warning = Не удалось описать { $count } табл.; продолжаем без них.

# ADR-0030 result grid: truncated long / multi-line cell values.
cell-expand-hint = Показать полное значение
cell-full-text-title = Значение ячейки
cell-copy = Копировать

# ADR-0030 auto-limit guard for bare SELECTs.
auto-limit-checkbox = LIMIT { $count }
auto-limit-hint = Добавляет LIMIT к SELECT без ограничения, чтобы неограниченное сканирование не подвесило интерфейс. Напишите свой LIMIT или снимите флажок, чтобы переопределить.

# ADR-0031 structure tab.
tab-results = Результат
tab-structure = Структура
structure-empty = (нажмите на таблицу, чтобы увидеть её структуру)
structure-loading = Описание таблицы…
structure-no-columns = (нет столбцов)
structure-col-ordinal = #
structure-col-name = Имя
structure-col-type = Тип
structure-col-nullable = Null
structure-col-pk = Ключ
structure-col-default = По умолч.
