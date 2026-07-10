# English — source of truth (ADR-0015). Every other locale falls back
# here for missing keys. Keep this file complete; add new keys here
# first, then propagate.

app-title = dbboard

tables-heading = Tables
tables-empty = (no tables)

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

# ADR-0023: AI assistance panel (Phase 4 Stage 1). The menu entry and
# the panel are both hidden when no provider is wired at startup, so
# these keys are only ever rendered behind that gate. Error prefixes
# mirror the `AiError` variants; the body of each error stays in the
# language the provider returned it in (typically English).
ai-menu-button = AI Assistant
ai-panel-title = AI Assistant
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
