//! Live round-trip test against a real MySQL-wire database (`MySQL` 8,
//! `MariaDB`, `PlanetScale`, ...).
//!
//! Network-bound, so it is gated behind an environment variable (see
//! `docs/architecture.md`): it self-skips unless `DBBOARD_MYSQL_URL` is set.
//! With it set it exercises the full
//! `connect → ping → DDL → DML → SELECT → list_tables` path plus
//! `describe_table` / `foreign_keys`, and asserts the text-format value
//! mapping (every value comes back as `Value::Text`, NULL as `Value::Null`).

use dbboard_core::{DatabaseAdapter, Value};
use dbboard_mysql::{MySqlAdapter, MySqlConfig};

fn config_from_env() -> Option<MySqlConfig> {
    Some(MySqlConfig {
        url: std::env::var("DBBOARD_MYSQL_URL").ok()?,
    })
}

/// A 0..=9 derived table. Cross-joining four of these yields exactly `10_000`
/// rows without touching `cte_max_recursion_depth` — `MySQL` has no
/// `generate_series`, and a recursive CTE would trip the default `1_000` depth
/// cap long before the row cap.
const DIGITS: &str = "(SELECT 0 AS d UNION SELECT 1 UNION SELECT 2 UNION SELECT 3 \
     UNION SELECT 4 UNION SELECT 5 UNION SELECT 6 UNION SELECT 7 \
     UNION SELECT 8 UNION SELECT 9)";

#[tokio::test]
async fn select_one_round_trips() {
    let Some(config) = config_from_env() else {
        eprintln!("skipping: DBBOARD_MYSQL_URL not set");
        return;
    };

    let adapter = MySqlAdapter::connect(config).await.expect("connect");
    adapter.ping().await.expect("ping");

    let result = adapter.query("SELECT 1 AS one").await.expect("query");
    assert_eq!(result.columns.len(), 1);
    assert_eq!(result.columns[0].name, "one");
    assert_eq!(result.rows.len(), 1);
    // Text protocol: the integer arrives as its textual representation.
    assert_eq!(result.rows[0].get(0), Some(&Value::Text("1".to_string())));
}

#[tokio::test]
async fn dml_and_select_round_trip() {
    let Some(config) = config_from_env() else {
        eprintln!("skipping: DBBOARD_MYSQL_URL not set");
        return;
    };

    let adapter = MySqlAdapter::connect(config).await.expect("connect");

    // Unique name so concurrent / repeated runs don't collide.
    let table = format!("dbboard_mysql_it_{}", std::process::id());
    let drop_sql = format!("DROP TABLE IF EXISTS `{table}`");

    adapter.query(&drop_sql).await.expect("pre-drop");
    adapter
        .query(&format!(
            "CREATE TABLE `{table}` (id INT PRIMARY KEY, name VARCHAR(255))"
        ))
        .await
        .expect("create");

    let inserted = adapter
        .query(&format!(
            "INSERT INTO `{table}` (id, name) VALUES (1, 'alice'), (2, NULL)"
        ))
        .await
        .expect("insert");
    assert_eq!(inserted.rows_affected, 2);
    assert!(inserted.rows.is_empty());

    let selected = adapter
        .query(&format!("SELECT id, name FROM `{table}` ORDER BY id"))
        .await
        .expect("select");
    assert_eq!(selected.rows.len(), 2);
    assert_eq!(selected.rows[0].get(0), Some(&Value::Text("1".to_string())));
    assert_eq!(
        selected.rows[0].get(1),
        Some(&Value::Text("alice".to_string()))
    );
    // NULL stays NULL rather than the string "NULL".
    assert_eq!(selected.rows[1].get(1), Some(&Value::Null));

    // The new table shows up in introspection.
    let tables = adapter.list_tables().await.expect("list tables");
    assert!(
        tables.iter().any(|t| t.name == table),
        "created table {table} not found in {tables:?}"
    );

    adapter.query(&drop_sql).await.expect("cleanup drop");
}

