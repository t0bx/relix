/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::catalog::{list_databases, list_objects};
use crate::connection::MySqlConnection;
use crate::definitions::show_create;
use crate::table_structure::{columns, foreign_keys, indexes, primary_key, relation_kind_of};
use async_trait::async_trait;
use relix_core::{DatabaseInfo, ObjectInfo, TableDetails, TableRef};
use relix_driver_api::{DriverError, ErrorKind, SqlIntrospector};

#[async_trait]
impl SqlIntrospector for MySqlConnection {
    async fn databases(&self) -> Result<Vec<DatabaseInfo>, DriverError> {
        let mut conn = self.pooled().await?;
        let names = list_databases(&mut conn).await?;
        Ok(names
            .into_iter()
            .map(|name| DatabaseInfo {
                is_default: self.default_database.as_deref() == Some(name.as_str()),
                name,
            })
            .collect())
    }

    async fn schemas(&self, _database: &str) -> Result<Vec<String>, DriverError> {
        Ok(Vec::new())
    }

    async fn objects(
        &self,
        database: &str,
        _schema: Option<&str>,
    ) -> Result<Vec<ObjectInfo>, DriverError> {
        let database = self.resolve_database(Some(database))?;
        let mut conn = self.pooled().await?;
        list_objects(&mut conn, &database).await
    }

    async fn table_details(&self, table: &TableRef) -> Result<TableDetails, DriverError> {
        let table = self.resolve_table(table)?;
        let mut conn = self.pooled().await?;
        let kind = relation_kind_of(&mut conn, &table).await?;
        let indexes = indexes(&mut conn, &table).await?;
        let primary_key = primary_key(&indexes);
        let columns = columns(&mut conn, &table, self.server_flavor, &primary_key).await?;
        let foreign_keys = foreign_keys(&mut conn, &table).await?;
        let definition = tolerate_query_error(show_create(&mut conn, kind, &table).await)?;
        Ok(TableDetails {
            table,
            kind,
            columns,
            primary_key,
            indexes,
            foreign_keys,
            definition,
        })
    }

    async fn object_definition(&self, object: &ObjectInfo) -> Result<Option<String>, DriverError> {
        let target = self.resolve_table(&object.object)?;
        let mut conn = self.pooled().await?;
        show_create(&mut conn, object.kind, &target).await
    }
}

fn tolerate_query_error(
    definition: Result<Option<String>, DriverError>,
) -> Result<Option<String>, DriverError> {
    match definition {
        Err(error) if error.kind == ErrorKind::Query => Ok(None),
        other => other,
    }
}
