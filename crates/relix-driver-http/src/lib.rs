/*
 * @author t0bx
 * @since 23.10.2026
 */

mod endpoint;
mod errors;

pub use endpoint::{EndpointOptions, HttpAuth, HttpEndpoint};
pub use errors::{is_tls_message, map_status_error, map_transport_error};
pub use reqwest::{Method, RequestBuilder, Response, StatusCode, Url, header};
