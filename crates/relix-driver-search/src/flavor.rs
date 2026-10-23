/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::cluster::RootInfo;
use relix_core::DriverId;
use relix_driver_api::ServerInfo;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Flavor {
    Elasticsearch,
    OpenSearch,
}

impl Flavor {
    pub fn driver_id(self) -> &'static str {
        match self {
            Self::Elasticsearch => DriverId::ELASTICSEARCH,
            Self::OpenSearch => DriverId::OPENSEARCH,
        }
    }

    pub fn product_name(self) -> &'static str {
        match self {
            Self::Elasticsearch => "Elasticsearch",
            Self::OpenSearch => "OpenSearch",
        }
    }

    pub fn detect(root: &RootInfo) -> Self {
        let distribution = root.version.distribution.as_deref().unwrap_or_default();
        let tagline = root.tagline.as_deref().unwrap_or_default();
        if distribution.eq_ignore_ascii_case("opensearch")
            || tagline.to_ascii_lowercase().contains("opensearch")
        {
            Self::OpenSearch
        } else {
            Self::Elasticsearch
        }
    }

    pub fn server_info(self, root: &RootInfo) -> ServerInfo {
        let server = Self::detect(root);
        let product = if server == self {
            server.product_name().to_owned()
        } else {
            format!(
                "{} (connected via {} driver)",
                server.product_name(),
                self.product_name()
            )
        };
        let version = match root.version.build_flavor.as_deref() {
            Some(build) if build != "default" && server == Self::Elasticsearch => {
                format!("{} ({build})", root.version.number)
            }
            _ => root.version.number.clone(),
        };
        ServerInfo { product, version }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root(body: &str) -> RootInfo {
        serde_json::from_str(body).unwrap()
    }

    fn elasticsearch() -> RootInfo {
        root(
            r#"{"name":"n1","cluster_name":"docker-cluster","version":{"number":"8.15.0",
            "build_flavor":"default","lucene_version":"9.11.1"},"tagline":"You Know, for Search"}"#,
        )
    }

    fn opensearch() -> RootInfo {
        root(
            r#"{"name":"n1","cluster_name":"docker-cluster","version":{"distribution":"opensearch",
            "number":"2.17.1"},"tagline":"The OpenSearch Project: https://opensearch.org/"}"#,
        )
    }

    #[test]
    fn detects_the_product_from_the_root_endpoint() {
        assert_eq!(Flavor::detect(&elasticsearch()), Flavor::Elasticsearch);
        assert_eq!(Flavor::detect(&opensearch()), Flavor::OpenSearch);
        let compatibility_mode = root(
            r#"{"cluster_name":"c","version":{"number":"7.10.2"},
            "tagline":"The OpenSearch Project: https://opensearch.org/"}"#,
        );
        assert_eq!(Flavor::detect(&compatibility_mode), Flavor::OpenSearch);
    }

    #[test]
    fn matching_flavor_reports_the_plain_product() {
        let info = Flavor::Elasticsearch.server_info(&elasticsearch());
        assert_eq!(info.product, "Elasticsearch");
        assert_eq!(info.version, "8.15.0");
        assert_eq!(
            Flavor::OpenSearch.server_info(&opensearch()).product,
            "OpenSearch"
        );
    }

    #[test]
    fn mismatched_flavor_is_reported() {
        assert_eq!(
            Flavor::Elasticsearch.server_info(&opensearch()).product,
            "OpenSearch (connected via Elasticsearch driver)"
        );
        assert_eq!(
            Flavor::OpenSearch.server_info(&elasticsearch()).product,
            "Elasticsearch (connected via OpenSearch driver)"
        );
    }

    #[test]
    fn serverless_builds_are_marked_in_the_version() {
        let serverless = root(
            r#"{"cluster_name":"c","version":{"number":"8.11.0","build_flavor":"serverless"}}"#,
        );
        assert_eq!(
            Flavor::Elasticsearch.server_info(&serverless).version,
            "8.11.0 (serverless)"
        );
    }
}
