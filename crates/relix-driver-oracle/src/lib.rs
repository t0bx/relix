/*
 * @author t0bx
 * @since 23.10.2026
 */

mod blocking;
mod catalog;
mod catalog_rows;
mod changes;
mod client_library;
mod column_type;
mod connect_target;
mod connection;
mod connector;
mod data_connections;
mod definitions;
mod driver;
mod errors;
mod execution;
mod introspection;
mod params;
mod result_rows;
mod session;
mod statement_text;
mod table_data;
mod table_structure;
mod values;

pub use connection::OracleConnection;
pub use driver::OracleDriver;
