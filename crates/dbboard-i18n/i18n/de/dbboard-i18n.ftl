app-title = dbboard

tables-heading = Tabellen
tables-empty = (keine Tabellen)

sql-heading = SQL
sql-run-button = Ausführen

history-title = Verlauf ({ $count })
history-empty = (keine letzten Abfragen)

result-heading = Ergebnis
result-empty = (Abfrage ausführen)
result-affected = OK ({ $rows } Zeilen betroffen)

error-prefix-connection = Verbindungsfehler
error-prefix-query = Abfragefehler
error-prefix-schema = Schemafehler
error-prefix-type-conversion = Typkonvertierungsfehler
error-prefix-capability = Funktion nicht verfügbar

connections-window-title = Verbindungen
connections-restart-hint = Änderungen werden beim nächsten Start von dbboard wirksam.
connections-list-empty = (keine Verbindungen konfiguriert)
connections-add-button = Hinzufügen
connections-edit-button = Bearbeiten
connections-delete-button = Löschen
connections-save-button = Speichern
connections-cancel-button = Abbrechen
connections-confirm-delete = Diese Verbindung löschen?
connections-field-id = ID
connections-field-name = Name
connections-field-kind = Typ
connections-field-turso-path = Datenbankpfad
connections-field-d1-account = Account-ID
connections-field-d1-database = Datenbank-ID
connections-field-d1-base-url = Basis-URL (optional)
connections-field-d1-token = API-Token
connections-field-pg-url = Verbindungs-URL
connections-replace-token = Token ersetzen
connections-replace-url = URL ersetzen
connections-connect-button = Verbinden
connections-active-marker = (aktiv)

language-menu = Sprache

ai-menu-button = KI-Assistent
ai-panel-title = KI-Assistent
ai-mode-explain = SQL erklären
ai-mode-suggest = SQL vorschlagen
ai-input-explain = Zu erklärendes SQL:
ai-input-suggest = Beschreiben Sie die gewünschte Abfrage:
ai-send-button = Senden
ai-busy = Warte auf Antwort des Anbieters…
ai-empty = (Noch keine Antwort — bitte oben einen Prompt eingeben und Senden drücken)
ai-error-prefix-configuration = KI-Konfigurationsfehler
ai-error-prefix-network = KI-Netzwerkfehler
ai-error-prefix-provider = KI-Anbieterfehler
ai-error-prefix-quota = KI-Kontingent überschritten
ai-error-prefix-cancelled = KI-Anfrage abgebrochen

# ADR-0026 Phase 4 Stage 2 Group B: Streaming + kooperative Stornierung
# + Token-Anzeige.
ai-cancel-button = Abbrechen
ai-cancelled-message = Abgebrochen.
ai-tokens-meter = Tokens: { $tin } ein / { $tout } aus

# ADR-0025 Phase 4 Stage 2 Group A slice (b): KI-Anbieter-Einstellungsfenster.
ai-settings-menu-button = KI-Anbieter
ai-settings-window-title = KI-Anbieter
ai-settings-list-empty = (keine KI-Anbieter konfiguriert)
ai-settings-add-button = Hinzufügen
ai-settings-edit-button = Bearbeiten
ai-settings-delete-button = Löschen
ai-settings-save-button = Speichern
ai-settings-cancel-button = Abbrechen
ai-settings-use-button = Verwenden
ai-settings-confirm-delete = Diesen KI-Anbieter löschen?
ai-settings-active-marker = (aktiv)
ai-settings-field-id = ID
ai-settings-field-name = Name
ai-settings-field-kind = Art
ai-settings-field-model = Modell (optional)
ai-settings-field-api-key = API-Schlüssel
ai-settings-replace-api-key = API-Schlüssel ersetzen
ai-settings-kind-anthropic = Anthropic
ai-active-with-name = Aktiv: { $name }

ai-include-details = Spaltendetails einbeziehen
ai-prefetching = Tabellenschemata werden abgerufen…
ai-prefetch-warning = { $count } Tabelle(n) konnten nicht beschrieben werden; es wird ohne sie fortgefahren.

# ADR-0030 result grid: truncated long / multi-line cell values.
cell-expand-hint = Vollständigen Wert anzeigen
cell-full-text-title = Zellenwert
cell-copy = Kopieren

# ADR-0030 auto-limit guard for bare SELECTs.
auto-limit-checkbox = LIMIT { $count }
auto-limit-hint = Fügt bloßen SELECTs ein LIMIT hinzu, damit ein unbegrenzter Scan die UI nicht einfriert. Eigenes LIMIT schreiben oder abwählen zum Überschreiben.

# ADR-0031 structure tab.
tab-results = Ergebnis
tab-structure = Struktur
structure-empty = (Tabelle anklicken, um die Struktur zu sehen)
structure-loading = Tabelle wird beschrieben…
structure-no-columns = (keine Spalten)
structure-col-ordinal = #
structure-col-name = Name
structure-col-type = Typ
structure-col-nullable = Null
structure-col-pk = Schlüssel
structure-col-default = Standard
