/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::abort::AbortSignal;
use crate::errors::{connection_closed, map_sdk_error};
use async_trait::async_trait;
use aws_sdk_dynamodb::Client;
use aws_sdk_dynamodb::types::{KeyType, TableDescription};
use relix_driver_api::{
    DatabaseConnection, DocumentConnection, DriverError, QueryExecutor, ServerInfo,
};
use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};

pub struct DynamoConnection {
    client: Mutex<Option<Client>>,
    pub(crate) region: String,
    pub(crate) abort: AbortSignal,
    key_fields: Mutex<HashMap<String, Vec<String>>>,
    server_info: ServerInfo,
}

impl DynamoConnection {
    pub(crate) fn new(client: Client, region: String, server_info: ServerInfo) -> Self {
        Self {
            client: Mutex::new(Some(client)),
            region,
            abort: AbortSignal::default(),
            key_fields: Mutex::new(HashMap::new()),
            server_info,
        }
    }

    pub(crate) fn client(&self) -> Result<Client, DriverError> {
        self.client
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
            .ok_or_else(connection_closed)
    }

    pub(crate) async fn describe_table(
        &self,
        table: &str,
    ) -> Result<TableDescription, DriverError> {
        let output = self
            .client()?
            .describe_table()
            .table_name(table)
            .send()
            .await
            .map_err(map_sdk_error)?;
        let description = output
            .table
            .ok_or_else(|| DriverError::query(format!("table {table} has no description")))?;
        self.remember_key_fields(table, &description);
        Ok(description)
    }

    /// Returns the table's key attributes (partition key first, then the sort key),
    /// cached per table because every page and write needs them.
    pub(crate) async fn key_fields(&self, table: &str) -> Result<Vec<String>, DriverError> {
        let cached = self
            .key_fields
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(table)
            .cloned();
        match cached {
            Some(fields) => Ok(fields),
            None => Ok(table_key_fields(&self.describe_table(table).await?)),
        }
    }

    fn remember_key_fields(&self, table: &str, description: &TableDescription) {
        self.key_fields
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(table.to_owned(), table_key_fields(description));
    }
}

pub fn table_key_fields(description: &TableDescription) -> Vec<String> {
    let mut elements: Vec<_> = description.key_schema().iter().collect();
    elements.sort_by_key(|element| *element.key_type() != KeyType::Hash);
    elements
        .into_iter()
        .map(|element| element.attribute_name().to_owned())
        .collect()
}

#[async_trait]
impl DatabaseConnection for DynamoConnection {
    fn server_info(&self) -> ServerInfo {
        self.server_info.clone()
    }

    async fn ping(&self) -> Result<(), DriverError> {
        self.client()?
            .list_tables()
            .limit(1)
            .send()
            .await
            .map(|_| ())
            .map_err(map_sdk_error)
    }

    async fn close(&self) -> Result<(), DriverError> {
        self.client
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        Ok(())
    }

    fn as_query(&self) -> Option<&dyn QueryExecutor> {
        Some(self)
    }

    fn as_document(&self) -> Option<&dyn DocumentConnection> {
        Some(self)
    }
}
