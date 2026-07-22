app-title = dbboard

tables-heading = Tabelle
tables-empty = (nessuna tabella)
tables-context-select = Seleziona tutte le righe
tables-context-count = Conta le righe

sql-heading = SQL
sql-run-button = Esegui

history-title = Cronologia ({ $count })
history-empty = (nessuna query recente)

result-heading = Risultato
result-empty = (esegui una query)
result-affected = OK ({ $rows } righe modificate)
result-copy-all = Copia
result-copy-all-hint = Copia l'intero risultato negli appunti come TSV (incollalo in un foglio di calcolo)
result-export-csv = Salva CSV…
result-export-error = Impossibile salvare il file CSV
result-copy-selected = Copia selezione
result-copy-selected-hint = Copia le righe selezionate negli appunti come TSV
result-export-selected-csv = Salva selezione come CSV…
result-clear-selection = Cancella selezione
result-selected-count = { $count } selezionate
result-select-row-hint = Clic per selezionare la riga (Ctrl / Maiusc per selezione multipla)
result-sort-hint = Clicca per ordinare; Ctrl / Maiusc per aggiungere un livello

error-prefix-connection = Errore di connessione
error-prefix-query = Errore di query
error-prefix-schema = Errore di schema
error-prefix-type-conversion = Errore di conversione di tipo
error-prefix-capability = Funzionalità non disponibile

connections-window-title = Connessioni
connections-restart-hint = Le modifiche avranno effetto al prossimo avvio di dbboard.
connections-list-empty = (nessuna connessione configurata)
connections-add-button = Aggiungi
connections-edit-button = Modifica
connections-delete-button = Elimina
connections-save-button = Salva
connections-cancel-button = Annulla
connections-confirm-delete = Eliminare questa connessione?
connections-field-id = ID
connections-field-name = Nome
connections-field-kind = Tipo
connections-field-turso-path = Percorso del database
connections-field-d1-account = ID account
connections-field-d1-database = ID database
connections-field-d1-base-url = URL base (opzionale)
connections-field-d1-token = Token API
connections-field-pg-url = URL di connessione
connections-replace-token = Sostituisci token
connections-replace-url = Sostituisci URL
connections-connect-button = Connetti
connections-reconnect-button = Riconnetti
connections-active-marker = (attiva)
connections-switch-error = Connessione non riuscita

language-menu = Lingua
theme-menu = Tema
theme-auto = Automatico
theme-light = Chiaro
theme-dark = Scuro
help-menu = Aiuto
help-docs-hint = Consulta README.md e docs/ per la configurazione e le guide di connessione.
help-repo-link = Progetto su GitHub
help-ai-about-title = Informazioni sull'Assistente IA
help-ai-about-body = L'Assistente IA spiega un'istruzione SQL in linguaggio semplice e redige una query SQL a partire da una descrizione che digiti; per i suggerimenti legge anche i nomi delle tue tabelle e colonne. Non esegue mai SQL, non scrive mai nel tuo database e non invia mai righe di dati da nessuna parte: non accade nulla finché non copi una bozza nell'editor e la esegui tu stesso. È necessaria una chiave API, che viene memorizzata nel gestore delle credenziali del tuo sistema operativo.

ai-menu-button = Assistente IA
ai-panel-title = Assistente IA
ai-scope-hint = Spiega l'SQL e redige query da una descrizione. Non esegue mai SQL né modifica dati: sei tu a controllare ed eseguire tutto.
ai-mode-explain = Spiega SQL
ai-mode-suggest = Suggerisci SQL
ai-input-explain = SQL da spiegare:
ai-input-suggest = Descrivi la query desiderata:
ai-send-button = Invia
ai-busy = In attesa del provider…
ai-empty = (Nessuna risposta — scrivi un prompt sopra e premi Invia)
ai-error-prefix-configuration = Errore di configurazione IA
ai-error-prefix-network = Errore di rete IA
ai-error-prefix-provider = Errore del provider IA
ai-error-prefix-quota = Quota IA superata
ai-error-prefix-cancelled = Richiesta IA annullata

