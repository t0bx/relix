/*
 * @author t0bx
 * @since 23.10.2026
 */

#![allow(clippy::unwrap_used)]

use relix_core::{
    CellChange, ChangeSet, ObjectKind, RowChange, Secret, StatementOutcome, TableQuery, TableRef,
    TlsMode, Value,
};
use relix_driver_api::{
    DatabaseConnection, DatabaseDriver, ErrorKind, ExecuteOptions, SqlConnection,
};
use relix_driver_mysql::{MariaDbDriver, MySqlDriver};
use relix_testing::integration::{TestEngine, integration_config, unique_name};
use std::sync::Arc;
use std::time::{Duration, Instant};

macro_rules! for_both_engines {
    ($($test:ident),* $(,)?) => {
        mod mysql {
            $(
                #[tokio::test(flavor = "multi_thread")]
                async fn $test() {
                    super::$test(super::TestEngine::MySql).await;
                }
            )*
        }

        mod mariadb {
            $(
                #[tokio::test(flavor = "multi_thread")]
                async fn $test() {
                    super::$test(super::TestEngine::MariaDb).await;
                }
            )*
        }
    };
}

for_both_engines!(
    connects_and_reports_server,
    rejects_wrong_password,
    fails_on_wrong_port,
    negotiates_tls,
    introspects_catalog,
    executes_statements,
    keeps_session_state,
    paginates_and_counts,
    applies_changes_in_one_transaction,
    rolls_back_conflicting_changes,
    cancels_running_statement,
);

struct Fixture {
    connection: Arc<dyn DatabaseConnection>,
    database: String,
}

impl Fixture {
    async fn open(engine: TestEngine) -> Option<Self> {
        let config = integration_config(engine)?;
        let database = config.settings.database.clone()?;
        let connection = driver_for(engine).connect(&config).await.unwrap();
        Some(Self {
            connection: Arc::from(connection),
            database,
        })
    }

    fn sql(&self) -> &dyn SqlConnection {
        self.connection.as_sql().unwrap()
    }

    async fn run(&self, statement: &str) -> StatementOutcome {
        self.sql()
            .execute(statement, &ExecuteOptions::default())
            .await
            .unwrap()
    }

    fn table(&self, name: &str) -> TableRef {
        TableRef::new(Some(self.database.clone()), None, name)
    }

    async fn create_items(&self) -> String {
        let name = unique_name("items");
        self.run(&format!(
            "CREATE TABLE `{name}` (id INT PRIMARY KEY, label VARCHAR(40) NOT NULL)"
        ))
        .await;
        self.run(&format!(
            "INSERT INTO `{name}` VALUES (1, 'a'), (2, 'b'), (3, 'c'), (4, 'd'), (5, 'e')"
        ))
        .await;
        name
    }

    async fn rows(&self, table: &str) -> Vec<Vec<Value>> {
        let query = TableQuery {
            tiebreaker: vec!["id".into()],
            ..TableQuery::default()
        };
        self.sql()
            .fetch_table_page(&self.table(table), &query)
            .await
            .unwrap()
            .rows
    }
}

fn driver_for(engine: TestEngine) -> Box<dyn DatabaseDriver> {
    match engine {
        TestEngine::MariaDb => Box::new(MariaDbDriver::new()),
        _ => Box::new(MySqlDriver::new()),
    }
}

fn product_for(engine: TestEngine) -> &'static str {
    match engine {
        TestEngine::MariaDb => "MariaDB",
        _ => "MySQL",
    }
}

fn rows_of(outcome: StatementOutcome) -> relix_core::RowBatch {
    match outcome {
        StatementOutcome::Rows(batch) => batch,
        other => panic!("expected rows, got {other:?}"),
    }
}

async fn connects_and_reports_server(engine: TestEngine) {
    let Some(fixture) = Fixture::open(engine).await else {
        return;
    };
    let info = fixture.connection.server_info();
    assert_eq!(info.product, product_for(engine));
    assert!(!info.version.is_empty());
    fixture.connection.ping().await.unwrap();
    fixture.connection.close().await.unwrap();
    let error = fixture
        .sql()
        .execute("SELECT 1", &ExecuteOptions::default())
        .await
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Connection);
}

