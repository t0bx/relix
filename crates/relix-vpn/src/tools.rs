/*
 * @author t0bx
 * @since 23.10.2026
 */

use std::path::{Path, PathBuf};

const EXTRA_UNIX_DIRS: &[&str] = &[
    "/usr/local/sbin",
    "/usr/local/bin",
    "/usr/sbin",
    "/sbin",
    "/opt/homebrew/bin",
    "/opt/homebrew/sbin",
];

const EXTRA_WINDOWS_DIRS: &[&str] = &[
    r"C:\Program Files\WireGuard",
    r"C:\Program Files\OpenVPN\bin",
];

pub fn find_executable(name: &str) -> Option<PathBuf> {
    let path_dirs = std::env::var_os("PATH")
        .map(|paths| std::env::split_paths(&paths).collect::<Vec<_>>())
        .unwrap_or_default();
    let extra: &[&str] = if cfg!(windows) {
        EXTRA_WINDOWS_DIRS
    } else {
        EXTRA_UNIX_DIRS
    };
    path_dirs
        .into_iter()
        .chain(extra.iter().map(PathBuf::from))
        .flat_map(|dir| candidates(&dir, name))
        .find(|candidate| candidate.is_file())
}

fn candidates(dir: &Path, name: &str) -> Vec<PathBuf> {
    if cfg!(windows) && Path::new(name).extension().is_none() {
        vec![dir.join(format!("{name}.exe")), dir.join(name)]
    } else {
        vec![dir.join(name)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_programs_on_the_path() {
        let program = if cfg!(windows) { "cmd" } else { "sh" };
        assert!(find_executable(program).is_some());
        assert!(find_executable("relix-definitely-missing-tool").is_none());
    }
}
