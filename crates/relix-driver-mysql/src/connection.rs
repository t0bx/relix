/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::errors::map_error;
use crate::flavor::Flavor;
use async_trait::async_trait;
use mysql_async::prelude::Queryable;
use mysql_async::{Conn, Pool};
use relix_core::{Dialect, TableRef};
use relix_driver_api::{
    DatabaseConnection, DriverError, ParamStyle, QueryExecutor, ServerInfo, SqlBuilder,
    SqlConnection,
};
use tokio::sync::Mutex;

pub struct MySqlConnection {
    pub(crate) server_flavor: Flavor,
    pub(crate) server_info: ServerInfo,
    pub(crate) session: Mutex<Option<Conn>>,
    pub(crate) session_id: u32,
    pub(crate) pool: Pool,
    pub(crate) default_database: Option<String>,
}

impl MySqlConnection {
    pub(crate) async fn pooled(&self) -> Result<Conn, DriverError> {
        self.pool.get_conn().await.map_err(map_error)
    }

    pub(crate) fn resolve_database(&self, database: Option<&str>) -> Result<String, DriverError> {
        database
            .filter(|name| !name.is_empty())
            .or(self.default_database.as_deref())
            .map(str::to_owned)
            .ok_or_else(|| DriverError::configuration("no database selected"))
    }

    pub(crate) fn resolve_table(&self, table: &TableRef) -> Result<TableRef, DriverError> {
        let database = self.resolve_database(table.database.as_deref())?;
        Ok(TableRef::new(Some(database), None, table.name.clone()))
    }
}

pub(crate) fn sql_builder() -> SqlBuilder {
    SqlBuilder::new(Dialect::MySql, ParamStyle::QuestionMark)
}

#[async_trait]
impl DatabaseConnection for MySqlConnection {
    fn server_info(&self) -> ServerInfo {
        self.server_info.clone()
    }

    async fn ping(&self) -> Result<(), DriverError> {
        let mut conn = self.pooled().await?;
        conn.ping().await.map_err(map_error)
    }

    async fn close(&self) -> Result<(), DriverError> {
        let mut session = match self.session.try_lock() {
            Ok(session) => session,
            Err(_) => {
                let _ = self.cancel().await;
                self.session.lock().await
            }
        };
        let Some(conn) = session.take() else {
            return Ok(());
        };
        drop(session);
        let disconnected = conn.disconnect().await.map_err(map_error);
        self.pool.clone().disconnect().await.map_err(map_error)?;
        disconnected
    }

    fn as_sql(&self) -> Option<&dyn SqlConnection> {
        Some(self)
    }
}