async fn rejects_wrong_password(engine: TestEngine) {
    let Some(mut config) = integration_config(engine) else {
        return;
    };
    config.password = Some(Secret::new("definitely-wrong"));
    let error = match driver_for(engine).connect(&config).await {
        Ok(_) => panic!("connection with a wrong password succeeded"),
        Err(error) => error,
    };
    assert_eq!(error.kind, ErrorKind::Authentication);
    assert!(!error.to_string().contains("definitely-wrong"));
}

async fn fails_on_wrong_port(engine: TestEngine) {
    let Some(mut config) = integration_config(engine) else {
        return;
    };
    config.settings.port = Some(1);
    let error = match driver_for(engine).connect(&config).await {
        Ok(_) => panic!("connection to a closed port succeeded"),
        Err(error) => error,
    };
    assert_eq!(error.kind, ErrorKind::Connection);
}

async fn negotiates_tls(engine: TestEngine) {
    let Some(mut config) = integration_config(engine) else {
        return;
    };
    for mode in [TlsMode::Prefer, TlsMode::Require] {
        config.settings.tls.mode = mode;
        let connection = driver_for(engine).connect(&config).await.unwrap();
        let outcome = connection
            .as_sql()
            .unwrap()
            .execute(
                "SHOW SESSION STATUS LIKE 'Ssl_cipher'",
                &ExecuteOptions::default(),
            )
            .await
            .unwrap();
        let cipher = rows_of(outcome).rows[0][1].clone();
        assert!(
            cipher.as_text().is_some_and(|cipher| !cipher.is_empty()),
            "{mode:?} did not negotiate TLS"
        );
        connection.close().await.unwrap();
    }
    config.settings.tls.mode = TlsMode::VerifyFull;
    let error = match driver_for(engine).connect(&config).await {
        Ok(_) => panic!("self-signed test certificate passed full verification"),
        Err(error) => error,
    };
    assert_eq!(error.kind, ErrorKind::Tls);
}

