/*
 * @author t0bx
 * @since 23.10.2026
 */

#![allow(clippy::unwrap_used)]

use relix_core::{
    ChangeSet, ConnectionConfig, ConnectionSettings, Dialect, DriverId, RowChange, SortDirection,
    SortKey, StatementOutcome, TableQuery, TableRef, Value,
};
use relix_driver_api::{
    ConnectionTarget, DatabaseConnection, DatabaseDriver, DriverMetadata, ErrorKind, ExecuteOptions,
};
use relix_driver_manager::ExternalDriver;
use std::path::Path;
use std::time::Duration;

const CONNECTOR: &str = env!("CARGO_BIN_EXE_relix-csv-connector");

fn csv_directory() -> tempfile::TempDir {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join("people.csv"),
        "id,name,age\n1,Ada,36\n2,Linus,54\n3,Grace,85\n4,Alan,\n",
    )
    .unwrap();
    std::fs::write(
        directory.path().join("cities.csv"),
        "city,population\nBerlin,3.7\nHamburg,1.9\n",
    )
    .unwrap();
    std::fs::write(directory.path().join("notes.txt"), "ignored").unwrap();
    directory
}

fn config_for(directory: &Path) -> ConnectionConfig {
    let settings = ConnectionSettings {
        file_path: Some(directory.to_path_buf()),
        ..ConnectionSettings::default()
    };
    ConnectionConfig::new(settings, None)
}

async fn connect(directory: &Path) -> Box<dyn DatabaseConnection> {
    let driver = ExternalDriver::probe(CONNECTOR).await.unwrap();
    driver.connect(&config_for(directory)).await.unwrap()
}

#[tokio::test]
async fn probe_reports_connector_metadata() {
    let driver = ExternalDriver::probe(CONNECTOR).await.unwrap();
    let metadata = driver.metadata();
    assert_eq!(metadata.id, DriverId::new("csv"));
    assert_eq!(metadata.display_name, "CSV Files");
    assert_eq!(metadata.dialect, Some(Dialect::Sqlite));
    assert_eq!(metadata.target, ConnectionTarget::File);
    assert!(driver.capabilities().is_relational());
    assert!(!driver.capabilities().table_editing);
}

#[tokio::test]
async fn browses_pages_and_queries_csv_tables() {
    let directory = csv_directory();
    let connection = connect(directory.path()).await;
    assert_eq!(connection.server_info().product, "CSV Files");
    connection.ping().await.unwrap();
    let sql = connection.as_sql().unwrap();

    let databases = sql.databases().await.unwrap();
    let database = databases
        .iter()
        .find(|database| database.is_default)
        .unwrap_or(&databases[0])
        .name
        .clone();
    let mut tables: Vec<String> = sql
        .objects(&database, None)
        .await
        .unwrap()
        .into_iter()
        .map(|object| object.object.name)
        .collect();
    tables.sort();
    assert_eq!(tables, ["cities", "people"]);

    let people = TableRef::new(Some(database.clone()), None, "people");
    let details = sql.table_details(&people).await.unwrap();
    let types: Vec<&str> = details
        .columns
        .iter()
        .map(|column| column.data_type.as_str())
        .collect();
    assert_eq!(types, ["INTEGER", "TEXT", "INTEGER"]);

    let query = TableQuery {
        filter: Some("age > 40".into()),
        sort: vec![SortKey {
            column: "name".into(),
            direction: SortDirection::Descending,
        }],
        tiebreaker: vec!["id".into()],
        offset: 0,
        limit: 1,
    };
    let page = sql.fetch_table_page(&people, &query).await.unwrap();
    assert_eq!(page.rows.len(), 1);
    assert!(page.has_more);
    let name = page.column_index("name").unwrap();
    assert_eq!(page.rows[0][name], Value::Text("Linus".into()));
    assert_eq!(sql.count_rows(&people, Some("age > 40")).await.unwrap(), 2);
    assert_eq!(sql.count_rows(&people, None).await.unwrap(), 4);

    let outcome = sql
        .execute(
            "SELECT name, age FROM people WHERE age IS NULL",
            &ExecuteOptions::default(),
        )
        .await
        .unwrap();
    let StatementOutcome::Rows(rows) = outcome else {
        panic!("expected rows, got {outcome:?}");
    };
    assert_eq!(
        rows.rows,
        vec![vec![Value::Text("Alan".into()), Value::Null]]
    );

    connection.close().await.unwrap();
    let error = connection.ping().await.unwrap_err();
    assert_eq!(error.kind, ErrorKind::Connection);
}

#[tokio::test]
async fn data_is_read_only() {
    let directory = csv_directory();
    let connection = connect(directory.path()).await;
    let sql = connection.as_sql().unwrap();
    let error = sql
        .execute("DELETE FROM people", &ExecuteOptions::default())
        .await
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Unsupported);
    let error = sql
        .execute(
            "WITH gone AS (SELECT 1) DELETE FROM people",
            &ExecuteOptions::default(),
        )
        .await
        .unwrap_err();
    assert_ne!(error.kind, ErrorKind::Connection);
    let changes = ChangeSet {
        table: TableRef::new(None, None, "people"),
        changes: vec![RowChange::Delete {
            key: vec![("id".into(), Value::Int(1))],
        }],
    };
    let error = sql.apply_changes(&changes).await.unwrap_err();
    assert_eq!(error.kind, ErrorKind::Unsupported);
    let people = TableRef::new(None, None, "people");
    assert_eq!(sql.count_rows(&people, None).await.unwrap(), 4);
    connection.close().await.unwrap();
}

#[tokio::test]
async fn cancel_interrupts_a_running_statement() {
    let directory = csv_directory();
    let connection = connect(directory.path()).await;
    let sql = connection.as_sql().unwrap();
    let endless = "WITH RECURSIVE counter(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM counter) \
                   SELECT count(*) FROM counter";
    let options = ExecuteOptions::default();
    let execution = sql.execute(endless, &options);
    tokio::pin!(execution);
    let result = tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            tokio::select! {
                result = &mut execution => break result,
                () = tokio::time::sleep(Duration::from_millis(200)) => sql.cancel().await.unwrap(),
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(result.unwrap_err().kind, ErrorKind::Cancelled);
    sql.execute("SELECT 1", &options).await.unwrap();
    connection.close().await.unwrap();
}

#[tokio::test]
async fn connector_errors_and_identity_mismatches_are_reported() {
    let driver = ExternalDriver::probe(CONNECTOR).await.unwrap();
    let missing = tempfile::tempdir().unwrap().path().join("absent");
    let error = driver.connect(&config_for(&missing)).await.err().unwrap();
    assert_eq!(error.kind, ErrorKind::Configuration);

    let impostor = ExternalDriver::new(
        CONNECTOR,
        DriverMetadata::built_in("postgres", "PostgreSQL", None, ConnectionTarget::File),
        driver.capabilities(),
    );
    let directory = csv_directory();
    let error = impostor
        .connect(&config_for(directory.path()))
        .await
        .err()
        .unwrap();
    assert_eq!(error.kind, ErrorKind::Protocol);

    let absent = ExternalDriver::new(missing, driver.metadata(), driver.capabilities());
    let error = absent
        .connect(&config_for(directory.path()))
        .await
        .err()
        .unwrap();
    assert_eq!(error.kind, ErrorKind::Connection);
}
