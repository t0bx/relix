/*
 * @author t0bx
 * @since 23.10.2026
 */

mod api_endpoint;
mod endpoint;
mod errors;
mod poll_backoff;
mod running_job;
mod text_position;

pub use api_endpoint::RawResponse;
pub use endpoint::{EndpointOptions, HttpAuth, HttpEndpoint, PATH_PREFIX};
pub use errors::{is_tls_message, map_status_error, map_transport_error};
pub use poll_backoff::PollBackoff;
pub use reqwest::{Method, RequestBuilder, Response, StatusCode, Url, header};
pub use running_job::{RunningJob, RunningJobGuard};
pub use text_position::char_offset;
