app-title = dbboard

tables-heading = Tabelas
tables-empty = (sem tabelas)

sql-heading = SQL
sql-run-button = Executar

history-title = Histórico ({ $count })
history-empty = (sem consultas recentes)

result-heading = Resultado
result-empty = (execute uma consulta)
result-affected = OK ({ $rows } linhas afetadas)
result-copy-all = Copiar
result-copy-all-hint = Copiar todo o resultado para a área de transferência como TSV (cole em uma planilha)
result-export-csv = Salvar CSV…
result-export-error = Não foi possível salvar o arquivo CSV
result-copy-selected = Copiar seleção
result-copy-selected-hint = Copiar as linhas selecionadas para a área de transferência como TSV
result-export-selected-csv = Salvar seleção como CSV…
result-clear-selection = Limpar seleção
result-selected-count = { $count } selecionadas
result-select-row-hint = Clique para selecionar a linha (Ctrl / Shift para várias)

error-prefix-connection = Erro de conexão
error-prefix-query = Erro de consulta
error-prefix-schema = Erro de esquema
error-prefix-type-conversion = Erro de conversão de tipo
error-prefix-capability = Recurso indisponível

connections-window-title = Conexões
connections-restart-hint = As alterações serão aplicadas na próxima inicialização do dbboard.
connections-list-empty = (nenhuma conexão configurada)
connections-add-button = Adicionar
connections-edit-button = Editar
connections-delete-button = Excluir
connections-save-button = Salvar
connections-cancel-button = Cancelar
connections-confirm-delete = Excluir esta conexão?
connections-field-id = ID
connections-field-name = Nome
connections-field-kind = Tipo
connections-field-turso-path = Caminho do banco
connections-field-d1-account = ID da conta
connections-field-d1-database = ID do banco
connections-field-d1-base-url = URL base (opcional)
connections-field-d1-token = Token de API
connections-field-pg-url = URL de conexão
connections-replace-token = Substituir token
connections-replace-url = Substituir URL
connections-connect-button = Conectar
connections-reconnect-button = Reconectar
connections-active-marker = (ativa)
connections-switch-error = Falha na conexão

language-menu = Idioma

ai-menu-button = Assistente de IA
ai-panel-title = Assistente de IA
ai-mode-explain = Explicar SQL
ai-mode-suggest = Sugerir SQL
ai-input-explain = SQL a explicar:
ai-input-suggest = Descreva a consulta desejada:
ai-send-button = Enviar
ai-busy = Aguardando o provedor…
ai-empty = (Sem resposta ainda — digite uma instrução acima e pressione Enviar)
ai-error-prefix-configuration = Erro de configuração da IA
ai-error-prefix-network = Erro de rede da IA
ai-error-prefix-provider = Erro do provedor de IA
ai-error-prefix-quota = Cota de IA excedida
ai-error-prefix-cancelled = Solicitação de IA cancelada

# ADR-0026 Phase 4 Stage 2 Group B: streaming + cancelamento
# cooperativo + medidor de tokens.
ai-cancel-button = Cancelar
ai-cancelled-message = Cancelado.
ai-tokens-meter = Tokens: { $tin } entrada / { $tout } saída

# ADR-0025 Phase 4 Stage 2 Group A slice (b): janela de configurações dos provedores de IA.
ai-settings-menu-button = Provedores de IA
ai-settings-window-title = Provedores de IA
ai-settings-list-empty = (nenhum provedor de IA configurado)
ai-settings-add-button = Adicionar
ai-settings-edit-button = Editar
ai-settings-delete-button = Excluir
ai-settings-save-button = Salvar
ai-settings-cancel-button = Cancelar
ai-settings-use-button = Usar
ai-settings-confirm-delete = Excluir este provedor de IA?
ai-settings-active-marker = (ativo)
ai-settings-field-id = Id
ai-settings-field-name = Nome
ai-settings-field-kind = Tipo
ai-settings-field-model = Modelo (opcional)
ai-settings-field-api-key = Chave de API
ai-settings-replace-api-key = Substituir chave de API
ai-settings-kind-anthropic = Anthropic
ai-active-with-name = Ativo: { $name }

ai-include-details = Incluir detalhes das colunas
ai-prefetching = Buscando esquemas das tabelas…
ai-prefetch-warning = Não foi possível descrever { $count } tabela(s); continuando sem elas.

# ADR-0030 result grid: truncated long / multi-line cell values.
cell-expand-hint = Mostrar valor completo
cell-full-text-title = Valor da célula
cell-copy = Copiar

# ADR-0030 auto-limit guard for bare SELECTs.
auto-limit-checkbox = LIMIT { $count }
auto-limit-hint = Adiciona LIMIT a SELECTs sem limite para que uma varredura ilimitada não congele a interface. Escreva seu próprio LIMIT ou desmarque para substituir.

# ADR-0031 structure tab.
tab-results = Resultado
tab-structure = Estrutura
structure-empty = (clique em uma tabela para ver sua estrutura)
structure-loading = Descrevendo tabela…
structure-no-columns = (sem colunas)
structure-col-ordinal = #
structure-col-name = Nome
structure-col-type = Tipo
structure-col-nullable = Nulo
structure-col-pk = Chave
structure-col-default = Padrão
