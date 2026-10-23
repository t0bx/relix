/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::error::ManagerError;
use relix_core::DriverId;
use semver::Version;
use serde::{Deserialize, Serialize};

pub const INDEX_FORMAT_VERSION: u32 = 1;
pub const MAX_INDEX_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageIndex {
    pub format_version: u32,
    #[serde(default)]
    pub releases: Vec<PackageRelease>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageRelease {
    pub id: DriverId,
    pub name: String,
    pub version: Version,
    pub api_version: u32,
    #[serde(default)]
    pub description: String,
    pub artifacts: Vec<PackageArtifact>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageArtifact {
    pub target: String,
    pub url: String,
    pub size: u64,
    pub sha256: String,
    pub signature: String,
}

impl PackageIndex {
    pub fn parse(bytes: &[u8]) -> Result<Self, ManagerError> {
        let mut index: PackageIndex = serde_json::from_slice(bytes)
            .map_err(|error| ManagerError::InvalidIndex(error.to_string()))?;
        if index.format_version != INDEX_FORMAT_VERSION {
            return Err(ManagerError::InvalidIndex(format!(
                "unsupported index format version {}",
                index.format_version
            )));
        }
        for release in &mut index.releases {
            for artifact in &mut release.artifacts {
                artifact.sha256 = normalized_sha256(&artifact.sha256).ok_or_else(|| {
                    ManagerError::InvalidIndex(format!(
                        "{} {} ({}) has a malformed sha256",
                        release.id, release.version, artifact.target
                    ))
                })?;
            }
        }
        Ok(index)
    }
}

impl PackageRelease {
    pub fn artifact_for(&self, target: &str) -> Option<&PackageArtifact> {
        self.artifacts
            .iter()
            .find(|artifact| artifact.target == target)
    }
}

fn normalized_sha256(value: &str) -> Option<String> {
    let trimmed = value.trim();
    let valid = trimmed.len() == 64 && trimmed.chars().all(|ch| ch.is_ascii_hexdigit());
    valid.then(|| trimmed.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index_json(sha256: &str, format_version: u32) -> String {
        format!(
            r#"{{"format_version":{format_version},"releases":[{{"id":"csv","name":"CSV Files",
            "version":"1.2.0","api_version":1,"artifacts":[{{"target":"linux-x86_64",
            "url":"https://example.test/csv.tar.gz","size":10,"sha256":"{sha256}",
            "signature":"c2ln"}}]}}]}}"#
        )
    }

    #[test]
    fn parses_releases_and_normalizes_checksums() {
        let index = PackageIndex::parse(index_json(&"AB".repeat(32), 1).as_bytes()).unwrap();
        let release = &index.releases[0];
        assert_eq!(release.version, Version::new(1, 2, 0));
        assert_eq!(
            release.artifact_for("linux-x86_64").unwrap().sha256,
            "ab".repeat(32)
        );
        assert!(release.artifact_for("windows-x86_64").is_none());
    }

    #[test]
    fn rejects_malformed_checksums_and_unknown_formats() {
        assert!(matches!(
            PackageIndex::parse(index_json("xyz", 1).as_bytes()),
            Err(ManagerError::InvalidIndex(_))
        ));
        assert!(matches!(
            PackageIndex::parse(index_json(&"ab".repeat(32), 2).as_bytes()),
            Err(ManagerError::InvalidIndex(_))
        ));
    }
}
