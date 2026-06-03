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