/// `describe_table` round-trip: columns arrive in ordinal order with
/// nullability, defaults, and the composite primary key in key order
/// (ADR-0028). Missing tables surface as `DbError::Query`. An unqualified
/// `TableInfo` resolves against `DATABASE()`.
#[tokio::test]
async fn describe_table_round_trips_columns_and_composite_pk() {
    use dbboard_core::TableInfo;
    let Some(config) = config_from_env() else {
        eprintln!("skipping: DBBOARD_MYSQL_URL not set");
        return;
    };

    let adapter = MySqlAdapter::connect(config).await.expect("connect");

    let table = format!("dbboard_mysql_describe_{}", std::process::id());
    let drop_sql = format!("DROP TABLE IF EXISTS `{table}`");
    adapter.query(&drop_sql).await.expect("pre-drop");
    // VARCHAR, not TEXT: MySQL rejects a literal DEFAULT on a TEXT/BLOB column.
    adapter
        .query(&format!(
            "CREATE TABLE `{table}` (\
             order_id INT NOT NULL, \
             line_no INT NOT NULL, \
             sku VARCHAR(255) NOT NULL DEFAULT 'unknown', \
             PRIMARY KEY (order_id, line_no))"
        ))
        .await
        .expect("create");

    let info = TableInfo::unqualified(&table);
    let schema = adapter.describe_table(&info).await.expect("describe");

    assert_eq!(schema.table, info);
    let names: Vec<&str> = schema.columns.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, vec!["order_id", "line_no", "sku"]);
    let ordinals: Vec<u32> = schema.columns.iter().map(|c| c.ordinal).collect();
    assert_eq!(ordinals, vec![1, 2, 3]);
    assert_eq!(
        schema.primary_key,
        vec!["order_id".to_owned(), "line_no".to_owned()]
    );

    let sku = &schema.columns[2];
    assert!(!sku.nullable);
    assert!(!sku.primary_key);
    assert!(
        sku.default_value
            .as_deref()
            .is_some_and(|d| d.contains("unknown")),
        "expected a default mentioning 'unknown', got {:?}",
        sku.default_value
    );

    adapter.query(&drop_sql).await.expect("cleanup drop");

    let err = adapter
        .describe_table(&TableInfo::unqualified(&table))
        .await
        .expect_err("describing a dropped table should fail");
    assert!(
        matches!(err, dbboard_core::DbError::Query(_)),
        "expected DbError::Query, got {err:?}"
    );
}

