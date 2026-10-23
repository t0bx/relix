/*
 * @author t0bx
 * @since 23.10.2026
 */

use relix_driver_api::DRIVER_API_VERSION;

pub fn current_target() -> String {
    format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)
}

pub fn check_api_version(api_version: u32) -> Result<(), String> {
    if api_version == DRIVER_API_VERSION {
        return Ok(());
    }
    Err(format!(
        "requires driver API version {api_version}, this RELIX build provides version {DRIVER_API_VERSION}"
    ))
}

pub fn check_target(package_target: &str, host_target: &str) -> Result<(), String> {
    if package_target == host_target {
        return Ok(());
    }
    Err(format!(
        "built for {package_target}, this machine is {host_target}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_target_combines_os_and_architecture() {
        let target = current_target();
        assert!(target.starts_with(std::env::consts::OS));
        assert!(target.ends_with(std::env::consts::ARCH));
    }

    #[test]
    fn mismatches_are_explained() {
        assert!(check_api_version(DRIVER_API_VERSION).is_ok());
        assert!(
            check_api_version(DRIVER_API_VERSION + 1)
                .unwrap_err()
                .contains("driver API version")
        );
        assert!(check_target("linux-x86_64", "linux-x86_64").is_ok());
        assert!(check_target("windows-x86_64", "linux-x86_64").is_err());
    }
}
