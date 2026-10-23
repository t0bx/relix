/*
 * @author t0bx
 * @since 23.10.2026
 */

#![allow(clippy::unwrap_used)]

use relix_core::{
    CellChange, ChangeSet, ConnectionConfig, ConnectionSettings, ObjectKind, RowChange,
    SortDirection, SortKey, StatementOutcome, TableQuery, TableRef, Value,
};
use relix_driver_api::{
    DatabaseConnection, DatabaseDriver, DriverError, ErrorKind, ExecuteOptions, SqlConnection,
};
use relix_driver_sqlite::SqliteDriver;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tempfile::TempDir;

const SCHEMA: [&str; 6] = [
    "CREATE TABLE authors (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL UNIQUE)",
    "CREATE TABLE books (id INTEGER PRIMARY KEY, author_id INTEGER NOT NULL REFERENCES authors, \
     title VARCHAR(200) NOT NULL, price REAL DEFAULT 9.5, cover BLOB)",
    "CREATE INDEX books_title_idx ON books (title, price)",
    "CREATE VIEW cheap_books AS SELECT title FROM books WHERE price < 10",
    "CREATE TRIGGER books_touch AFTER UPDATE ON books BEGIN SELECT 1; END",
    "CREATE TABLE tags (book_id INTEGER, label TEXT, PRIMARY KEY (label, book_id))",
];

fn config_for(path: &Path, create_if_missing: bool) -> ConnectionConfig {
    ConnectionConfig::new(
        ConnectionSettings {
            file_path: Some(path.to_path_buf()),
            create_if_missing,
            ..ConnectionSettings::default()
        },
        None,
    )
}

async fn connect(
    path: &Path,
    create_if_missing: bool,
) -> Result<Box<dyn DatabaseConnection>, DriverError> {
    SqliteDriver::new()
        .connect(&config_for(path, create_if_missing))
        .await
}

struct Fixture {
    directory: TempDir,
    path: PathBuf,
    connection: Box<dyn DatabaseConnection>,
}

impl Fixture {
    async fn new() -> Self {
        let directory = TempDir::new().unwrap();
        let path = directory.path().join("library.db");
        let connection = connect(&path, true).await.unwrap();
        let fixture = Self {
            directory,
            path,
            connection,
        };
        for statement in SCHEMA {
            fixture.run(statement).await;
        }
        fixture
            .run("INSERT INTO authors (name) VALUES ('Ada'), ('Grace')")
            .await;
        for index in 1..=25 {
            fixture
                .run(&format!(
                    "INSERT INTO books (author_id, title, price) VALUES ({}, 'Book {index:02}', {index})",
                    index % 2 + 1
                ))
                .await;
        }
        fixture
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

    async fn scalar(&self, statement: &str) -> Value {
        match self.run(statement).await {
            StatementOutcome::Rows(batch) => batch.rows[0][0].clone(),
            other => panic!("expected rows, got {other:?}"),
        }
    }
}

fn books() -> TableRef {
    TableRef::new(Some("main".into()), None, "books")
}

#[tokio::test]
async fn creates_missing_files_only_when_requested() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("absent.db");
    let error = connect(&path, false).await.err().unwrap();
    assert_eq!(error.kind, ErrorKind::Configuration);
    assert!(error.message.contains("does not exist"));
    assert!(!path.exists());

    let connection = connect(&path, true).await.unwrap();
    assert!(path.exists());
    assert_eq!(connection.server_info().product, "SQLite");
    assert!(!connection.server_info().version.is_empty());
    connection.ping().await.unwrap();
    connection.close().await.unwrap();
    assert_eq!(
        connection.ping().await.unwrap_err().kind,
        ErrorKind::Connection
    );

    let reopened = connect(&path, false).await.unwrap();
    reopened.ping().await.unwrap();
}

#[tokio::test]
async fn rejects_missing_path_and_non_database_files() {
    let missing = SqliteDriver::new()
        .connect(&ConnectionConfig::new(ConnectionSettings::default(), None))
        .await
        .err()
        .unwrap();
    assert_eq!(missing.kind, ErrorKind::Configuration);

    let directory = TempDir::new().unwrap();
    let path = directory.path().join("garbage.db");
    std::fs::write(&path, vec![b'x'; 4096]).unwrap();
    let error = connect(&path, false).await.err().unwrap();
    assert_eq!(error.kind, ErrorKind::Connection);
}

