/*
 * @author t0bx
 * @since 23.10.2026
 */

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OpenSshHost {
    pub host_name: Option<String>,
    pub port: Option<u16>,
    pub user: Option<String>,
    pub identity_file: Option<PathBuf>,
}

pub fn user_config_path() -> Option<PathBuf> {
    home_dir().map(|home| home.join(".ssh").join("config"))
}

pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

pub fn expand_home(path: &Path) -> PathBuf {
    match path.strip_prefix("~") {
        Ok(rest) => home_dir().map_or_else(|| path.to_path_buf(), |home| home.join(rest)),
        Err(_) => path.to_path_buf(),
    }
}

pub fn lookup(path: &Path, alias: &str) -> Option<OpenSshHost> {
    let text = std::fs::read_to_string(path).ok()?;
    Some(resolve(&text, alias))
}

/// Applies the `Host` blocks of an OpenSSH client configuration to `alias`. Like
/// OpenSSH, the first value obtained for a keyword wins; blocks match when any of their
/// patterns matches and no negated pattern does. `Match` blocks are skipped because
/// their conditions cannot be evaluated outside of `ssh` itself.
pub fn resolve(text: &str, alias: &str) -> OpenSshHost {
    let mut host = OpenSshHost::default();
    let mut active = true;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (keyword, value) = split_keyword(line);
        let keyword = keyword.to_ascii_lowercase();
        match keyword.as_str() {
            "host" => active = host_block_matches(value, alias),
            "match" => active = false,
            _ if !active => {}
            "hostname" if host.host_name.is_none() => {
                host.host_name = Some(value.replace("%h", alias));
            }
            "port" if host.port.is_none() => host.port = value.parse().ok(),
            "user" if host.user.is_none() => host.user = Some(value.to_owned()),
            "identityfile" if host.identity_file.is_none() => {
                host.identity_file = Some(expand_home(Path::new(unquote(value))));
            }
            _ => {}
        }
    }
    host
}

fn split_keyword(line: &str) -> (&str, &str) {
    let separator = line
        .find(|ch: char| ch.is_whitespace() || ch == '=')
        .unwrap_or(line.len());
    let (keyword, rest) = line.split_at(separator);
    let value = rest.trim_start_matches(|ch: char| ch.is_whitespace() || ch == '=');
    (keyword, value.trim())
}

fn unquote(value: &str) -> &str {
    value
        .strip_prefix('"')
        .and_then(|inner| inner.strip_suffix('"'))
        .unwrap_or(value)
}

fn host_block_matches(patterns: &str, alias: &str) -> bool {
    let mut matched = false;
    for pattern in patterns.split_whitespace() {
        match pattern.strip_prefix('!') {
            Some(negated) if wildcard_match(negated, alias) => return false,
            Some(_) => {}
            None => matched |= wildcard_match(pattern, alias),
        }
    }
    matched
}

fn wildcard_match(pattern: &str, text: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    let text: Vec<char> = text.chars().collect();
    let (mut p, mut t) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while t < text.len() {
        if p < pattern.len() && (pattern[p] == '?' || pattern[p].eq_ignore_ascii_case(&text[t])) {
            p += 1;
            t += 1;
        } else if p < pattern.len() && pattern[p] == '*' {
            star = Some((p, t));
            p += 1;
        } else if let Some((star_p, star_t)) = star {
            p = star_p + 1;
            t = star_t + 1;
            star = Some((star_p, star_t + 1));
        } else {
            return false;
        }
    }
    pattern[p..].iter().all(|ch| *ch == '*')
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG: &str = "
# company hosts
Host bastion prod-*
    HostName bastion.example.com
    User deploy
    Port 2222
    IdentityFile ~/.ssh/deploy_ed25519

Host prod-db !prod-db-legacy
    HostName %h.internal
    User dba

Host *
    User fallback
    Port 22
";

    #[test]
    fn first_matching_value_wins() {
        let host = resolve(CONFIG, "bastion");
        assert_eq!(host.host_name.as_deref(), Some("bastion.example.com"));
        assert_eq!(host.user.as_deref(), Some("deploy"));
        assert_eq!(host.port, Some(2222));
        assert!(host.identity_file.unwrap().ends_with(".ssh/deploy_ed25519"));
    }

    #[test]
    fn wildcards_and_negation_are_respected() {
        let prod = resolve(CONFIG, "prod-db");
        assert_eq!(prod.host_name.as_deref(), Some("bastion.example.com"));
        let other = resolve(CONFIG, "staging");
        assert_eq!(other.host_name, None);
        assert_eq!(other.user.as_deref(), Some("fallback"));
        assert!(!host_block_matches(
            "prod-db !prod-db-legacy",
            "prod-db-legacy"
        ));
    }

    #[test]
    fn equals_separated_keywords_are_supported() {
        let host = resolve("Host x\n  Port=2200\n  User = admin\n", "x");
        assert_eq!(host.port, Some(2200));
        assert_eq!(host.user.as_deref(), Some("admin"));
    }
}
