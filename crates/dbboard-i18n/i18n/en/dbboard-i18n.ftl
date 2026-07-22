# English — source of truth (ADR-0015). Every other locale falls back
# here for missing keys. Keep this file complete; add new keys here
# first, then propagate.

app-title = dbboard

tables-heading = Tables
tables-empty = (no tables)
# Right-click a sidebar table for quick starter queries dropped into the
# editor. Read-only by design — no DELETE/DROP — since this ships to a
# data-collection user where a mis-click must never be destructive.
tables-context-select = Select all rows
tables-context-count = Count rows

sql-heading = SQL
sql-run-button = Run
# ADR-0030 auto-limit guard: appended to bare SELECTs so an unbounded scan
# cannot freeze the UI. Overridable — write your own LIMIT or uncheck.
auto-limit-checkbox = LIMIT { $count }
auto-limit-hint = Cap bare SELECTs with LIMIT so an unbounded scan can't freeze the UI. Write your own LIMIT or uncheck to override.

history-title = History ({ $count })
history-empty = (no recent queries)

result-heading = Result
result-empty = (run a query)
result-affected = OK ({ $rows } rows affected)
result-copy-all = Copy
result-copy-all-hint = Copy the whole result to the clipboard as TSV (paste into a spreadsheet)
result-export-csv = Save CSV…
result-export-error = Could not save the CSV file
result-copy-selected = Copy selected
result-copy-selected-hint = Copy the selected rows to the clipboard as TSV
result-export-selected-csv = Save selected as CSV…
result-clear-selection = Clear selection
result-selected-count = { $count } selected
result-select-row-hint = Click to select the row (Ctrl / Shift for multiple)
result-sort-hint = Click to sort; Ctrl / Shift to add a level

# ADR-0031 structure tab: clicking a sidebar table describes it and shows
# its columns here, next to the query result.
tab-results = Result
tab-structure = Structure
structure-empty = (click a table to inspect its structure)
structure-loading = Describing table…
structure-no-columns = (no columns)
structure-col-ordinal = #
structure-col-name = Name
structure-col-type = Type
structure-col-nullable = Null
structure-col-pk = Key
structure-col-default = Default
structure-col-note = Note
structure-note-hint = Add a note…
structure-table-note = Table note

# ADR-0030 result grid: long / multi-line cell values are truncated with
# an ellipsis and an expand button that opens the full text in a popup.
cell-expand-hint = Show full value
cell-full-text-title = Cell value
cell-copy = Copy

# DbError category prefixes. The error *body* is the server-returned
# English string (ADR-0009 / ADR-0015); only the prefix is translated.
error-prefix-connection = Connection error
error-prefix-query = Query error
error-prefix-schema = Schema error
error-prefix-type-conversion = Type conversion error
error-prefix-capability = Capability unavailable

# ADR-0039 unified error display. Every app-side error is rendered as a
# localized message plus its original English text, both selectable and
# copyable. These keys are the localized half; the English half comes from
# each error type's own `Display`. `error-copy-button` copies both halves.
error-copy-button = Copy
error-copy-hint = Copy this error (translation and original English) to the clipboard
error-original-label = Original

# SecretError (shared by connection and AI-provider stores).
secret-error-not-found = No secret is stored for this connection (reference: { $reference }). Seed it in this machine's credential store first.
secret-error-backend = The secret store operation failed (reference: { $reference }): { $detail }

# ConfigError — connection store load / validation / edit failures.
config-error-parse = Could not parse the configuration file: { $detail }
config-error-unsupported-version = Unsupported configuration version: { $found } (only version { $expected } is supported).
config-error-duplicate-id = Duplicate connection id: { $id }
config-error-io = Configuration file access failed: { $detail }
config-error-serialize = Could not write the configuration: { $detail }
config-error-no-config-dir = Could not determine a per-user configuration directory.
config-error-not-found = No connection found with id: { $id }
config-error-kind-mismatch = The kind of connection { $id } cannot change on edit; delete it and add it again instead.

