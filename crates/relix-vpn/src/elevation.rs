/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::command::{CommandOutput, SystemCommand};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElevationMethod {
    Pkexec,
    AppleScript,
    PowerShellRunAs,
}

impl ElevationMethod {
    pub fn current() -> Option<Self> {
        if cfg!(target_os = "macos") {
            Some(Self::AppleScript)
        } else if cfg!(windows) {
            Some(Self::PowerShellRunAs)
        } else if cfg!(unix) {
            Some(Self::Pkexec)
        } else {
            None
        }
    }

    /// Wraps a command so it runs with administrator rights after the operating system
    /// asked the user. Only the single command is elevated, never Relix itself. Each
    /// argument is quoted for the respective interpreter, and arguments containing
    /// control characters are refused rather than escaped.
    pub fn wrap(self, command: &SystemCommand) -> Option<SystemCommand> {
        let parts: Vec<&str> = std::iter::once(command.program.as_str())
            .chain(command.args.iter().map(String::as_str))
            .collect();
        if parts.iter().any(|part| part.chars().any(char::is_control)) {
            return None;
        }
        Some(match self {
            Self::Pkexec => {
                let mut args = vec![command.program.clone()];
                args.extend(command.args.iter().cloned());
                SystemCommand {
                    program: "pkexec".into(),
                    args,
                }
            }
            Self::AppleScript => {
                let shell = parts
                    .iter()
                    .map(|part| posix_quote(part))
                    .collect::<Vec<_>>()
                    .join(" ");
                let script = format!(
                    "do shell script {} with administrator privileges",
                    applescript_string(&shell)
                );
                SystemCommand {
                    program: "osascript".into(),
                    args: vec!["-e".into(), script],
                }
            }
            Self::PowerShellRunAs => {
                let arguments = command
                    .args
                    .iter()
                    .map(|arg| powershell_quote(arg))
                    .collect::<Vec<_>>()
                    .join(",");
                let argument_list = if arguments.is_empty() {
                    String::new()
                } else {
                    format!(" -ArgumentList {arguments}")
                };
                let script = format!(
                    "$process = Start-Process -FilePath {}{argument_list} -Verb RunAs -Wait \
                     -WindowStyle Hidden -PassThru; exit $process.ExitCode",
                    powershell_quote(&command.program)
                );
                SystemCommand {
                    program: "powershell".into(),
                    args: vec![
                        "-NoProfile".into(),
                        "-NonInteractive".into(),
                        "-Command".into(),
                        script,
                    ],
                }
            }
        })
    }

    pub fn was_denied(self, output: &CommandOutput) -> bool {
        let stderr = output.stderr.to_lowercase();
        match self {
            Self::Pkexec => matches!(output.code, Some(126 | 127)),
            Self::AppleScript => stderr.contains("user canceled") || stderr.contains("(-128)"),
            Self::PowerShellRunAs => stderr.contains("canceled by the user"),
        }
    }
}

fn posix_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn applescript_string(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

fn powershell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn up(path: &str) -> SystemCommand {
        SystemCommand::new("wg-quick", &["up", path])
    }

    #[test]
    fn pkexec_passes_arguments_without_a_shell() {
        let wrapped = ElevationMethod::Pkexec
            .wrap(&up("/etc/x; rm -rf /"))
            .unwrap();
        assert_eq!(wrapped.program, "pkexec");
        assert_eq!(wrapped.args, vec!["wg-quick", "up", "/etc/x; rm -rf /"]);
    }

    #[test]
    fn applescript_quotes_for_the_shell_and_the_script() {
        let wrapped = ElevationMethod::AppleScript
            .wrap(&up("/Users/me/it's \"prod\".conf"))
            .unwrap();
        assert_eq!(wrapped.program, "osascript");
        assert_eq!(
            wrapped.args[1],
            r#"do shell script "'wg-quick' 'up' '/Users/me/it'\\''s \"prod\".conf'" with administrator privileges"#
        );
    }

    #[test]
    fn powershell_doubles_single_quotes() {
        let wrapped = ElevationMethod::PowerShellRunAs
            .wrap(&SystemCommand::new(
                "wireguard.exe",
                &["/installtunnelservice", "C:\\vpn\\o'neil.conf"],
            ))
            .unwrap();
        assert!(wrapped.args[3].contains("-FilePath 'wireguard.exe'"));
        assert!(wrapped.args[3].contains("'/installtunnelservice','C:\\vpn\\o''neil.conf'"));
    }

    #[test]
    fn control_characters_are_refused() {
        assert!(ElevationMethod::AppleScript.wrap(&up("a\nb")).is_none());
        assert!(ElevationMethod::Pkexec.wrap(&up("a\0b")).is_none());
    }

    #[test]
    fn denial_is_detected_per_method() {
        let pkexec = CommandOutput {
            code: Some(126),
            ..CommandOutput::default()
        };
        assert!(ElevationMethod::Pkexec.was_denied(&pkexec));
        let osascript = CommandOutput {
            code: Some(1),
            stderr: "execution error: User canceled. (-128)".into(),
            ..CommandOutput::default()
        };
        assert!(ElevationMethod::AppleScript.was_denied(&osascript));
        assert!(!ElevationMethod::AppleScript.was_denied(&CommandOutput::default()));
    }
}
