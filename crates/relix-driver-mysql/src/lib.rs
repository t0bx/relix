/*
 * @author t0bx
 * @since 23.10.2026
 */

mod catalog;
mod catalog_rows;
mod changes;
mod column_type;
mod connect;
mod connection;
mod definitions;
mod driver;
mod errors;
mod execution;
mod flavor;
mod introspection;
mod result_rows;
mod statement_kind;
mod table_data;
mod table_structure;
mod tls;
mod values;

pub use connection::MySqlConnection;
pub use driver::{MariaDbDriver, MySqlDriver};
pub use flavor::Flavor;
