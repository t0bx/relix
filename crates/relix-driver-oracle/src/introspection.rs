/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::catalog::{list_objects, list_schemas};
use crate::connection::OracleConnection;
use crate::definitions::object_definition;
use crate::table_structure::table_structure;
use async_trait::async_trait;
use relix_core::{DatabaseInfo, ObjectInfo, TableDetails, TableRef};
use relix_driver_api::{DriverError, ErrorKind, SqlIntrospector};

#[async_trait]
impl SqlIntrospector for OracleConnection {
    async fn databases(&self) -> Result<Vec<DatabaseInfo>, DriverError> {
        Ok(vec![DatabaseInfo {
            name: self.service_name.clone(),
            is_default: true,
        }])
    }

    async fn schemas(&self, _database: &str) -> Result<Vec<String>, DriverError> {
        self.data.run(list_schemas).await
    }

    async fn objects(
        &self,
        _database: &str,
        schema: Option<&str>,
    ) -> Result<Vec<ObjectInfo>, DriverError> {
        let schema = self.resolve_schema(schema);
        let service_name = self.service_name.clone();
        self.data
            .run(move |connection| list_objects(connection, &service_name, &schema))
            .await
    }

    async fn table_details(&self, table: &TableRef) -> Result<TableDetails, DriverError> {
        let table = self.resolve_table(table);
        self.data
            .run(move |connection| {
                let structure = table_structure(connection, &table)?;
                let definition =
                    tolerate_query_error(object_definition(connection, structure.kind, &table))?;
                Ok(TableDetails {
                    table,
                    kind: structure.kind,
                    columns: structure.columns,
                    primary_key: structure.primary_key,
                    indexes: structure.indexes,
                    foreign_keys: structure.foreign_keys,
                    definition,
                })
            })
            .await
    }

    async fn object_definition(&self, object: &ObjectInfo) -> Result<Option<String>, DriverError> {
        let target = self.resolve_table(&object.object);
        let kind = object.kind;
        self.data
            .run(move |connection| object_definition(connection, kind, &target))
            .await
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