async fn introspects_catalog(engine: TestEngine) {
    let Some(fixture) = Fixture::open(engine).await else {
        return;
    };
    let sql = fixture.sql();
    let parent = unique_name("parent");
    let child = unique_name("child");
    let view = unique_name("child_view");
    let procedure = unique_name("touch");
    let sequence = unique_name("seq");
    fixture
        .run(&format!(
            "CREATE TABLE `{parent}` (id INT PRIMARY KEY) COMMENT 'parents'"
        ))
        .await;
    fixture
        .run(&format!(
            "CREATE TABLE `{child}` (\
               id BIGINT UNSIGNED AUTO_INCREMENT PRIMARY KEY, \
               parent_id INT NOT NULL, \
               code VARCHAR(20) NOT NULL DEFAULT 'x' COMMENT 'the code', \
               note TEXT NULL, \
               UNIQUE KEY uq_code (code), \
               CONSTRAINT fk_parent FOREIGN KEY (parent_id) REFERENCES `{parent}` (id))"
        ))
        .await;
    fixture
        .run(&format!(
            "CREATE VIEW `{view}` AS SELECT id, code FROM `{child}`"
        ))
        .await;
    fixture
        .run(&format!(
            "CREATE PROCEDURE `{procedure}`() COMMENT 'touches' SELECT 1"
        ))
        .await;
    if engine == TestEngine::MariaDb {
        fixture.run(&format!("CREATE SEQUENCE `{sequence}`")).await;
    }

    let databases = sql.databases().await.unwrap();
    assert!(
        databases
            .iter()
            .any(|database| database.name == fixture.database && database.is_default)
    );
    assert!(sql.schemas(&fixture.database).await.unwrap().is_empty());

    let objects = sql.objects(&fixture.database, None).await.unwrap();
    let kind_of = |name: &str| {
        objects
            .iter()
            .find(|object| object.object.name == name)
            .map(|object| (object.kind, object.comment.clone()))
    };
    assert_eq!(
        kind_of(&parent),
        Some((ObjectKind::Table, Some("parents".into())))
    );
    assert_eq!(kind_of(&view), Some((ObjectKind::View, None)));
    assert_eq!(
        kind_of(&procedure),
        Some((ObjectKind::Procedure, Some("touches".into())))
    );
    if engine == TestEngine::MariaDb {
        assert_eq!(kind_of(&sequence), Some((ObjectKind::Sequence, None)));
    }

    let details = sql
        .table_details(&TableRef::new(None, None, child.clone()))
        .await
        .unwrap();
    assert_eq!(details.table, fixture.table(&child));
    assert_eq!(details.kind, ObjectKind::Table);
    assert_eq!(details.primary_key, vec!["id".to_owned()]);
    let names: Vec<&str> = details.columns.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["id", "parent_id", "code", "note"]);
    let id = &details.columns[0];
    assert!(id.is_primary_key && id.is_auto_increment && !id.nullable);
    assert!(id.data_type.to_ascii_lowercase().contains("unsigned"));
    let code = &details.columns[2];
    assert_eq!(code.comment.as_deref(), Some("the code"));
    assert!(
        code.default_value
            .as_deref()
            .is_some_and(|d| d.contains('x'))
    );
    assert_eq!(details.columns[3].default_value, None);
    assert!(details.columns[3].nullable);
    assert!(
        details
            .indexes
            .iter()
            .any(|index| index.name == "uq_code" && index.is_unique && !index.is_primary)
    );
    assert!(details.indexes[0].is_primary);
    let foreign_key = &details.foreign_keys[0];
    assert_eq!(foreign_key.name, "fk_parent");
    assert_eq!(foreign_key.columns, vec!["parent_id".to_owned()]);
    assert_eq!(foreign_key.referenced_table, fixture.table(&parent));
    assert_eq!(foreign_key.referenced_columns, vec!["id".to_owned()]);
    assert!(
        details
            .definition
            .as_deref()
            .is_some_and(|definition| definition.contains("CREATE TABLE"))
    );

    let view_details = sql.table_details(&fixture.table(&view)).await.unwrap();
    assert_eq!(view_details.kind, ObjectKind::View);
    assert!(
        view_details
            .definition
            .as_deref()
            .is_some_and(|definition| definition.contains("VIEW"))
    );
    let procedure_object = objects
        .iter()
        .find(|object| object.object.name == procedure)
        .unwrap();
    let procedure_definition = sql.object_definition(procedure_object).await.unwrap();
    assert!(procedure_definition.is_some_and(|definition| definition.contains("PROCEDURE")));

    if engine == TestEngine::MariaDb {
        fixture.run(&format!("DROP SEQUENCE `{sequence}`")).await;
    }
    fixture.run(&format!("DROP PROCEDURE `{procedure}`")).await;
    fixture.run(&format!("DROP VIEW `{view}`")).await;
    fixture.run(&format!("DROP TABLE `{child}`")).await;
    fixture.run(&format!("DROP TABLE `{parent}`")).await;
}

