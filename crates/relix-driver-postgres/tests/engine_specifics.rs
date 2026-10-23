/*
 * @author t0bx
 * @since 23.10.2026
 */

#![allow(clippy::unwrap_used)]

mod common;

mod cockroachdb {
    use super::common::{self, Engine};

    #[tokio::test(flavor = "multi_thread")]
    async fn reports_release_version() {
        let Some(config) = Engine::CockroachDb.config() else {
            return;
        };
        let connection = common::connect(Engine::CockroachDb, &config).await;
        let version = connection.server_info().version;
        assert!(version.starts_with('v'), "{version}");
        assert!(!version.contains("CockroachDB"), "{version}");
        connection.close().await.unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn implicit_rowid_stays_hidden() {
        let engine = Engine::CockroachDb;
        let Some(config) = engine.config() else {
            return;
        };
        let connection = common::connect(engine, &config).await;
        let sql = common::sql(connection.as_ref());
        let name = common::table_name("no_key");
        common::run(
            connection.as_ref(),
            &format!("CREATE TABLE {name} (label text)"),
        )
        .await;
        let details = sql
            .table_details(&common::table_ref(&config, &name))
            .await
            .unwrap();
        let columns: Vec<_> = details.columns.iter().map(|column| &column.name).collect();
        assert_eq!(columns, ["label"]);
        assert!(details.primary_key.is_empty());
        let ddl = details.definition.unwrap();
        assert!(ddl.contains("rowid INT8 NOT VISIBLE"), "{ddl}");
        common::run(connection.as_ref(), &format!("DROP TABLE {name}")).await;
        connection.close().await.unwrap();
    }
}

mod timescaledb {
    use super::common::{self, Engine};
    use relix_core::{ObjectInfo, ObjectKind, TableRef};
    use relix_driver_api::DatabaseDriver;
    use relix_driver_postgres::PostgresDriver;

    #[tokio::test(flavor = "multi_thread")]
    async fn reports_timescale_and_postgres_versions() {
        let Some(config) = Engine::TimescaleDb.config() else {
            return;
        };
        let connection = common::connect(Engine::TimescaleDb, &config).await;
        let info = connection.server_info();
        assert_eq!(info.product, "TimescaleDB");
        assert!(info.version.contains("(PostgreSQL "), "{}", info.version);
        connection.close().await.unwrap();
        let via_postgres = PostgresDriver::new().connect(&config).await.unwrap();
        assert_eq!(
            via_postgres.server_info().product,
            "TimescaleDB (connected via PostgreSQL driver)"
        );
        via_postgres.close().await.unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn marks_hypertables_and_continuous_aggregates() {
        let engine = Engine::TimescaleDb;
        let Some(config) = engine.config() else {
            return;
        };
        let connection = common::connect(engine, &config).await;
        let sql = common::sql(connection.as_ref());
        let schema = common::table_name("ts");
        let setup = [
            format!("CREATE SCHEMA {schema}"),
            format!(
                "CREATE TABLE {schema}.metrics (time timestamptz NOT NULL, device int, value float8)"
            ),
            format!("SELECT create_hypertable('{schema}.metrics', by_range('time'))"),
            format!(
                "CREATE MATERIALIZED VIEW {schema}.hourly WITH (timescaledb.continuous) AS \
                 SELECT time_bucket('1 hour', time) AS bucket, device, avg(value) AS average \
                 FROM {schema}.metrics GROUP BY bucket, device WITH NO DATA"
            ),
            format!("CREATE TABLE {schema}.plain (id int)"),
        ];
        for statement in &setup {
            common::run(connection.as_ref(), statement).await;
        }
        let database = config.settings.database.clone().unwrap();
        let objects = sql.objects(&database, Some(&schema)).await.unwrap();
        let find = |name: &str| {
            objects
                .iter()
                .find(|object| object.object.name == name)
                .unwrap()
        };
        assert_eq!(find("metrics").comment.as_deref(), Some("hypertable"));
        assert_eq!(find("hourly").kind, ObjectKind::MaterializedView);
        assert_eq!(
            find("hourly").comment.as_deref(),
            Some("continuous aggregate")
        );
        assert_eq!(find("plain").comment, None);

        let metrics = TableRef::new(Some(database.clone()), Some(schema.clone()), "metrics");
        let ddl = sql
            .table_details(&metrics)
            .await
            .unwrap()
            .definition
            .unwrap();
        assert!(
            ddl.contains(&format!(
                "SELECT create_hypertable('\"{schema}\".\"metrics\"', by_range('time', INTERVAL '7 days'));"
            )),
            "{ddl}"
        );
        let hourly = ObjectInfo {
            object: TableRef::new(Some(database.clone()), Some(schema.clone()), "hourly"),
            kind: ObjectKind::MaterializedView,
            comment: None,
        };
        let aggregate = sql.object_definition(&hourly).await.unwrap().unwrap();
        assert!(
            aggregate.starts_with(&format!(
                "CREATE MATERIALIZED VIEW \"{schema}\".\"hourly\"\nWITH (timescaledb.continuous) AS"
            )),
            "{aggregate}"
        );
        assert!(aggregate.contains("time_bucket"), "{aggregate}");
        let details = sql.table_details(&hourly.object).await.unwrap();
        assert_eq!(details.kind, ObjectKind::MaterializedView);

        common::run(
            connection.as_ref(),
            &format!("DROP SCHEMA {schema} CASCADE"),
        )
        .await;
        connection.close().await.unwrap();
    }
}

mod yugabytedb {
    use super::common::{self, Engine};

    #[tokio::test(flavor = "multi_thread")]
    async fn reports_release_and_postgres_level() {
        let Some(config) = Engine::YugabyteDb.config() else {
            return;
        };
        let connection = common::connect(Engine::YugabyteDb, &config).await;
        let version = connection.server_info().version;
        assert!(version.contains(" (PostgreSQL "), "{version}");
        assert!(!version.contains("-YB-"), "{version}");
        connection.close().await.unwrap();
    }
}
