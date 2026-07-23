app-title = dbboard

tables-heading = Tabellen
tables-empty = (keine Tabellen)
tables-context-select = Alle Zeilen auswählen
tables-context-count = Zeilen zählen

sql-heading = SQL
sql-run-button = Ausführen

history-title = Verlauf ({ $count })
history-empty = (keine letzten Abfragen)

result-heading = Ergebnis
result-empty = (Abfrage ausführen)
result-affected = OK ({ $rows } Zeilen betroffen)
result-copy-all = Kopieren
result-copy-all-hint = Das gesamte Ergebnis als TSV in die Zwischenablage kopieren (in eine Tabelle einfügbar)
result-export-csv = CSV speichern…
result-export-error = Die CSV-Datei konnte nicht gespeichert werden
result-copy-selected = Auswahl kopieren
result-copy-selected-hint = Ausgewählte Zeilen als TSV in die Zwischenablage kopieren
result-export-selected-csv = Auswahl als CSV speichern…
result-clear-selection = Auswahl aufheben
result-selected-count = { $count } ausgewählt
result-select-row-hint = Klicken, um die Zeile auszuwählen (Strg / Umschalt für Mehrfachauswahl)
result-sort-hint = Zum Sortieren klicken; Strg / Umschalt für eine weitere Ebene

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
connections-reconnect-button = Erneut verbinden
connections-active-marker = (aktiv)
connections-switch-error = Verbindung fehlgeschlagen

language-menu = Sprache
theme-menu = Design
theme-auto = Automatisch
theme-light = Hell
theme-dark = Dunkel
help-menu = Hilfe
help-docs-hint = Siehe README.md und docs/ für Einrichtung und Verbindungsanleitungen.
help-repo-link = Projekt auf GitHub
help-ai-about-title = Über den KI-Assistenten
help-ai-about-body = Der KI-Assistent erklärt eine SQL-Anweisung in verständlicher Sprache und entwirft aus einer von Ihnen eingegebenen Beschreibung eine SQL-Abfrage; für Vorschläge liest er außerdem Ihre Tabellen- und Spaltennamen. Er führt niemals SQL aus, schreibt niemals in Ihre Datenbank und sendet niemals Tabellenzeilen irgendwohin – nichts geschieht, bis Sie einen Entwurf in den Editor kopieren und selbst ausführen. Ein API-Schlüssel ist erforderlich und wird in der Anmeldeinformationsverwaltung Ihres Betriebssystems gespeichert.

ai-menu-button = KI-Assistent
ai-panel-title = KI-Assistent
ai-scope-hint = Erklärt SQL und entwirft Abfragen aus einer Beschreibung. Es führt niemals SQL aus und ändert keine Daten – Sie prüfen und führen alles selbst aus.
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
structure-col-note = Notiz
structure-note-hint = Notiz hinzufügen…
structure-table-note = Tabellennotiz

edit-save-button = Speichern
edit-discard-button = Verwerfen
edit-staged-count = { $count } ausstehende Änderung(en)
edit-set-null = Auf NULL setzen
edit-revert-cell = Zelle zurücksetzen
edit-cell-hint = Zum Bearbeiten doppelklicken · Rechtsklick für NULL
edit-save-unexpected-rows = Speichern gestoppt: 1 Zeile erwartet, { $rows } betroffen

# ADR-0049 backup (logical dump).
backup-button = Sicherung…
backup-button-hint = Die Tabellen dieser Datenbank in eine SQL-Datei exportieren
backup-planning = Sicherung wird vorbereitet…
backup-warn-title = Große Datenbank
backup-warn-body = Diese Datenbank enthält { $rows } Zeilen über alle Tabellen. Ein Export kann eine Weile dauern und eine große Datei erzeugen.
backup-warn-continue = Trotzdem sichern
backup-warn-cancel = Abbrechen
backup-dialog-title = Sicherung speichern unter
backup-progress-title = Sicherung läuft
backup-progress-table = Tabelle { $done } von { $total }
backup-progress-rows = { $done } / { $total } Zeilen
backup-progress-current = Aktuell: { $table }
backup-cancel-button = Abbrechen
backup-done-title = Sicherung abgeschlossen
backup-done-summary = { $tables } Tabelle(n), { $rows } Zeilen exportiert.
backup-done-cancelled = Sicherung abgebrochen — die Datei enthält einen unvollständigen Export.
backup-done-failures = { $count } Tabelle(n) konnten nicht gelesen werden und wurden übersprungen.
backup-done-truncations = { $count } Tabelle(n) wurden vorzeitig abgeschnitten.
backup-failed-title = Sicherung fehlgeschlagen
backup-close-button = Schließen

# ADR-0050: persisted, user-editable backup warn threshold.
backup-settings-menu = Sicherung
backup-threshold-label = Warnen ab (Zeilen)
backup-threshold-hint = Zeigt vor dem Dump die Warnung für große Datenbanken an, wenn die Gesamtzahl der Zeilen diesen Wert überschreitet. Wird in ui-settings.toml gespeichert.
