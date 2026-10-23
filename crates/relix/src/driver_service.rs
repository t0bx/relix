/*
 * @author t0bx
 * @since 23.10.2026
 */

use async_trait::async_trait;
use relix_app::services::{DriverDescriptor, DriverService, InstallOffer, describe_driver};
use relix_core::DriverId;
use relix_driver_api::DatabaseDriver;
use relix_driver_manager::{DriverManager, DriverStatus, PackageOffer};
use std::sync::Arc;

pub struct ManagerDriverService {
    manager: Arc<DriverManager>,
}

impl ManagerDriverService {
    pub fn new(manager: Arc<DriverManager>) -> Self {
        Self { manager }
    }

    async fn offer(&self, id: &DriverId) -> Result<Option<PackageOffer>, String> {
        if self.manager.offer_for(id).is_none() {
            let refresh = self
                .manager
                .refresh_index()
                .await
                .map_err(|error| error.to_string())?;
            for failure in refresh.failures {
                tracing::warn!(%failure, "driver registry unavailable");
            }
        }
        Ok(self.manager.offer_for(id))
    }
}

/// Built-in engines in the order users expect them; external connectors follow.
fn display_rank(id: &DriverId) -> usize {
    [
        DriverId::POSTGRES,
        DriverId::MYSQL,
        DriverId::MARIADB,
        DriverId::SQLITE,
        DriverId::REDIS,
    ]
    .iter()
    .position(|known| *known == id.as_str())
    .unwrap_or(usize::MAX)
}

fn describe_offer(offer: &PackageOffer) -> InstallOffer {
    let host = offer
        .artifact
        .url
        .split("://")
        .nth(1)
        .and_then(|rest| rest.split('/').next())
        .unwrap_or_default()
        .to_owned();
    InstallOffer {
        id: offer.id().clone(),
        name: offer.release.name.clone(),
        version: offer.version().to_string(),
        source: format!("{} ({host})", offer.registry),
        size_bytes: offer.size(),
        description: offer.release.description.clone(),
    }
}

#[async_trait]
impl DriverService for ManagerDriverService {
    fn driver(&self, id: &DriverId) -> Option<Arc<dyn DatabaseDriver>> {
        self.manager.driver(id)
    }

    fn descriptors(&self) -> Vec<DriverDescriptor> {
        let mut descriptors: Vec<DriverDescriptor> = self
            .manager
            .entries()
            .into_iter()
            .filter_map(|entry| {
                let status = match &entry.status {
                    DriverStatus::Ready => "Ready".to_owned(),
                    DriverStatus::Incompatible(reason) => format!("Incompatible: {reason}"),
                    DriverStatus::UpdateAvailable(version) => format!("Update {version} available"),
                };
                let driver = self.manager.driver(&entry.metadata.id)?;
                let mut descriptor = describe_driver(driver.as_ref(), &status);
                if let Some(installed) = &entry.installed {
                    descriptor.origin = format!("external ({})", installed.package_label());
                }
                Some(descriptor)
            })
            .collect();
        descriptors
            .sort_by_key(|descriptor| (display_rank(&descriptor.id), descriptor.name.clone()));
        descriptors
    }

    async fn find_offer(&self, id: &DriverId) -> Result<Option<InstallOffer>, String> {
        Ok(self.offer(id).await?.as_ref().map(describe_offer))
    }

    async fn install(&self, id: &DriverId) -> Result<String, String> {
        let offer = self.offer(id).await?.ok_or_else(|| {
            format!("no compatible package for '{id}' in the configured registries")
        })?;
        let installed = self
            .manager
            .install(&offer, &|progress| {
                tracing::debug!(?progress, "driver install")
            })
            .await
            .map_err(|error| error.to_string())?;
        Ok(installed.version.to_string())
    }

    async fn updates(&self) -> Result<Vec<InstallOffer>, String> {
        let refresh = self
            .manager
            .refresh_index()
            .await
            .map_err(|error| error.to_string())?;
        for failure in refresh.failures {
            tracing::warn!(%failure, "driver registry unavailable");
        }
        Ok(self
            .manager
            .outdated()
            .iter()
            .map(|(_, offer)| describe_offer(offer))
            .collect())
    }

    fn remove(&self, id: &DriverId) -> Result<(), String> {
        self.manager.remove(id).map_err(|error| error.to_string())
    }
}
