/*
 * @author t0bx
 * @since 23.10.2026
 */

mod blocking;
mod catalog;
mod changes;
mod connection;
mod driver;
mod errors;
mod execution;
mod introspection;
mod open;
mod query;
mod rows;
mod shared_connection;
mod statement_kind;
mod table_data;
mod table_details;
mod values;

pub use driver::SqliteDriver;
