/*
 * @author t0bx
 * @since 23.10.2026
 */

mod column_kind;
mod connection;
mod csv_reader;
mod driver;
mod loader;
mod statement_guard;

use driver::CsvDriver;
use std::process::ExitCode;
use std::sync::Arc;

#[tokio::main]
async fn main() -> ExitCode {
    match relix_connector_sdk::serve(Arc::new(CsvDriver::new())).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("relix-csv-connector: {error}");
            ExitCode::FAILURE
        }
    }
}