# BundleError — encrypted connection bundle export / import (ADR-0038).
config-error-bundle-passphrase-short = The passphrase must be at least { $min } characters.
config-error-bundle-serialize = Could not prepare the bundle contents: { $detail }
config-error-bundle-incorrect-passphrase = Incorrect passphrase.
config-error-bundle-corrupt = The file is corrupt or was not produced by dbboard.
config-error-bundle-unsupported-version = Unsupported bundle version: { $found }.
config-error-bundle-invalid-payload = The bundle contents are not a valid dbboard payload: { $detail }
config-error-bundle-io = Bundle file access failed: { $detail }

# AiSettingsError — AI-provider store load / validation / edit failures.
ai-settings-error-parse = Could not parse the AI providers file: { $detail }
ai-settings-error-unsupported-version = Unsupported AI providers version: { $found } (only version { $expected } is supported).
ai-settings-error-duplicate-id = Duplicate AI provider id: { $id }
ai-settings-error-unknown-active-id = The active AI provider id is unknown: { $id }
ai-settings-error-io = AI providers file access failed: { $detail }
ai-settings-error-serialize = Could not write the AI providers file: { $detail }
ai-settings-error-no-config-dir = Could not determine a per-user configuration directory.
ai-settings-error-not-found = No AI provider found with id: { $id }
ai-settings-error-kind-mismatch = The kind of AI provider { $id } cannot change on edit; delete it and add it again instead.

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
connections-reconnect-button = Reconnect
connections-active-marker = (active)
connections-switch-error = Could not connect

# ADR-0038: encrypted export / import of the whole connection store.
# The bundle is passphrase-encrypted (age scrypt + ChaCha20-Poly1305);
# import skips ids that already exist and reports them.
connections-export-button = Export…
connections-import-button = Import…
connections-export-heading = Export connections
connections-import-heading = Import connections
connections-export-passphrase-hint = Choose a passphrase. You will need it to import this file later — it cannot be recovered.
connections-import-passphrase-hint = Enter the passphrase this file was exported with.
connections-passphrase = Passphrase
connections-passphrase-confirm = Confirm passphrase
connections-passphrase-mismatch = Passphrases do not match.
connections-export-do = Export
connections-import-do = Import
connections-choose-file = Choose file…
connections-no-file-chosen = (no file chosen)
connections-bundle-filter = dbboard bundle
connections-export-ok = Connections exported
connections-import-imported = Imported
connections-import-skipped = Skipped

# ADR-0022: runtime locale switcher. The menu-bar label itself is
# translated so a user who landed in the wrong locale still finds the
# switcher; the submenu entries below it stay in each locale's native
# name (`English`, `日本語`, `한국어`, …) which is hard-coded in
# `apps/dbboard` rather than translated.
language-menu = Language

# ADR-0041: colour-theme switcher. Sits next to the Language menu. `Auto`
# follows the OS light/dark setting; `Light` / `Dark` pin it. The choice
# is persisted to `ui-settings.toml`.
theme-menu = Theme
theme-auto = Auto
theme-light = Light
theme-dark = Dark

# Help menu (internal distribution). A small Help menu surfaces the
# running version and points non-technical collector users at the setup
# docs. The version line itself (`dbboard 0.1.0`) is assembled in
# `apps/dbboard` from `CARGO_PKG_VERSION`, so it is not translated here.
help-menu = Help
help-docs-hint = See README.md and docs/ for setup and connection guides.
help-repo-link = Project on GitHub
# ADR-0045 follow-up: an "About AI Assistant" block in the Help menu, kept
# in sync with `ai-scope-hint` so users cannot misread the assistant as
# something that runs SQL or changes data on its own.
help-ai-about-title = About AI Assistant
help-ai-about-body = The AI Assistant explains a SQL statement in plain language and drafts a SQL query from a description you type; for suggestions it also reads your table and column names. It never runs SQL, never writes to your database, and never sends table rows anywhere — nothing happens until you copy a draft into the editor and run it yourself. An API key is required and is stored in your operating system's credential manager.
# ADR-0040: startup update check. Shown in the Help menu only when a newer
# release exists; updating is manual (the link opens the release page).
help-update-available = Update available: { $version }
help-update-link = Get the new version
help-update-notes = What's changed

