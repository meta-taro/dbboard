# English — source of truth (ADR-0015). Every other locale falls back
# here for missing keys. Keep this file complete; add new keys here
# first, then propagate.

app-title = dbboard

tables-heading = Tables
tables-empty = (no tables)

sql-heading = SQL
sql-run-button = Run

history-title = History ({ $count })
history-empty = (no recent queries)

result-heading = Result
result-empty = (run a query)
result-affected = OK ({ $rows } rows affected)

# DbError category prefixes. The error *body* is the server-returned
# English string (ADR-0009 / ADR-0015); only the prefix is translated.
error-prefix-connection = Connection error
error-prefix-query = Query error
error-prefix-schema = Schema error
error-prefix-type-conversion = Type conversion error
error-prefix-capability = Capability unavailable

# Connection management window (ADR-0016). HeidiSQL mental model: this
# window manages stored entries; the running process keeps talking to
# whichever entry it was launched with.
connections-window-title = Connections
connections-restart-hint = Changes apply on next launch of dbboard.
connections-list-empty = (no connections configured)
connections-add-button = Add
connections-edit-button = Edit
connections-delete-button = Delete
connections-save-button = Save
connections-cancel-button = Cancel
connections-confirm-delete = Delete this connection?
connections-field-id = Id
connections-field-name = Name
connections-field-kind = Kind
connections-field-turso-path = Database path
connections-field-d1-account = Account id
connections-field-d1-database = Database id
connections-field-d1-base-url = Base URL (optional)
connections-field-d1-token = API token
connections-field-pg-url = Connection URL
connections-replace-token = Replace token
connections-replace-url = Replace URL
# ADR-0020: per-row "Connect" button switches the running adapter in
# place; the active row is decorated with `connections-active-marker`
# and its Connect button is disabled.
connections-connect-button = Connect
connections-active-marker = (active)

# ADR-0022: runtime locale switcher. The menu-bar label itself is
# translated so a user who landed in the wrong locale still finds the
# switcher; the submenu entries below it stay in each locale's native
# name (`English`, `日本語`, `한국어`, …) which is hard-coded in
# `apps/dbboard` rather than translated.
language-menu = Language
