/*
 * @author t0bx
 * @since 23.10.2026
 */

#![allow(clippy::unwrap_used)]

mod common;

mod tidb {
    use super::common::Engine;
    use relix_core::{StatementOutcome, TableRef};
    use relix_driver_api::{DatabaseDriver, ExecuteOptions};
    use relix_driver_mysql::MySqlDriver;
    use relix_testing::integration::unique_name;

    #[tokio::test(flavor = "multi_thread")]
    async fn reports_release_and_actual_server() {
        let Some(config) = Engine::TiDb.config() else {
            return;
        };
        let connection = Engine::TiDb.driver().connect(&config).await.unwrap();
        let info = connection.server_info();
        assert_eq!(info.product, "TiDB");
        assert!(info.version.starts_with('v'), "{}", info.version);
        assert!(info.version.contains("(MySQL "), "{}", info.version);
        connection.close().await.unwrap();
        let via_mysql = MySqlDriver::new().connect(&config).await.unwrap();
        assert_eq!(
            via_mysql.server_info().product,
            "TiDB (connected via MySQL driver)"
        );
        via_mysql.close().await.unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn auto_random_keys_are_generated_columns() {
        let Some(config) = Engine::TiDb.config() else {
            return;
        };
        let connection = Engine::TiDb.driver().connect(&config).await.unwrap();
        let sql = connection.as_sql().unwrap();
        let name = unique_name("random_key");
        let created = sql
            .execute(
                &format!("CREATE TABLE `{name}` (id BIGINT AUTO_RANDOM PRIMARY KEY, v INT)"),
                &ExecuteOptions::default(),
            )
            .await
            .unwrap();
        assert!(matches!(created, StatementOutcome::Completed { .. }));
        let details = sql
            .table_details(&TableRef::new(
                config.settings.database.clone(),
                None,
                name.clone(),
            ))
            .await
            .unwrap();
        assert_eq!(details.primary_key, vec!["id".to_owned()]);
        assert!(details.columns[0].is_auto_increment);
        assert!(!details.columns[1].is_auto_increment);
        sql.execute(&format!("DROP TABLE `{name}`"), &ExecuteOptions::default())
            .await
            .unwrap();
        connection.close().await.unwrap();
    }
}
