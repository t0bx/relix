/*
 * @author t0bx
 * @since 23.10.2026
 */

mod client;
mod connection;
mod driver;
mod handshake;
mod line_reader;
mod pending_calls;
mod stderr_tail;

pub use connection::ExternalConnection;
pub use driver::ExternalDriver;
