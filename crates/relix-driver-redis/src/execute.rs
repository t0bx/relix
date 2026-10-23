/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::command_guard::ensure_command_allowed;
use crate::reply::command_reply;
use crate::runner::CommandRunner;
use relix_core::keyvalue::CommandReply;
use relix_driver_api::DriverError;

/// Runs a console command. Error replies from the server are returned as
/// [`CommandReply::Error`] like `redis-cli` shows them; only transport failures are `Err`.
pub async fn execute_command(
    runner: &CommandRunner,
    arguments: &[String],
) -> Result<CommandReply, DriverError> {
    ensure_command_allowed(arguments)?;
    let mut command = redis::Cmd::new();
    for argument in arguments {
        command.arg(argument.as_bytes());
    }
    runner.raw(&command).await.map(command_reply)
}