async fn executes_statements(engine: TestEngine) {
    let Some(fixture) = Fixture::open(engine).await else {
        return;
    };
    let batch = rows_of(
        fixture
            .run(
                "SELECT 42 AS i, CAST(7 AS UNSIGNED) AS u, 1.5e0 AS d, \
                 CAST(12.34 AS DECIMAL(6,2)) AS amount, 'héllo' AS s, X'00FF' AS b, \
                 NULL AS n, DATE '2026-10-23' AS dt",
            )
            .await,
    );
    assert_eq!(
        batch.rows,
        vec![vec![
            Value::Int(42),
            Value::UInt(7),
            Value::Float(1.5),
            Value::Text("12.34".into()),
            Value::Text("héllo".into()),
            Value::Bytes(vec![0x00, 0xff]),
            Value::Null,
            Value::Text("2026-10-23".into()),
        ]]
    );
    assert_eq!(batch.columns[5].type_name.as_deref(), Some("VARBINARY"));
    assert_eq!(batch.columns[3].type_name.as_deref(), Some("DECIMAL"));
    assert!(!batch.has_more);

    let table = unique_name("exec");
    assert!(matches!(
        fixture
            .run(&format!(
                "CREATE TABLE `{table}` (id INT PRIMARY KEY, v INT)"
            ))
            .await,
        StatementOutcome::Completed { .. }
    ));
    assert_eq!(
        fixture
            .run(&format!(
                "INSERT INTO `{table}` VALUES (1, 1), (2, 2), (3, 3)"
            ))
            .await,
        StatementOutcome::Affected { rows: 3 }
    );
    assert_eq!(
        fixture
            .run(&format!("UPDATE `{table}` SET v = 9 WHERE id >= 2"))
            .await,
        StatementOutcome::Affected { rows: 2 }
    );
    let empty = rows_of(
        fixture
            .run(&format!("SELECT * FROM `{table}` WHERE id > 100"))
            .await,
    );
    assert!(empty.rows.is_empty());
    assert_eq!(empty.columns.len(), 2);

    let limited = fixture
        .sql()
        .execute(
            &format!("SELECT id FROM `{table}` ORDER BY id"),
            &ExecuteOptions { row_limit: 2 },
        )
        .await
        .unwrap();
    let limited = rows_of(limited);
    assert_eq!(limited.rows, vec![vec![Value::Int(1)], vec![Value::Int(2)]]);
    assert!(limited.has_more);
    assert_eq!(
        rows_of(fixture.run("SELECT 1").await).rows,
        vec![vec![Value::Int(1)]]
    );

    let error = fixture
        .sql()
        .execute("SELECT * FROMM t", &ExecuteOptions::default())
        .await
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Query);
    assert_eq!(error.position, Some(9));
    assert!(error.detail.is_some());

    fixture.run(&format!("DROP TABLE `{table}`")).await;
}

async fn keeps_session_state(engine: TestEngine) {
    let Some(fixture) = Fixture::open(engine).await else {
        return;
    };
    let table = fixture.create_items().await;
    fixture.run("SET @relix_marker = 17").await;
    assert_eq!(
        rows_of(fixture.run("SELECT @relix_marker").await).rows,
        vec![vec![Value::Int(17)]]
    );
    fixture.run("BEGIN").await;
    fixture
        .run(&format!("DELETE FROM `{table}` WHERE id = 1"))
        .await;
    assert_eq!(fixture.rows(&table).await.len(), 5);
    fixture.run("ROLLBACK").await;
    assert_eq!(fixture.rows(&table).await.len(), 5);
    fixture.run(&format!("DROP TABLE `{table}`")).await;
}

