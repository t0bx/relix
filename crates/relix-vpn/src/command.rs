/*
 * @author t0bx
 * @since 23.10.2026
 */

use async_trait::async_trait;
use std::fmt;
use std::process::Stdio;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemCommand {
    pub program: String,
    pub args: Vec<String>,
}

impl SystemCommand {
    pub fn new(program: impl Into<String>, args: &[&str]) -> Self {
        Self {
            program: program.into(),
            args: args.iter().map(|arg| (*arg).to_owned()).collect(),
        }
    }
}

impl fmt::Display for SystemCommand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.program)?;
        for arg in &self.args {
            if arg.is_empty() || arg.contains(char::is_whitespace) {
                write!(f, " \"{arg}\"")?;
            } else {
                write!(f, " {arg}")?;
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CommandOutput {
    pub success: bool,
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

#[async_trait]
pub trait CommandRunner: Send + Sync {
    async fn run(&self, command: &SystemCommand) -> std::io::Result<CommandOutput>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct SystemRunner;

#[async_trait]
impl CommandRunner for SystemRunner {
    async fn run(&self, command: &SystemCommand) -> std::io::Result<CommandOutput> {
        let output = tokio::process::Command::new(&command.program)
            .args(&command.args)
            .stdin(Stdio::null())
            .kill_on_drop(true)
            .output()
            .await?;
        Ok(CommandOutput {
            success: output.status.success(),
            code: output.status.code(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_quotes_arguments_with_spaces() {
        let command = SystemCommand::new("wg-quick", &["up", "/path with space/prod.conf"]);
        assert_eq!(
            command.to_string(),
            "wg-quick up \"/path with space/prod.conf\""
        );
    }
}
