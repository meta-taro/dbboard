app-title = dbboard

tables-heading = Tablas
tables-empty = (sin tablas)
tables-context-select = Seleccionar todas las filas
tables-context-count = Contar filas

sql-heading = SQL
sql-run-button = Ejecutar

history-title = Historial ({ $count })
history-empty = (sin consultas recientes)

result-heading = Resultado
result-empty = (ejecuta una consulta)
result-affected = OK ({ $rows } filas afectadas)
result-copy-all = Copiar
result-copy-all-hint = Copiar todo el resultado al portapapeles como TSV (pégalo en una hoja de cálculo)
result-export-csv = Guardar CSV…
result-export-error = No se pudo guardar el archivo CSV
result-copy-selected = Copiar selección
result-copy-selected-hint = Copiar las filas seleccionadas al portapapeles como TSV
result-export-selected-csv = Guardar selección como CSV…
result-clear-selection = Borrar selección
result-selected-count = { $count } seleccionadas
result-select-row-hint = Clic para seleccionar la fila (Ctrl / Mayús para varias)

error-prefix-connection = Error de conexión
error-prefix-query = Error de consulta
error-prefix-schema = Error de esquema
error-prefix-type-conversion = Error de conversión de tipo
error-prefix-capability = Función no disponible

connections-window-title = Conexiones
connections-restart-hint = Los cambios se aplican al próximo inicio de dbboard.
connections-list-empty = (sin conexiones configuradas)
connections-add-button = Añadir
connections-edit-button = Editar
connections-delete-button = Eliminar
connections-save-button = Guardar
connections-cancel-button = Cancelar
connections-confirm-delete = ¿Eliminar esta conexión?
connections-field-id = ID
connections-field-name = Nombre
connections-field-kind = Tipo
connections-field-turso-path = Ruta de la base
connections-field-d1-account = ID de cuenta
connections-field-d1-database = ID de base de datos
connections-field-d1-base-url = URL base (opcional)
connections-field-d1-token = Token de API
connections-field-pg-url = URL de conexión
connections-replace-token = Reemplazar token
connections-replace-url = Reemplazar URL
connections-connect-button = Conectar
connections-reconnect-button = Reconectar
connections-active-marker = (activa)
connections-switch-error = No se pudo conectar

language-menu = Idioma
theme-menu = Tema
theme-auto = Automático
theme-light = Claro
theme-dark = Oscuro
help-menu = Ayuda
help-docs-hint = Consulta README.md y docs/ para la configuración y las guías de conexión.
help-repo-link = Proyecto en GitHub
help-ai-about-title = Acerca del Asistente de IA
help-ai-about-body = El Asistente de IA explica una instrucción SQL en lenguaje sencillo y redacta una consulta SQL a partir de una descripción que usted escribe; para las sugerencias también lee los nombres de sus tablas y columnas. Nunca ejecuta SQL, nunca escribe en su base de datos y nunca envía filas de datos a ningún sitio: no ocurre nada hasta que usted copia un borrador en el editor y lo ejecuta usted mismo. Se necesita una clave de API, que se almacena en el administrador de credenciales de su sistema operativo.

ai-menu-button = Asistente de IA
ai-panel-title = Asistente de IA
ai-scope-hint = Explica SQL y redacta consultas a partir de una descripción. Nunca ejecuta SQL ni modifica datos: usted revisa y ejecuta todo.
ai-mode-explain = Explicar SQL
ai-mode-suggest = Sugerir SQL
ai-input-explain = SQL a explicar:
ai-input-suggest = Describe la consulta que quieres:
ai-send-button = Enviar
ai-busy = Esperando al proveedor…
ai-empty = (Sin respuesta aún — escribe un mensaje arriba y pulsa Enviar)
ai-error-prefix-configuration = Error de configuración de IA
ai-error-prefix-network = Error de red de IA
ai-error-prefix-provider = Error del proveedor de IA
ai-error-prefix-quota = Cuota de IA superada
ai-error-prefix-cancelled = Solicitud de IA cancelada

# ADR-0026 Phase 4 Stage 2 Group B: streaming + cancelación cooperativa
# + medidor de tokens.
ai-cancel-button = Cancelar
ai-cancelled-message = Cancelado.
ai-tokens-meter = Tokens: { $tin } entrada / { $tout } salida

# ADR-0025 Phase 4 Stage 2 Group A slice (b): ventana de ajustes de proveedores de IA.
ai-settings-menu-button = Proveedores de IA
ai-settings-window-title = Proveedores de IA
ai-settings-list-empty = (no hay proveedores de IA configurados)
ai-settings-add-button = Añadir
ai-settings-edit-button = Editar
ai-settings-delete-button = Eliminar
ai-settings-save-button = Guardar
ai-settings-cancel-button = Cancelar
ai-settings-use-button = Usar
ai-settings-confirm-delete = ¿Eliminar este proveedor de IA?
ai-settings-active-marker = (activo)
ai-settings-field-id = Id
ai-settings-field-name = Nombre
ai-settings-field-kind = Tipo
ai-settings-field-model = Modelo (opcional)
ai-settings-field-api-key = Clave de API
ai-settings-replace-api-key = Reemplazar clave de API
ai-settings-kind-anthropic = Anthropic
ai-active-with-name = Activo: { $name }

ai-include-details = Incluir detalles de columnas
ai-prefetching = Obteniendo esquemas de tablas…
ai-prefetch-warning = No se pudieron describir { $count } tabla(s); se continúa sin ellas.

# ADR-0030 result grid: truncated long / multi-line cell values.
cell-expand-hint = Mostrar valor completo
cell-full-text-title = Valor de la celda
cell-copy = Copiar

# ADR-0030 auto-limit guard for bare SELECTs.
auto-limit-checkbox = LIMIT { $count }
auto-limit-hint = Añade LIMIT a los SELECT sin límite para que un escaneo ilimitado no congele la interfaz. Escribe tu propio LIMIT o desmárcalo para anularlo.

# ADR-0031 structure tab.
tab-results = Resultado
tab-structure = Estructura
structure-empty = (haz clic en una tabla para ver su estructura)
structure-loading = Describiendo tabla…
structure-no-columns = (sin columnas)
structure-col-ordinal = #
structure-col-name = Nombre
structure-col-type = Tipo
structure-col-nullable = Nulo
structure-col-pk = Clave
structure-col-default = Predet.
structure-col-note = Nota
structure-note-hint = Añadir una nota…
structure-table-note = Nota de tabla

edit-save-button = Guardar
edit-discard-button = Descartar
edit-staged-count = { $count } edición(es) pendiente(s)
edit-set-null = Establecer NULL
edit-revert-cell = Revertir celda
edit-cell-hint = Doble clic para editar · clic derecho para NULL
edit-save-unexpected-rows = Guardado detenido: se esperaba 1 fila, { $rows } afectadas
