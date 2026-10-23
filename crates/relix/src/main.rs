/*
 * @author t0bx
 * @since 23.10.2026
 */

mod cli;
mod driver_service;
mod logging;
mod startup;

use clap::Parser;
use std::process::ExitCode;

fn main() -> ExitCode {
    let cli = cli::Cli::parse();
    match startup::run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            relix_tui::terminal::restore();
            eprintln!("relix: {error:#}");
            ExitCode::FAILURE
        }
    }
}