/// `foreign_keys` round-trip (ADR-0054): a child table with a single-column
/// and a composite reference reports both, with local/referenced columns
/// aligned in key order and the named constraint preserved. A table without
/// references reports none. FK constraints are declared table-level — `MySQL`
/// silently ignores an inline column-level `REFERENCES`.
#[tokio::test]
async fn foreign_keys_round_trip_reports_single_and_composite_edges() {
    use dbboard_core::TableInfo;
    let Some(config) = config_from_env() else {
        eprintln!("skipping: DBBOARD_MYSQL_URL not set");
        return;
    };

    let adapter = MySqlAdapter::connect(config).await.expect("connect");

    let pid = std::process::id();
    let parent = format!("dbboard_mysql_fk_parent_{pid}");
    let composite = format!("dbboard_mysql_fk_composite_{pid}");
    let child = format!("dbboard_mysql_fk_child_{pid}");

    // Drop children before parents to satisfy referential order, then build
    // parents before children. Statements run one at a time.
    for stmt in [
        format!("DROP TABLE IF EXISTS `{child}`"),
        format!("DROP TABLE IF EXISTS `{composite}`"),
        format!("DROP TABLE IF EXISTS `{parent}`"),
        format!("CREATE TABLE `{parent}` (id INT PRIMARY KEY)"),
        format!("CREATE TABLE `{composite}` (a INT, b INT, PRIMARY KEY (a, b))"),
        format!(
            "CREATE TABLE `{child}` (\
             id INT PRIMARY KEY, \
             parent_id INT, \
             ca INT, cb INT, \
             FOREIGN KEY (parent_id) REFERENCES `{parent}` (id), \
             CONSTRAINT `{child}_composite_fk` FOREIGN KEY (ca, cb) \
             REFERENCES `{composite}` (a, b))"
        ),
    ] {
        adapter.query(&stmt).await.expect("setup ddl");
    }

    let edges = adapter
        .foreign_keys(&TableInfo::unqualified(&child))
        .await
        .expect("foreign_keys");
    assert_eq!(edges.len(), 2, "expected two edges, got {edges:?}");

    let single = edges
        .iter()
        .find(|e| e.columns == vec!["parent_id".to_string()])
        .expect("single-column edge");
    // MySQL qualifies the referenced table with DATABASE(); assert on the
    // name and columns rather than the (run-dependent) schema.
    assert_eq!(single.referenced_table.name, parent);
    assert_eq!(single.referenced_columns, vec!["id".to_string()]);

    let comp = edges
        .iter()
        .find(|e| e.columns == vec!["ca".to_string(), "cb".to_string()])
        .expect("composite edge");
    assert_eq!(comp.referenced_table.name, composite);
    assert_eq!(
        comp.referenced_columns,
        vec!["a".to_string(), "b".to_string()]
    );
    assert_eq!(
        comp.constraint_name.as_deref(),
        Some(format!("{child}_composite_fk").as_str())
    );

    // A table with no outbound references reports none.
    let parent_edges = adapter
        .foreign_keys(&TableInfo::unqualified(&parent))
        .await
        .expect("foreign_keys on parent");
    assert!(
        parent_edges.is_empty(),
        "parent has no FKs: {parent_edges:?}"
    );

    for stmt in [
        format!("DROP TABLE IF EXISTS `{child}`"),
        format!("DROP TABLE IF EXISTS `{composite}`"),
        format!("DROP TABLE IF EXISTS `{parent}`"),
    ] {
        adapter.query(&stmt).await.expect("cleanup drop");
    }
}

/// Exactly at the row cap: four cross-joined 0..=9 digit tables yield
/// `MAX_RESULT_ROWS` (`10_000`) rows, which must succeed. One row past the cap
/// is a `DbError::Query`, proving the buffered check fires mid-stream.
#[tokio::test]
async fn query_at_the_row_cap_returns_all_rows() {
    use dbboard_core::MAX_RESULT_ROWS;
    let Some(config) = config_from_env() else {
        eprintln!("skipping: DBBOARD_MYSQL_URL not set");
        return;
    };
    assert_eq!(
        MAX_RESULT_ROWS, 10_000,
        "digit cross-join assumes a 10_000 cap"
    );

    let adapter = MySqlAdapter::connect(config).await.expect("connect");

    // 0..=9999 + 1 → exactly 10_000 rows.
    let sql = format!(
        "SELECT d1.d + d2.d*10 + d3.d*100 + d4.d*1000 + 1 AS n \
         FROM {DIGITS} d1 CROSS JOIN {DIGITS} d2 \
         CROSS JOIN {DIGITS} d3 CROSS JOIN {DIGITS} d4"
    );
    let result = adapter.query(&sql).await.expect("query at cap");
    assert_eq!(result.rows.len(), MAX_RESULT_ROWS);

    // One more row than the cap must surface as an error, not a truncation.
    let over_sql = format!("{sql} UNION ALL SELECT 10001");
    let Err(err) = adapter.query(&over_sql).await else {
        panic!("query over cap should fail");
    };
    let dbboard_core::DbError::Query(msg) = err else {
        panic!("expected DbError::Query, got {err:?}");
    };
    assert!(
        msg.contains(&MAX_RESULT_ROWS.to_string()),
        "error should mention the cap, got: {msg}"
    );
}

