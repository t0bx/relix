/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::runner::CommandRunner;
use relix_driver_api::DriverError;

const VERSION_KEY: &str = "server.redis_version";

pub async fn server_details(runner: &CommandRunner) -> Result<Vec<(String, String)>, DriverError> {
    let mut pipeline = redis::pipe();
    pipeline.cmd("INFO").cmd("DBSIZE");
    let (info, size): (String, u64) = runner.query_pipeline(&pipeline).await?;
    let mut details = parse_info(&info);
    details.push(("dbsize".to_owned(), size.to_string()));
    Ok(details)
}

/// Parses `INFO` output into `section.key` pairs (for example `server.redis_version`),
/// skipping blank lines; `# Section` headers only set the prefix for the following keys.
pub fn parse_info(info: &str) -> Vec<(String, String)> {
    let mut section = String::new();
    let mut entries = Vec::new();
    for line in info.lines().map(str::trim).filter(|line| !line.is_empty()) {
        if let Some(header) = line.strip_prefix('#') {
            section = header.trim().to_ascii_lowercase();
            continue;
        }
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let key = if section.is_empty() {
            key.to_owned()
        } else {
            format!("{section}.{key}")
        };
        entries.push((key, value.to_owned()));
    }
    entries
}

pub fn redis_version(details: &[(String, String)]) -> Option<String> {
    details
        .iter()
        .find(|(key, _)| key == VERSION_KEY)
        .map(|(_, value)| value.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    const SAMPLE: &str = "# Server\r\nredis_version:7.4.1\r\nredis_mode:standalone\r\n\r\n# Keyspace\r\ndb0:keys=3,expires=0,avg_ttl=0\r\n";

    #[test]
    fn info_lines_are_section_qualified() {
        assert_eq!(
            parse_info(SAMPLE),
            vec![
                ("server.redis_version".to_owned(), "7.4.1".to_owned()),
                ("server.redis_mode".to_owned(), "standalone".to_owned()),
                (
                    "keyspace.db0".to_owned(),
                    "keys=3,expires=0,avg_ttl=0".to_owned()
                ),
            ]
        );
    }

    #[test]
    fn lines_without_section_keep_plain_keys() {
        assert_eq!(
            parse_info("uptime:5\ngarbage\n"),
            vec![("uptime".to_owned(), "5".to_owned())]
        );
    }

    #[test]
    fn version_is_read_from_the_server_section() {
        assert_eq!(redis_version(&parse_info(SAMPLE)).as_deref(), Some("7.4.1"));
        assert_eq!(redis_version(&parse_info("# Clients\r\n")), None);
    }
}