#[tokio::test]
async fn opens_in_memory_databases() {
    let connection = connect(Path::new(":memory:"), false).await.unwrap();
    let sql = connection.as_sql().unwrap();
    sql.execute("CREATE TABLE t (a)", &ExecuteOptions::default())
        .await
        .unwrap();
    let objects = sql.objects("main", None).await.unwrap();
    assert_eq!(objects.len(), 1);
    let foreign_keys = sql
        .execute("PRAGMA foreign_keys", &ExecuteOptions::default())
        .await
        .unwrap();
    let StatementOutcome::Rows(batch) = foreign_keys else {
        panic!("expected rows");
    };
    assert_eq!(batch.rows[0][0], Value::Int(1));
}

#[tokio::test]
async fn lists_databases_and_objects() {
    let fixture = Fixture::new().await;
    let sql = fixture.sql();
    let attached = fixture.directory.path().join("archive.db");
    fixture
        .run(&format!(
            "ATTACH DATABASE '{}' AS archive",
            attached.display()
        ))
        .await;
    fixture
        .run("CREATE TABLE archive.old_books (id INTEGER PRIMARY KEY)")
        .await;
    fixture.run("CREATE TEMP TABLE scratch (a)").await;

    let databases = sql.databases().await.unwrap();
    let names: Vec<_> = databases
        .iter()
        .map(|database| database.name.as_str())
        .collect();
    assert_eq!(names, vec!["main", "archive"]);
    assert!(databases[0].is_default);
    assert!(!databases[1].is_default);
    assert!(sql.schemas("main").await.unwrap().is_empty());

    let objects = sql.objects("main", None).await.unwrap();
    let summary: Vec<_> = objects
        .iter()
        .map(|object| (object.kind, object.object.name.as_str()))
        .collect();
    assert_eq!(
        summary,
        vec![
            (ObjectKind::Table, "authors"),
            (ObjectKind::Table, "books"),
            (ObjectKind::Table, "tags"),
            (ObjectKind::View, "cheap_books"),
            (ObjectKind::Trigger, "books_touch"),
        ]
    );
    assert_eq!(objects[0].object.database.as_deref(), Some("main"));
    assert_eq!(objects[0].object.schema, None);

    let archived = sql.objects("archive", None).await.unwrap();
    assert_eq!(archived.len(), 1);
    let details = sql.table_details(&archived[0].object).await.unwrap();
    assert_eq!(details.primary_key, vec!["id"]);

    let trigger = sql.object_definition(&objects[4]).await.unwrap().unwrap();
    assert!(trigger.starts_with("CREATE TRIGGER books_touch"));
}

#[tokio::test]
async fn describes_columns_keys_indexes_and_foreign_keys() {
    let fixture = Fixture::new().await;
    let sql = fixture.sql();
    let details = sql.table_details(&books()).await.unwrap();

    assert_eq!(details.kind, ObjectKind::Table);
    assert_eq!(details.primary_key, vec!["id"]);
    let id = &details.columns[0];
    assert!(id.is_primary_key && id.is_auto_increment && !id.nullable);
    let title = &details.columns[2];
    assert_eq!(title.data_type, "VARCHAR(200)");
    assert!(!title.nullable && !title.is_auto_increment);
    assert_eq!(details.columns[3].default_value.as_deref(), Some("9.5"));
    assert_eq!(details.columns[4].ordinal, 5);

    let index = details
        .indexes
        .iter()
        .find(|index| index.name == "books_title_idx")
        .unwrap();
    assert_eq!(index.columns, vec!["title", "price"]);
    assert!(!index.is_unique && !index.is_primary);

    assert_eq!(details.foreign_keys.len(), 1);
    let foreign_key = &details.foreign_keys[0];
    assert_eq!(foreign_key.columns, vec!["author_id"]);
    assert_eq!(foreign_key.referenced_table.name, "authors");
    assert_eq!(foreign_key.referenced_columns, vec!["id"]);
    assert!(
        details
            .definition
            .unwrap()
            .starts_with("CREATE TABLE books")
    );

    let authors = sql
        .table_details(&TableRef::new(None, None, "authors"))
        .await
        .unwrap();
    assert!(authors.columns[0].is_auto_increment);
    let unique = authors
        .indexes
        .iter()
        .find(|index| index.is_unique)
        .unwrap();
    assert_eq!(unique.columns, vec!["name"]);

    let tags = sql
        .table_details(&TableRef::new(None, None, "tags"))
        .await
        .unwrap();
    assert_eq!(tags.primary_key, vec!["label", "book_id"]);
    assert!(tags.columns.iter().all(|column| !column.is_auto_increment));
    assert!(tags.indexes.iter().any(|index| index.is_primary));

    let view = sql
        .table_details(&TableRef::new(None, None, "cheap_books"))
        .await
        .unwrap();
    assert_eq!(view.kind, ObjectKind::View);
    assert_eq!(view.columns.len(), 1);

    let missing = sql
        .table_details(&TableRef::new(None, None, "nope"))
        .await
        .unwrap_err();
    assert_eq!(missing.kind, ErrorKind::Query);
}

