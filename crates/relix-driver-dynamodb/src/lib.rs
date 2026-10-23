/*
 * @author t0bx
 * @since 23.10.2026
 */

mod abort;
mod attribute_json;
mod catalog;
mod connect;
mod connection;
mod describe;
mod document_access;
mod driver;
mod dynamodb_json;
mod errors;
mod execution;
mod find;
mod index_definition;
mod number_text;
mod read_request;
mod schema_json;
mod sdk_config;
mod writes;

pub use driver::DynamoDbDriver;
