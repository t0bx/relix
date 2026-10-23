/*
 * @author t0bx
 * @since 23.10.2026
 */

mod catalog;
mod catalog_rows;
mod changes;
mod column_type;
mod connection;
mod connector;
mod data_clients;
mod ddl;
mod definitions;
mod driver;
mod encryption;
mod errors;
mod execution;
mod introspection;
mod params;
mod result_sets;
mod row_count;
mod server_details;
mod session;
mod table_data;
mod table_structure;
mod temporal;
mod values;

pub use connection::MsSqlConnection;
pub use driver::MsSqlDriver;