# ADR-0026 Phase 4 Stage 2 Group B: streaming + annullamento cooperativo
# + contatore di token.
ai-cancel-button = Annulla
ai-cancelled-message = Annullato.
ai-tokens-meter = Token: { $tin } in / { $tout } out

# ADR-0025 Phase 4 Stage 2 Group A slice (b): finestra impostazioni provider IA.
ai-settings-menu-button = Provider IA
ai-settings-window-title = Provider IA
ai-settings-list-empty = (nessun provider IA configurato)
ai-settings-add-button = Aggiungi
ai-settings-edit-button = Modifica
ai-settings-delete-button = Elimina
ai-settings-save-button = Salva
ai-settings-cancel-button = Annulla
ai-settings-use-button = Usa
ai-settings-confirm-delete = Eliminare questo provider IA?
ai-settings-active-marker = (attivo)
ai-settings-field-id = Id
ai-settings-field-name = Nome
ai-settings-field-kind = Tipo
ai-settings-field-model = Modello (opzionale)
ai-settings-field-api-key = Chiave API
ai-settings-replace-api-key = Sostituisci chiave API
ai-settings-kind-anthropic = Anthropic
ai-active-with-name = Attivo: { $name }

ai-include-details = Includi dettagli delle colonne
ai-prefetching = Recupero degli schemi delle tabelle…
ai-prefetch-warning = Impossibile descrivere { $count } tabella/e; si continua senza di esse.

# ADR-0030 result grid: truncated long / multi-line cell values.
cell-expand-hint = Mostra valore completo
cell-full-text-title = Valore della cella
cell-copy = Copia

# ADR-0030 auto-limit guard for bare SELECTs.
auto-limit-checkbox = LIMIT { $count }
auto-limit-hint = Aggiunge un LIMIT ai SELECT senza limite così una scansione illimitata non blocca linterfaccia. Scrivi il tuo LIMIT o deseleziona per sovrascrivere.

# ADR-0031 structure tab.
tab-results = Risultato
tab-structure = Struttura
structure-empty = (clicca una tabella per vederne la struttura)
structure-loading = Descrizione tabella…
structure-no-columns = (nessuna colonna)
structure-col-ordinal = #
structure-col-name = Nome
structure-col-type = Tipo
structure-col-nullable = Null
structure-col-pk = Chiave
structure-col-default = Predef.
structure-col-note = Nota
structure-note-hint = Aggiungi una nota…
structure-table-note = Nota tabella

edit-save-button = Salva
edit-discard-button = Annulla
edit-staged-count = { $count } modifica/e in sospeso
edit-set-null = Imposta NULL
edit-revert-cell = Ripristina cella
edit-cell-hint = Doppio clic per modificare · clic destro per NULL
edit-save-unexpected-rows = Salvataggio interrotto: prevista 1 riga, { $rows } interessate

# ADR-0049 backup (logical dump).
backup-button = Backup…
backup-button-hint = Esporta le tabelle di questo database in un file SQL
backup-planning = Preparazione del backup…
backup-warn-title = Database di grandi dimensioni
backup-warn-body = Questo database contiene { $rows } righe in tutte le sue tabelle. L'esportazione può richiedere tempo e generare un file di grandi dimensioni.
backup-warn-continue = Esegui comunque il backup
backup-warn-cancel = Annulla
backup-dialog-title = Salva backup come
backup-progress-title = Backup in corso
backup-progress-table = Tabella { $done } di { $total }
backup-progress-rows = { $done } / { $total } righe
backup-progress-current = Corrente: { $table }
backup-cancel-button = Annulla
backup-done-title = Backup completato
backup-done-summary = Esportate { $tables } tabella/e, { $rows } righe.
backup-done-cancelled = Backup annullato — il file contiene un'esportazione parziale.
backup-done-failures = Impossibile leggere { $count } tabella/e, che sono state saltate.
backup-done-truncations = { $count } tabella/e sono state troncate a metà.
backup-failed-title = Backup non riuscito
backup-close-button = Chiudi