#[tokio::test]
async fn executes_queries_changes_and_ddl() {
    let fixture = Fixture::new().await;
    let sql = fixture.sql();

    let StatementOutcome::Rows(batch) = fixture
        .run("SELECT id, title, price, cover, 1 + 1 AS two FROM books WHERE id = 1")
        .await
    else {
        panic!("expected rows");
    };
    assert_eq!(batch.columns[0].type_name.as_deref(), Some("INTEGER"));
    assert_eq!(batch.columns[1].type_name.as_deref(), Some("VARCHAR(200)"));
    assert_eq!(batch.columns[4].type_name, None);
    assert_eq!(batch.columns[4].name, "two");
    assert_eq!(
        batch.rows[0],
        vec![
            Value::Int(1),
            Value::Text("Book 01".into()),
            Value::Float(1.0),
            Value::Null,
            Value::Int(2),
        ]
    );
    assert!(!batch.has_more);

    assert_eq!(
        fixture
            .run("UPDATE books SET cover = x'00ff' WHERE id <= 3")
            .await,
        StatementOutcome::Affected { rows: 3 }
    );
    assert_eq!(
        fixture.scalar("SELECT cover FROM books WHERE id = 2").await,
        Value::Bytes(vec![0, 255])
    );
    assert_eq!(
        fixture.run("CREATE TABLE extra (a INTEGER)").await,
        StatementOutcome::Completed {
            message: "CREATE TABLE".into()
        }
    );
    assert_eq!(
        fixture.run("INSERT INTO extra VALUES (1), (2)").await,
        StatementOutcome::Affected { rows: 2 }
    );

    fixture.run("BEGIN").await;
    fixture.run("DELETE FROM extra").await;
    fixture.run("ROLLBACK").await;
    assert_eq!(
        fixture.scalar("SELECT count(*) FROM extra").await,
        Value::Int(2)
    );

    let syntax = sql
        .execute(
            "SELECT * FROM books WHER id = 1",
            &ExecuteOptions::default(),
        )
        .await
        .unwrap_err();
    assert_eq!(syntax.kind, ErrorKind::Query);
    assert_eq!(syntax.position, Some(25));

    let multiple = sql
        .execute("SELECT 1; SELECT 2", &ExecuteOptions::default())
        .await
        .unwrap_err();
    assert_eq!(multiple.kind, ErrorKind::Query);
    let empty = sql
        .execute("  -- nothing", &ExecuteOptions::default())
        .await
        .unwrap_err();
    assert_eq!(empty.kind, ErrorKind::Query);
    let constraint = sql
        .execute(
            "INSERT INTO books (author_id, title) VALUES (99, 'orphan')",
            &ExecuteOptions::default(),
        )
        .await
        .unwrap_err();
    assert_eq!(constraint.kind, ErrorKind::Query);
}

#[tokio::test]
async fn truncates_results_at_the_row_limit() {
    let fixture = Fixture::new().await;
    let outcome = fixture
        .sql()
        .execute(
            "SELECT id FROM books ORDER BY id",
            &ExecuteOptions { row_limit: 10 },
        )
        .await
        .unwrap();
    let StatementOutcome::Rows(batch) = outcome else {
        panic!("expected rows");
    };
    assert_eq!(batch.rows.len(), 10);
    assert!(batch.has_more);
    assert_eq!(batch.rows[9][0], Value::Int(10));

    let StatementOutcome::Rows(exact) = fixture
        .sql()
        .execute("SELECT id FROM books", &ExecuteOptions { row_limit: 25 })
        .await
        .unwrap()
    else {
        panic!("expected rows");
    };
    assert_eq!(exact.rows.len(), 25);
    assert!(!exact.has_more);
}

#[tokio::test]
async fn pages_through_filtered_and_sorted_tables() {
    let fixture = Fixture::new().await;
    let sql = fixture.sql();
    let mut query = TableQuery {
        filter: Some("author_id = 1".into()),
        sort: vec![SortKey {
            column: "price".into(),
            direction: SortDirection::Descending,
        }],
        tiebreaker: vec!["id".into()],
        offset: 0,
        limit: 5,
    };
    let first = sql.fetch_table_page(&books(), &query).await.unwrap();
    assert_eq!(first.rows.len(), 5);
    assert!(first.has_more);
    let price = first.column_index("price").unwrap();
    assert_eq!(first.rows[0][price], Value::Float(24.0));
    assert_eq!(first.columns[3].type_name.as_deref(), Some("REAL"));

    query.offset = 10;
    let last = sql.fetch_table_page(&books(), &query).await.unwrap();
    assert_eq!(last.rows.len(), 2);
    assert!(!last.has_more);

    assert_eq!(
        sql.count_rows(&books(), Some("author_id = 1"))
            .await
            .unwrap(),
        12
    );
    assert_eq!(sql.count_rows(&books(), None).await.unwrap(), 25);
    let unqualified = TableRef::new(None, None, "books");
    assert_eq!(sql.count_rows(&unqualified, Some(" ")).await.unwrap(), 25);
}