# ADR-0023: AI assistance panel (Phase 4 Stage 1). The menu entry and
# the panel are both hidden when no provider is wired at startup, so
# these keys are only ever rendered behind that gate. Error prefixes
# mirror the `AiError` variants; the body of each error stays in the
# language the provider returned it in (typically English).
ai-menu-button = AI Assistant
ai-panel-title = AI Assistant
# One-line scope caption under the panel title. Keep the meaning aligned
# with `help-ai-about-body`.
ai-scope-hint = Explains SQL and drafts queries from a description. It never runs SQL or changes data — you review and run everything yourself.
ai-mode-explain = Explain SQL
ai-mode-suggest = Suggest SQL
ai-input-explain = SQL to explain:
ai-input-suggest = Describe the query you want:
ai-send-button = Send
ai-busy = Waiting for the provider…
ai-empty = (no response yet — write a prompt above and press Send)
ai-error-prefix-configuration = AI configuration error
ai-error-prefix-network = AI network error
ai-error-prefix-provider = AI provider error
ai-error-prefix-quota = AI quota exceeded
ai-error-prefix-cancelled = AI request cancelled

# ADR-0026 Phase 4 Stage 2 Group B: streaming + cooperative cancel +
# token meter. `ai-cancel-button` replaces the Send button while a
# request is in flight (both streaming and atomic paths). The token
# meter renders under both the in-flight streaming buffer and the final
# response with `{ $tin }` input + `{ $tout }` output (Anthropic
# usage.output_tokens is cumulative — the worker hands the panel the
# latest snapshot, the panel does not sum deltas).
ai-cancel-button = Cancel
ai-cancelled-message = Cancelled.
ai-tokens-meter = Tokens: { $tin } in / { $tout } out

# ADR-0025 Phase 4 Stage 2 Group A slice (b): AI provider Settings
# window. Sits next to the `connections-*` family and reuses its
# Add/Edit/Delete/Save/Cancel shape — kept as distinct keys so locales
# can diverge per context (e.g. "Save connection" vs "Save provider"
# may want different verbs in some languages).
ai-settings-menu-button = AI Providers
ai-settings-window-title = AI Providers
ai-settings-list-empty = (no AI providers configured)
ai-settings-add-button = Add
ai-settings-edit-button = Edit
ai-settings-delete-button = Delete
ai-settings-save-button = Save
ai-settings-cancel-button = Cancel
ai-settings-use-button = Use
ai-settings-confirm-delete = Delete this AI provider?
ai-settings-active-marker = (active)
ai-settings-field-id = Id
ai-settings-field-name = Name
ai-settings-field-kind = Kind
ai-settings-field-model = Model (optional)
ai-settings-field-api-key = API key
ai-settings-replace-api-key = Replace API key
ai-settings-kind-anthropic = Anthropic
# Subtitle on the AI assistant panel showing which provider is bound
# to the in-process slot right now. Rendered only when a provider is
# active (i.e. the panel is visible).
ai-active-with-name = Active: { $name }

# ADR-0028 Phase 4 Stage 2 Group D-1: full-DDL prompt enrichment. The
# checkbox only renders in Suggest mode when the active DB adapter
# reports `has_describe_table`; while the pre-Suggest describe fan-out
# is in flight the panel shows `ai-prefetching` (no Cancel — the
# describes are short and only the provider leg is cancellable). A
# partial describe failure is non-blocking: the warning renders with
# the failed-table count and the suggestion proceeds with whatever
# schemas arrived.
ai-include-details = Include column details
ai-prefetching = Fetching table schemas…
ai-prefetch-warning = Could not describe { $count } table(s); continuing without them.

edit-save-button = Save
edit-discard-button = Discard
edit-staged-count = { $count } pending edit(s)
edit-set-null = Set NULL
edit-revert-cell = Revert cell
edit-cell-hint = Double-click to edit · right-click for NULL
edit-save-unexpected-rows = Save stopped: expected 1 row, { $rows } affected