async fn paginates_and_counts(engine: TestEngine) {
    let Some(fixture) = Fixture::open(engine).await else {
        return;
    };
    let sql = fixture.sql();
    let name = fixture.create_items().await;
    let table = TableRef::new(None, None, name.clone());
    let first = sql
        .fetch_table_page(
            &table,
            &TableQuery {
                tiebreaker: vec!["id".into()],
                limit: 2,
                ..TableQuery::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(first.rows.len(), 2);
    assert!(first.has_more);
    assert_eq!(first.columns[0].name, "id");
    let last = sql
        .fetch_table_page(
            &fixture.table(&name),
            &TableQuery {
                tiebreaker: vec!["id".into()],
                offset: 4,
                limit: 2,
                ..TableQuery::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(
        last.rows,
        vec![vec![Value::Int(5), Value::Text("e".into())]]
    );
    assert!(!last.has_more);
    assert_eq!(sql.count_rows(&table, None).await.unwrap(), 5);
    assert_eq!(sql.count_rows(&table, Some("id > 3")).await.unwrap(), 2);
    fixture.run(&format!("DROP TABLE `{name}`")).await;
}

async fn applies_changes_in_one_transaction(engine: TestEngine) {
    let Some(fixture) = Fixture::open(engine).await else {
        return;
    };
    let name = fixture.create_items().await;
    let changes = ChangeSet {
        table: fixture.table(&name),
        changes: vec![
            RowChange::Insert {
                values: vec![
                    ("id".into(), Value::Int(6)),
                    ("label".into(), Value::Text("f".into())),
                ],
            },
            RowChange::Update {
                key: vec![("id".into(), Value::Int(2))],
                cells: vec![CellChange {
                    column: "label".into(),
                    original: Value::Text("b".into()),
                    updated: Value::Text("b".into()),
                }],
            },
            RowChange::Delete {
                key: vec![("id".into(), Value::Int(1))],
            },
        ],
    };
    let report = fixture.sql().apply_changes(&changes).await.unwrap();
    assert_eq!((report.inserted, report.updated, report.deleted), (1, 1, 1));
    let rows = fixture.rows(&name).await;
    assert_eq!(rows.len(), 5);
    assert_eq!(rows[0][0], Value::Int(2));
    assert_eq!(rows[4], vec![Value::Int(6), Value::Text("f".into())]);
    fixture.run(&format!("DROP TABLE `{name}`")).await;
}

async fn rolls_back_conflicting_changes(engine: TestEngine) {
    let Some(fixture) = Fixture::open(engine).await else {
        return;
    };
    let name = fixture.create_items().await;
    let changes = ChangeSet {
        table: fixture.table(&name),
        changes: vec![
            RowChange::Delete {
                key: vec![("id".into(), Value::Int(1))],
            },
            RowChange::Update {
                key: vec![("id".into(), Value::Int(2))],
                cells: vec![CellChange {
                    column: "label".into(),
                    original: Value::Text("stale".into()),
                    updated: Value::Text("z".into()),
                }],
            },
        ],
    };
    let error = fixture.sql().apply_changes(&changes).await.unwrap_err();
    assert_eq!(error.kind, ErrorKind::ConcurrentModification);
    assert!(error.message.contains("#2"));
    let rows = fixture.rows(&name).await;
    assert_eq!(rows.len(), 5);
    assert_eq!(rows[1], vec![Value::Int(2), Value::Text("b".into())]);

    let missing = ChangeSet {
        table: fixture.table(&name),
        changes: vec![RowChange::Delete {
            key: vec![("id".into(), Value::Int(999))],
        }],
    };
    let error = fixture.sql().apply_changes(&missing).await.unwrap_err();
    assert_eq!(error.kind, ErrorKind::ConcurrentModification);
    fixture.run(&format!("DROP TABLE `{name}`")).await;
}

async fn cancels_running_statement(engine: TestEngine) {
    let Some(fixture) = Fixture::open(engine).await else {
        return;
    };
    let connection = Arc::clone(&fixture.connection);
    let started = Instant::now();
    let running = tokio::spawn(async move {
        connection
            .as_sql()
            .unwrap()
            .execute(
                "SELECT COUNT(*) FROM information_schema.COLUMNS a, \
                 information_schema.COLUMNS b, information_schema.COLUMNS c",
                &ExecuteOptions::default(),
            )
            .await
    });
    tokio::time::sleep(Duration::from_millis(500)).await;
    fixture.sql().cancel().await.unwrap();
    let result = running.await.unwrap();
    assert!(started.elapsed() < Duration::from_secs(10));
    assert_eq!(result.unwrap_err().kind, ErrorKind::Cancelled);
    assert_eq!(
        rows_of(fixture.run("SELECT 2").await).rows,
        vec![vec![Value::Int(2)]]
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn mismatched_driver_reports_actual_server() {
    for (engine, driver, expected) in [
        (
            TestEngine::MySql,
            Box::new(MariaDbDriver::new()) as Box<dyn DatabaseDriver>,
            "MySQL (connected via MariaDB driver)",
        ),
        (
            TestEngine::MariaDb,
            Box::new(MySqlDriver::new()),
            "MariaDB (connected via MySQL driver)",
        ),
    ] {
        let Some(config) = integration_config(engine) else {
            return;
        };
        let connection = driver.connect(&config).await.unwrap();
        assert_eq!(connection.server_info().product, expected);
        connection.close().await.unwrap();
    }
}