#[tokio::test]
async fn applies_inserts_updates_and_deletes_in_one_transaction() {
    let fixture = Fixture::new().await;
    let sql = fixture.sql();
    let report = sql
        .apply_changes(&ChangeSet {
            table: books(),
            changes: vec![
                RowChange::Insert {
                    values: vec![
                        ("author_id".into(), Value::Int(1)),
                        ("title".into(), Value::Text("Fresh".into())),
                        ("cover".into(), Value::Bytes(vec![1, 2, 3])),
                    ],
                },
                RowChange::Update {
                    key: vec![("id".into(), Value::Int(1))],
                    cells: vec![CellChange {
                        column: "title".into(),
                        original: Value::Text("Book 01".into()),
                        updated: Value::Text("Renamed".into()),
                    }],
                },
                RowChange::Delete {
                    key: vec![("id".into(), Value::Text("2".into()))],
                },
            ],
        })
        .await
        .unwrap();
    assert_eq!((report.inserted, report.updated, report.deleted), (1, 1, 1));
    assert_eq!(
        fixture.scalar("SELECT title FROM books WHERE id = 1").await,
        Value::Text("Renamed".into())
    );
    assert_eq!(
        fixture.scalar("SELECT count(*) FROM books").await,
        Value::Int(25)
    );
    assert_eq!(
        fixture
            .scalar("SELECT cover FROM books WHERE title = 'Fresh'")
            .await,
        Value::Bytes(vec![1, 2, 3])
    );
}

#[tokio::test]
async fn rolls_back_when_a_change_conflicts() {
    let fixture = Fixture::new().await;
    let sql = fixture.sql();
    let error = sql
        .apply_changes(&ChangeSet {
            table: books(),
            changes: vec![
                RowChange::Delete {
                    key: vec![("id".into(), Value::Int(3))],
                },
                RowChange::Update {
                    key: vec![("id".into(), Value::Int(4))],
                    cells: vec![CellChange {
                        column: "title".into(),
                        original: Value::Text("stale title".into()),
                        updated: Value::Text("lost".into()),
                    }],
                },
            ],
        })
        .await
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::ConcurrentModification);
    assert!(error.message.contains("change #2"));
    assert_eq!(
        fixture.scalar("SELECT count(*) FROM books").await,
        Value::Int(25)
    );
    assert_eq!(
        fixture.scalar("SELECT title FROM books WHERE id = 4").await,
        Value::Text("Book 04".into())
    );

    let ambiguous = sql
        .apply_changes(&ChangeSet {
            table: books(),
            changes: vec![
                RowChange::Insert {
                    values: vec![
                        ("author_id".into(), Value::Int(1)),
                        ("title".into(), Value::Text("never".into())),
                    ],
                },
                RowChange::Delete { key: vec![] },
            ],
        })
        .await
        .unwrap_err();
    assert_eq!(ambiguous.kind, ErrorKind::AmbiguousRow);
    assert_eq!(
        fixture.scalar("SELECT count(*) FROM books").await,
        Value::Int(25)
    );

    fixture.run("BEGIN").await;
    let busy = sql
        .apply_changes(&ChangeSet {
            table: books(),
            changes: vec![RowChange::Delete {
                key: vec![("id".into(), Value::Int(5))],
            }],
        })
        .await
        .unwrap_err();
    assert_eq!(busy.kind, ErrorKind::Query);
    fixture.run("COMMIT").await;
}

#[tokio::test]
async fn cancels_a_long_running_query() {
    let fixture = Arc::new(Fixture::new().await);
    let running = Arc::clone(&fixture);
    let query = tokio::spawn(async move {
        running
            .sql()
            .execute(
                "WITH RECURSIVE counter(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM counter) \
                 SELECT count(*) FROM counter",
                &ExecuteOptions::default(),
            )
            .await
    });
    tokio::time::sleep(Duration::from_millis(300)).await;
    fixture.sql().cancel().await.unwrap();
    let error = tokio::time::timeout(Duration::from_secs(10), query)
        .await
        .unwrap()
        .unwrap()
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Cancelled);
    assert_eq!(fixture.scalar("SELECT 42").await, Value::Int(42));
    assert!(fixture.path.exists());
}
