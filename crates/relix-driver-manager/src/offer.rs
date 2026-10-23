/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::compatibility::check_api_version;
use crate::fetcher::is_https;
use crate::index::{PackageArtifact, PackageIndex, PackageRelease};
use crate::verification::MAX_PACKAGE_BYTES;
use relix_core::DriverId;
use semver::Version;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageOffer {
    pub registry: String,
    pub release: PackageRelease,
    pub artifact: PackageArtifact,
}

impl PackageOffer {
    pub fn id(&self) -> &DriverId {
        &self.release.id
    }

    pub fn version(&self) -> &Version {
        &self.release.version
    }

    pub fn size(&self) -> u64 {
        self.artifact.size
    }
}

#[derive(Debug, Clone)]
pub(crate) struct CachedIndex {
    pub(crate) registry: String,
    pub(crate) index: PackageIndex,
}

/// Picks the newest release of `id` that can run here: matching driver API version, an
/// artifact for `target` served over HTTPS and within the size limit. Registries are
/// searched in configuration order, so on equal versions the first registry wins.
pub(crate) fn best_offer(
    indexes: &[CachedIndex],
    id: &DriverId,
    target: &str,
) -> Option<PackageOffer> {
    let mut best: Option<PackageOffer> = None;
    for cached in indexes {
        for release in cached
            .index
            .releases
            .iter()
            .filter(|release| &release.id == id)
        {
            if check_api_version(release.api_version).is_err() {
                continue;
            }
            let Some(artifact) = release.artifact_for(target) else {
                continue;
            };
            if !is_https(&artifact.url) || artifact.size > MAX_PACKAGE_BYTES {
                continue;
            }
            if best
                .as_ref()
                .is_some_and(|current| current.release.version >= release.version)
            {
                continue;
            }
            best = Some(PackageOffer {
                registry: cached.registry.clone(),
                release: release.clone(),
                artifact: artifact.clone(),
            });
        }
    }
    best
}