/// `query_read_only` caps by *truncating*, not erroring (ADR-0046): a
/// 100-row digit series with `max_rows = 10` comes back with exactly 10 rows.
#[tokio::test]
async fn read_only_query_truncates_to_max_rows() {
    let Some(config) = config_from_env() else {
        eprintln!("skipping: DBBOARD_MYSQL_URL not set");
        return;
    };
    let adapter = MySqlAdapter::connect(config).await.expect("connect");
    // 0..=99 + 1 → 100 rows.
    let sql = format!(
        "SELECT d1.d + d2.d*10 + 1 AS n \
         FROM {DIGITS} d1 CROSS JOIN {DIGITS} d2 ORDER BY n"
    );
    let result = adapter.query_read_only(&sql, 10).await.expect("read-only");
    assert_eq!(result.rows.len(), 10);
    assert_eq!(result.rows[0].get(0), Some(&Value::Text("1".to_string())));
}

/// Wire-protocol regression, the `MySQL` half of the Postgres
/// `read_only_decodes_wide_types_as_printed_text` case: the read-only path
/// must use the text protocol (`COM_QUERY`), because `decode_cell` reads each
/// cell's bytes as its printed representation.
///
/// `sqlx::query` prepares the statement, so the server answers with the
/// *binary* resultset — and there the corruption is silent rather than loud:
/// `decode_cell` falls back to `Value::Blob` when the bytes are not UTF-8, so
/// an `INT` comes back as an opaque blob and a `BIGINT` as whatever those
/// eight bytes happen to spell. Asserting the printed text pins the protocol.
#[tokio::test]
async fn read_only_decodes_wide_types_as_printed_text() {
    let Some(config) = config_from_env() else {
        eprintln!("skipping: DBBOARD_MYSQL_URL not set");
        return;
    };
    let adapter = MySqlAdapter::connect(config).await.expect("connect");
    let sql = "SELECT CAST(42 AS SIGNED) AS small, \
                      CAST(1234567890123 AS SIGNED) AS wide, \
                      CAST('2026-07-30 12:34:56' AS DATETIME) AS ts";
    let result = adapter.query_read_only(sql, 10).await.expect("read-only");
    assert_eq!(result.rows.len(), 1);
    let row = &result.rows[0];
    assert_eq!(row.get(0), Some(&Value::Text("42".to_string())));
    assert_eq!(row.get(1), Some(&Value::Text("1234567890123".to_string())));
    assert_eq!(
        row.get(2),
        Some(&Value::Text("2026-07-30 12:34:56".to_string()))
    );
}

/// `table_ddl` round-trip. `SHOW CREATE TABLE` is the one metadata path with no
/// `CAST(… AS CHAR)` escape hatch — `SHOW` takes no expressions — so if the
/// server hands the statement back under a binary type, reading it as bytes is
/// the only way through. A multi-byte identifier is included because the same
/// read is what turns those bytes back into a string.
#[tokio::test]
async fn table_ddl_round_trips_including_multibyte_identifiers() {
    use dbboard_core::TableInfo;
    let Some(config) = config_from_env() else {
        eprintln!("skipping: DBBOARD_MYSQL_URL not set");
        return;
    };

    let adapter = MySqlAdapter::connect(config).await.expect("connect");

    let table = format!("dbboard_mysql_ddl_{}", std::process::id());
    let drop_sql = format!("DROP TABLE IF EXISTS `{table}`");
    adapter.query(&drop_sql).await.expect("pre-drop");
    adapter
        .query(&format!(
            "CREATE TABLE `{table}` (\
             id INT NOT NULL PRIMARY KEY, \
             `点検日` DATE NULL)"
        ))
        .await
        .expect("create");

    let ddl = adapter
        .table_ddl(&TableInfo::unqualified(&table))
        .await
        .expect("table_ddl");

    assert!(ddl.contains(&table), "DDL should name the table: {ddl}");
    assert!(
        ddl.contains("点検日"),
        "a multi-byte column name must survive the decode: {ddl}"
    );

    adapter.query(&drop_sql).await.expect("cleanup drop");
}
