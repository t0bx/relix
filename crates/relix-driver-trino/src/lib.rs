/*
 * @author t0bx
 * @since 23.10.2026
 */

mod authentication;
mod catalog;
mod client;
mod connection;
mod driver;
mod errors;
mod execution;
mod introspection;
mod protocol;
mod session_state;
mod statement_runner;
mod table_data;
mod table_details;
mod url_codec;
mod values;

pub use driver::TrinoDriver;
