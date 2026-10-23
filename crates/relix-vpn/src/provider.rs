/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::command::SystemCommand;
use crate::error::VpnError;
use crate::network::NetworkInspector;
use relix_core::{VpnProfile, VpnProvider};
use std::path::{Path, PathBuf};

const WIREGUARD_RUN_DIR: &str = "/var/run/wireguard";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderPlan {
    pub up: SystemCommand,
    pub tool: String,
    pub pid_file: Option<PathBuf>,
}

pub fn plan(profile: &VpnProfile, runtime_dir: &Path) -> Result<ProviderPlan, VpnError> {
    let config = config_path(profile)?;
    let config_text = config.display().to_string();
    match profile.provider {
        VpnProvider::WireGuard => {
            validate_wireguard_name(profile, &config)?;
            if cfg!(windows) {
                Ok(ProviderPlan {
                    up: SystemCommand::new(
                        "wireguard.exe",
                        &["/installtunnelservice", &config_text],
                    ),
                    tool: "wireguard.exe".into(),
                    pid_file: None,
                })
            } else {
                Ok(ProviderPlan {
                    up: SystemCommand::new("wg-quick", &["up", &config_text]),
                    tool: "wg-quick".into(),
                    pid_file: None,
                })
            }
        }
        VpnProvider::OpenVpn if cfg!(windows) => Err(VpnError::UnsupportedPlatform),
        VpnProvider::OpenVpn => {
            let pid_file = runtime_dir.join(format!("{}.pid", profile.id));
            let log_file = runtime_dir.join(format!("{}.log", profile.id));
            let daemon = format!("relix-{}", profile.id);
            Ok(ProviderPlan {
                up: SystemCommand::new(
                    "openvpn",
                    &[
                        "--config",
                        &config_text,
                        "--daemon",
                        &daemon,
                        "--writepid",
                        &pid_file.display().to_string(),
                        "--log",
                        &log_file.display().to_string(),
                    ],
                ),
                tool: "openvpn".into(),
                pid_file: Some(pid_file),
            })
        }
        VpnProvider::Other => Err(VpnError::Configuration {
            profile: profile.name.clone(),
            message: "only WireGuard and OpenVPN profiles can be managed by Relix".into(),
        }),
    }
}

pub fn down_command(
    profile: &VpnProfile,
    plan: &ProviderPlan,
    inspector: &dyn NetworkInspector,
) -> Result<SystemCommand, VpnError> {
    match profile.provider {
        VpnProvider::WireGuard if cfg!(windows) => Ok(SystemCommand::new(
            "wireguard.exe",
            &[
                "/uninstalltunnelservice",
                &tunnel_name(&config_path(profile)?),
            ],
        )),
        VpnProvider::WireGuard => Ok(SystemCommand::new(
            "wg-quick",
            &["down", &config_path(profile)?.display().to_string()],
        )),
        _ => {
            let pid = plan
                .pid_file
                .as_deref()
                .and_then(|path| inspector.read_file(path))
                .and_then(|text| text.trim().parse::<u32>().ok())
                .ok_or_else(|| VpnError::Configuration {
                    profile: profile.name.clone(),
                    message: "the OpenVPN process id is unknown".into(),
                })?;
            Ok(SystemCommand::new("kill", &["-TERM", &pid.to_string()]))
        }
    }
}

/// The interface a profile is expected to create. An explicit interface always wins;
/// WireGuard names it after the configuration file, except on macOS where `wg-quick`
/// picks a `utunN` device and records the mapping in `/var/run/wireguard/<name>.name`.
pub fn expected_interface(
    profile: &VpnProfile,
    inspector: &dyn NetworkInspector,
) -> Option<String> {
    if let Some(interface) = profile.interface.as_deref().filter(|name| !name.is_empty()) {
        return Some(interface.to_owned());
    }
    if profile.provider != VpnProvider::WireGuard {
        return None;
    }
    let name = tunnel_name(profile.config_path.as_deref()?);
    if cfg!(target_os = "macos") {
        let mapping = Path::new(WIREGUARD_RUN_DIR).join(format!("{name}.name"));
        return inspector
            .read_file(&mapping)
            .map(|device| device.trim().to_owned());
    }
    Some(name)
}

