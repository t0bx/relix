/*
 * @author t0bx
 * @since 23.10.2026
 */

//! Building blocks for external RELIX connectors.
//!
//! A connector implements the regular [`DatabaseDriver`] trait (exactly like a built-in
//! driver) and hands it to [`serve`] from its `main` function. The SDK speaks the JSON-lines
//! protocol of [`relix_driver_api::ipc`] on stdin/stdout, so a connector must never print
//! to stdout itself; diagnostics belong on stderr.

mod dispatch;
mod serve;
mod session;

pub use relix_driver_api::DatabaseDriver;
pub use serve::{ServeError, serve, serve_io};
