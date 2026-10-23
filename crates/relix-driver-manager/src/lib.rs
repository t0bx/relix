/*
 * @author t0bx
 * @since 23.10.2026
 */

mod archive;
mod compatibility;
mod entry;
mod error;
mod external;
mod fetcher;
mod https_fetcher;
mod index;
mod installed;
mod installer;
mod manager;
mod manifest;
mod offer;
mod registry;
mod verification;

pub use compatibility::current_target;
pub use entry::{DriverEntry, DriverStatus, IndexRefresh, InstallProgress};
pub use error::ManagerError;
pub use external::{ExternalConnection, ExternalDriver};
pub use fetcher::{DownloadProgress, PackageFetcher, ensure_https, is_https};
pub use https_fetcher::HttpsFetcher;
pub use index::{INDEX_FORMAT_VERSION, PackageArtifact, PackageIndex, PackageRelease};
pub use installed::{INSTALLED_FILE, InstalledDriver};
pub use manager::{DriverManager, ProgressCallback};
pub use manifest::{ConnectorManifest, MANIFEST_FILE};
pub use offer::PackageOffer;
pub use registry::RegistrySource;
pub use verification::{
    MAX_PACKAGE_BYTES, SIGNATURE_DOMAIN, parse_public_key, sha256_hex, signing_message,
};