fn config_path(profile: &VpnProfile) -> Result<PathBuf, VpnError> {
    profile
        .config_path
        .clone()
        .filter(|path| !path.as_os_str().is_empty())
        .ok_or_else(|| VpnError::Configuration {
            profile: profile.name.clone(),
            message: "no configuration file selected".into(),
        })
}

fn tunnel_name(path: &Path) -> String {
    path.file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn validate_wireguard_name(profile: &VpnProfile, config: &Path) -> Result<(), VpnError> {
    let name = tunnel_name(config);
    let valid = !name.is_empty()
        && name.len() <= 15
        && name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || "_=+.-".contains(ch));
    let is_conf = config.extension().is_some_and(|ext| ext == "conf");
    if valid && is_conf {
        Ok(())
    } else {
        Err(VpnError::Configuration {
            profile: profile.name.clone(),
            message: "WireGuard files must be named <name>.conf with at most 15 characters \
                      (letters, digits, _ = + . -)"
                .into(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::InterfaceInfo;

    struct NoInterfaces;

    impl NetworkInspector for NoInterfaces {
        fn interfaces(&self) -> Vec<InterfaceInfo> {
            Vec::new()
        }

        fn read_file(&self, _path: &Path) -> Option<String> {
            Some("4242\n".into())
        }
    }

    fn profile(provider: VpnProvider, path: &str) -> VpnProfile {
        let mut profile = VpnProfile::new("Production Network");
        profile.provider = provider;
        profile.config_path = Some(path.into());
        profile
    }

    #[cfg(unix)]
    #[test]
    fn wireguard_uses_wg_quick() {
        let profile = profile(
            VpnProvider::WireGuard,
            "/home/me/.config/relix/vpn/prod.conf",
        );
        let plan = plan(&profile, Path::new("/tmp")).unwrap();
        assert_eq!(plan.up.program, "wg-quick");
        assert_eq!(
            plan.up.args,
            vec!["up", "/home/me/.config/relix/vpn/prod.conf"]
        );
        let down = down_command(&profile, &plan, &NoInterfaces).unwrap();
        assert_eq!(down.args[0], "down");
    }

    #[cfg(unix)]
    #[test]
    fn openvpn_runs_as_a_tracked_daemon() {
        let profile = profile(VpnProvider::OpenVpn, "/etc/openvpn/office.ovpn");
        let plan = plan(&profile, Path::new("/var/lib/relix/vpn")).unwrap();
        assert_eq!(plan.up.program, "openvpn");
        assert!(plan.up.args.contains(&"--writepid".to_owned()));
        let down = down_command(&profile, &plan, &NoInterfaces).unwrap();
        assert_eq!(down.args, vec!["-TERM", "4242"]);
    }

    #[test]
    fn invalid_wireguard_names_are_rejected() {
        let long = profile(
            VpnProvider::WireGuard,
            "/vpn/this-name-is-far-too-long.conf",
        );
        assert!(plan(&long, Path::new("/tmp")).is_err());
        let wrong_extension = profile(VpnProvider::WireGuard, "/vpn/prod.txt");
        assert!(plan(&wrong_extension, Path::new("/tmp")).is_err());
        let missing = VpnProfile::new("Empty");
        assert!(plan(&missing, Path::new("/tmp")).is_err());
    }

    #[test]
    fn explicit_interfaces_take_precedence() {
        let mut profile = profile(VpnProvider::WireGuard, "/vpn/prod.conf");
        profile.interface = Some("wg-office".into());
        assert_eq!(
            expected_interface(&profile, &NoInterfaces).as_deref(),
            Some("wg-office")
        );
        profile.provider = VpnProvider::OpenVpn;
        profile.interface = None;
        assert_eq!(expected_interface(&profile, &NoInterfaces), None);
    }
}
