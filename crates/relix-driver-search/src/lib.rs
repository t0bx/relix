/*
 * @author t0bx
 * @since 23.10.2026
 */

mod auth;
mod cancel;
mod catalog;
mod cells;
mod client;
mod cluster;
mod connect;
mod connection;
mod console;
mod describe;
mod document_key;
mod driver;
mod find;
mod flavor;
mod hit_rows;
mod hits;
mod index_admin;
mod index_stats;
mod json_rows;
mod options;
mod page_token;
mod paths;
mod request;
mod request_body;
mod resolve;
mod response_outcome;
mod result_window;
mod search_body;
mod writes;

pub use connect::DEFAULT_PORT;
pub use connection::FILTER_HINT;
pub use driver::{ElasticsearchDriver, OpenSearchDriver};
