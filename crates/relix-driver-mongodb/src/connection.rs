/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::cancellation::CommandTracker;
use crate::connect::ADMIN_DATABASE;
use crate::errors::{connection_closed, map_error};
use async_trait::async_trait;
use mongodb::bson::{Document, doc};
use mongodb::{Client, Collection, Database};
use relix_core::CollectionRef;
use relix_driver_api::{
    DatabaseConnection, DocumentConnection, DriverError, QueryExecutor, ServerInfo,
};
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

const SHUTDOWN_GRACE: Duration = Duration::from_secs(2);

pub struct MongoConnection {
    client: Mutex<Option<Client>>,
    pub(crate) default_database: String,
    current_database: Mutex<String>,
    pub(crate) supports_comment: bool,
    pub(crate) query_timeout: Option<Duration>,
    pub(crate) tracker: CommandTracker,
    server_info: ServerInfo,
}

impl MongoConnection {
    pub(crate) fn new(
        client: Client,
        default_database: String,
        server_info: ServerInfo,
        supports_comment: bool,
        query_timeout: Option<Duration>,
    ) -> Self {
        Self {
            client: Mutex::new(Some(client)),
            current_database: Mutex::new(default_database.clone()),
            default_database,
            supports_comment,
            query_timeout,
            tracker: CommandTracker::default(),
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

    pub(crate) fn database(&self, name: &str) -> Result<Database, DriverError> {
        Ok(self.client()?.database(name))
    }

    pub(crate) fn database_of(&self, collection: &CollectionRef) -> Result<Database, DriverError> {
        if collection.namespace.is_empty() {
            self.database(&self.default_database)
        } else {
            self.database(&collection.namespace)
        }
    }

    pub(crate) fn collection(
        &self,
        collection: &CollectionRef,
    ) -> Result<Collection<Document>, DriverError> {
        Ok(self.database_of(collection)?.collection(&collection.name))
    }

    pub(crate) fn current_database(&self) -> String {
        self.current_database
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    pub(crate) fn use_database(&self, name: &str) {
        *self
            .current_database
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = name.to_owned();
    }
}

#[async_trait]
impl DatabaseConnection for MongoConnection {
    fn server_info(&self) -> ServerInfo {
        self.server_info.clone()
    }

    async fn ping(&self) -> Result<(), DriverError> {
        self.database(ADMIN_DATABASE)?
            .run_command(doc! { "ping": 1 })
            .await
            .map(|_| ())
            .map_err(map_error)
    }

    async fn close(&self) -> Result<(), DriverError> {
        let client = self
            .client
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(client) = client {
            let _ = tokio::time::timeout(SHUTDOWN_GRACE, client.shutdown()).await;
        }
        Ok(())
    }

    fn as_query(&self) -> Option<&dyn QueryExecutor> {
        Some(self)
    }

    fn as_document(&self) -> Option<&dyn DocumentConnection> {
        Some(self)
    }
}
