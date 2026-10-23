/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::error::ManagerError;
use async_trait::async_trait;

pub type DownloadProgress<'a> = &'a (dyn Fn(u64) + Send + Sync);

/// Transport used to fetch registry indexes and package artifacts. Implementations must
/// stop reading and fail with [`ManagerError::TooLarge`] once `max_bytes` is exceeded and
/// report the number of bytes received so far through `progress`.
#[async_trait]
pub trait PackageFetcher: Send + Sync {
    async fn fetch_index(&self, url: &str, max_bytes: u64) -> Result<Vec<u8>, ManagerError>;

    async fn download(
        &self,
        url: &str,
        max_bytes: u64,
        progress: DownloadProgress<'_>,
    ) -> Result<Vec<u8>, ManagerError>;
}

pub fn is_https(url: &str) -> bool {
    url.get(..8)
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case("https://"))
        && url.len() > 8
}

pub fn ensure_https(url: &str) -> Result<(), ManagerError> {
    if is_https(url) {
        Ok(())
    } else {
        Err(ManagerError::InsecureUrl(url.to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_https_urls_are_accepted() {
        assert!(ensure_https("https://drivers.example.org/index.json").is_ok());
        assert!(ensure_https("HTTPS://drivers.example.org/").is_ok());
        assert!(ensure_https("http://drivers.example.org/index.json").is_err());
        assert!(ensure_https("file:///etc/passwd").is_err());
        assert!(ensure_https("https://").is_err());
    }
}
