app-title = dbboard

tables-heading = Таблицы
tables-empty = (нет таблиц)
tables-context-select = Выбрать все строки
tables-context-count = Подсчитать строки

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
result-copy-selected = Копировать выбранное
result-copy-selected-hint = Скопировать выбранные строки в буфер обмена как TSV
result-export-selected-csv = Сохранить выбранное в CSV…
result-clear-selection = Снять выделение
result-selected-count = Выбрано: { $count }
result-select-row-hint = Клик для выбора строки (Ctrl / Shift для нескольких)
result-sort-hint = Нажмите для сортировки; Ctrl / Shift — добавить уровень

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
connections-reconnect-button = Переподключиться
connections-active-marker = (активно)
connections-switch-error = Не удалось подключиться

language-menu = Язык
theme-menu = Тема
theme-auto = Авто
theme-light = Светлая
theme-dark = Тёмная
help-menu = Справка
help-docs-hint = См. README.md и docs/ для настройки и руководств по подключению.
help-repo-link = Проект на GitHub
help-ai-about-title = Об ИИ-ассистенте
help-ai-about-body = ИИ-ассистент объясняет SQL-запрос простым языком и составляет черновик SQL-запроса по введённому вами описанию; для подсказок он также читает имена ваших таблиц и столбцов. Он никогда не выполняет SQL, никогда не пишет в вашу базу данных и никогда не отправляет строки данных куда-либо: ничего не произойдёт, пока вы сами не скопируете черновик в редактор и не запустите его. Требуется ключ API, который хранится в менеджере учётных данных вашей операционной системы.

ai-menu-button = ИИ-ассистент
ai-panel-title = ИИ-ассистент
ai-scope-hint = Объясняет SQL и составляет черновики запросов по описанию. Он никогда не выполняет SQL и не изменяет данные: вы всё проверяете и запускаете сами.
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
ai-settings-kind-openai = OpenAI
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
structure-col-note = Заметка
structure-note-hint = Добавить заметку…
structure-table-note = Заметка таблицы

edit-save-button = Сохранить
edit-discard-button = Отменить
edit-staged-count = Несохранённых правок: { $count }
edit-set-null = Установить NULL
edit-revert-cell = Вернуть ячейку
edit-cell-hint = Двойной клик для правки · правый клик для NULL
edit-save-unexpected-rows = Сохранение остановлено: ожидалась 1 строка, затронуто { $rows }

# ADR-0049 backup (logical dump).
backup-button = Резервная копия…
backup-button-hint = Выгрузить таблицы этой базы данных в файл SQL
backup-planning = Подготовка резервной копии…
backup-warn-title = Большая база данных
backup-warn-body = В этой базе данных { $rows } строк во всех таблицах. Выгрузка может занять время и создать большой файл.
backup-warn-continue = Всё равно создать копию
backup-warn-cancel = Отмена
backup-dialog-title = Сохранить резервную копию как
backup-progress-title = Создание резервной копии
backup-progress-table = Таблица { $done } из { $total }
backup-progress-rows = { $done } / { $total } строк
backup-progress-current = Текущая: { $table }
backup-cancel-button = Отмена
backup-done-title = Резервное копирование завершено
backup-done-summary = Выгружено таблиц: { $tables }, строк: { $rows }.
backup-done-cancelled = Резервное копирование отменено — файл содержит частичную выгрузку.
backup-done-failures = Не удалось прочитать { $count } таблиц(ы), они пропущены.
backup-done-truncations = { $count } таблиц(ы) обрезаны на середине.
backup-failed-title = Ошибка резервного копирования
backup-close-button = Закрыть

# ADR-0051: логическое восстановление (импорт).
restore-button = Восстановить…
restore-button-hint = Применить файл SQL к этой базе данных
restore-planning = Чтение файла…
restore-dialog-title = Выберите файл SQL для восстановления
restore-warn-title = Цель не пуста
restore-warn-body = В этой базе данных уже есть { $tables } таблиц(ы). Восстановление { $statements } операторов(а) может завершиться ошибкой или перезаписать существующие данные.
restore-warn-continue = Всё равно восстановить
restore-warn-cancel = Отмена
restore-progress-title = Восстановление
restore-progress-statements = Оператор { $done } из { $total }
restore-cancel-button = Отмена
restore-done-title = Восстановление завершено
restore-done-summary = Применено { $statements } операторов(а): { $ddl } схемы, { $data } данных.
restore-done-cancelled = Восстановление отменено — цель содержит частичное восстановление.
restore-done-failures = { $count } операторов(а) завершились ошибкой и были пропущены.
restore-close-button = Закрыть
restore-failed-title = Ошибка восстановления

# ADR-0050: persisted, user-editable backup warn threshold.
backup-settings-menu = Резервная копия
backup-threshold-label = Предупреждать свыше (строк)
backup-threshold-hint = Показывает предупреждение о большой базе данных перед дампом, когда общее число строк превышает это значение. Сохраняется в ui-settings.toml.
