/*
 * @author t0bx
 * @since 23.10.2026
 */

mod binary_text;
mod blocking;
mod catalog;
mod changes;
mod connection;
mod definitions;
mod driver;
mod errors;
mod errors_position;
mod execution;
mod introspection;
mod nested_text;
mod open;
mod query;
mod result_schema;
mod rows;
mod shared_connection;
mod statement_plan;
mod table_data;
mod table_details;
mod temporal;
mod type_names;
mod values;

pub use driver::DuckDbDriver;
