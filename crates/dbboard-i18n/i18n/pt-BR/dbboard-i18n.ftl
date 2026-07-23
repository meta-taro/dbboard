app-title = dbboard

tables-heading = Tabelas
tables-empty = (sem tabelas)
tables-context-select = Selecionar todas as linhas
tables-context-count = Contar linhas

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
result-sort-hint = Clique para ordenar; Ctrl / Shift para adicionar um nível

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
theme-menu = Tema
theme-auto = Automático
theme-light = Claro
theme-dark = Escuro
help-menu = Ajuda
help-docs-hint = Consulte README.md e docs/ para configuração e guias de conexão.
help-repo-link = Projeto no GitHub
help-ai-about-title = Sobre o Assistente de IA
help-ai-about-body = O Assistente de IA explica uma instrução SQL em linguagem simples e rascunha uma consulta SQL a partir de uma descrição que você digita; para as sugestões, ele também lê os nomes de suas tabelas e colunas. Ele nunca executa SQL, nunca grava no seu banco de dados e nunca envia linhas de dados para lugar algum: nada acontece até você copiar um rascunho para o editor e executá-lo você mesmo. É necessária uma chave de API, que fica armazenada no gerenciador de credenciais do seu sistema operacional.

ai-menu-button = Assistente de IA
ai-panel-title = Assistente de IA
ai-scope-hint = Explica SQL e rascunha consultas a partir de uma descrição. Nunca executa SQL nem altera dados: você revisa e executa tudo.
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
structure-col-note = Nota
structure-note-hint = Adicionar uma nota…
structure-table-note = Nota da tabela

edit-save-button = Salvar
edit-discard-button = Descartar
edit-staged-count = { $count } edição(ões) pendente(s)
edit-set-null = Definir NULL
edit-revert-cell = Reverter célula
edit-cell-hint = Clique duplo para editar · clique direito para NULL
edit-save-unexpected-rows = Salvamento interrompido: esperava 1 linha, { $rows } afetadas

# ADR-0049 backup (logical dump).
backup-button = Backup…
backup-button-hint = Exportar as tabelas deste banco de dados para um arquivo SQL
backup-planning = Preparando o backup…
backup-warn-title = Banco de dados grande
backup-warn-body = Este banco de dados tem { $rows } linhas em todas as suas tabelas. O despejo pode demorar e gerar um arquivo grande.
backup-warn-continue = Fazer backup mesmo assim
backup-warn-cancel = Cancelar
backup-dialog-title = Salvar backup como
backup-progress-title = Fazendo backup
backup-progress-table = Tabela { $done } de { $total }
backup-progress-rows = { $done } / { $total } linhas
backup-progress-current = Atual: { $table }
backup-cancel-button = Cancelar
backup-done-title = Backup concluído
backup-done-summary = { $tables } tabela(s), { $rows } linhas exportadas.
backup-done-cancelled = Backup cancelado — o arquivo contém um despejo parcial.
backup-done-failures = Não foi possível ler { $count } tabela(s), que foram ignoradas.
backup-done-truncations = { $count } tabela(s) foram truncadas no meio.
backup-failed-title = Falha no backup
backup-close-button = Fechar

# ADR-0050: persisted, user-editable backup warn threshold.
backup-settings-menu = Backup
backup-threshold-label = Avisar acima de (linhas)
backup-threshold-hint = Mostra o aviso de banco de dados grande antes do despejo quando o total de linhas excede este valor. Salvo em ui-settings.toml.
