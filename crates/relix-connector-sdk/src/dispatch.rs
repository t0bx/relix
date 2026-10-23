/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::session::{Session, sql};
use relix_driver_api::ipc::{HandshakeResult, IPC_PROTOCOL_VERSION, IpcCall};
use relix_driver_api::{DRIVER_API_VERSION, DatabaseDriver, DriverError, ErrorKind};
use serde::Serialize;
use serde_json::Value;

/// Executes one request against the driver or the open connection and encodes the result
/// in the shape the RELIX side expects for that method.
pub(crate) async fn dispatch(
    driver: &dyn DatabaseDriver,
    session: &Session,
    call: IpcCall,
) -> Result<Value, DriverError> {
    match call {
        IpcCall::Handshake { protocol_version } => handshake(driver, protocol_version),
        IpcCall::Connect(config) => {
            let connection = driver.connect(&config.into_config()).await?;
            let info = connection.server_info();
            session.open(connection)?;
            encode(info)
        }
        IpcCall::Close {} => match session.take() {
            Some(connection) => encode(connection.close().await?),
            None => encode(()),
        },
        call => dispatch_connected(session, call).await,
    }
}

async fn dispatch_connected(session: &Session, call: IpcCall) -> Result<Value, DriverError> {
    let connection = session.current()?;
    let connection = connection.as_ref();
    match call {
        IpcCall::Ping {} => encode(connection.ping().await?),
        IpcCall::ServerInfo {} => encode(connection.server_info()),
        IpcCall::Databases {} => encode(sql(connection)?.databases().await?),
        IpcCall::Schemas { database } => encode(sql(connection)?.schemas(&database).await?),
        IpcCall::Objects { database, schema } => encode(
            sql(connection)?
                .objects(&database, schema.as_deref())
                .await?,
        ),
        IpcCall::TableDetails { table } => encode(sql(connection)?.table_details(&table).await?),
        IpcCall::ObjectDefinition { object } => {
            encode(sql(connection)?.object_definition(&object).await?)
        }
        IpcCall::Execute { statement, options } => {
            encode(sql(connection)?.execute(&statement, &options).await?)
        }
        IpcCall::Cancel {} => encode(sql(connection)?.cancel().await?),
        IpcCall::FetchTablePage { table, query } => {
            encode(sql(connection)?.fetch_table_page(&table, &query).await?)
        }
        IpcCall::CountRows { table, filter } => encode(
            sql(connection)?
                .count_rows(&table, filter.as_deref())
                .await?,
        ),
        IpcCall::ApplyChanges { changes } => {
            encode(sql(connection)?.apply_changes(&changes).await?)
        }
        IpcCall::Handshake { .. } | IpcCall::Connect(_) | IpcCall::Close {} => Err(
            DriverError::internal("lifecycle requests are handled before the session"),
        ),
    }
}

fn handshake(driver: &dyn DatabaseDriver, protocol_version: u32) -> Result<Value, DriverError> {
    if protocol_version != IPC_PROTOCOL_VERSION {
        return Err(DriverError::new(
            ErrorKind::Protocol,
            format!(
                "RELIX speaks protocol version {protocol_version}, this connector speaks {IPC_PROTOCOL_VERSION}"
            ),
        ));
    }
    encode(HandshakeResult {
        protocol_version: IPC_PROTOCOL_VERSION,
        api_version: DRIVER_API_VERSION,
        metadata: driver.metadata(),
        capabilities: driver.capabilities(),
    })
}

fn encode<T: Serialize>(value: T) -> Result<Value, DriverError> {
    serde_json::to_value(value)
        .map_err(|error| DriverError::internal(format!("cannot encode result: {error}")))
}
