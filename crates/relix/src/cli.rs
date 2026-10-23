/*
 * @author t0bx
 * @since 23.10.2026
 */

use clap::Parser;

#[derive(Debug, Parser)]
#[command(name = "relix", version, about = "RELIX — a modern database IDE for your terminal", long_about = None)]
pub struct Cli {
    /// Open (or create) the project with this name instead of the most recent one.
    #[arg(short, long, value_name = "NAME")]
    pub project: Option<String>,

    /// Use this color theme for the session (dark, light, high-contrast or a custom theme).
    #[arg(long, value_name = "NAME")]
    pub theme: Option<String>,

    /// Disable mouse capture so the terminal's native text selection works.
    #[arg(long)]
    pub no_mouse: bool,

    /// Print the configuration, data and log directories and exit.
    #[arg(long)]
    pub print_paths: bool,
}
