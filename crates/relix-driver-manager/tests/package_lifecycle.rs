/*
 * @author t0bx
 * @since 23.10.2026
 */

#![allow(clippy::unwrap_used)]

mod fixtures;

use fixtures::{FakeBuiltIn, PackageSpec, TARGET, TestRegistry, leftovers};
use relix_core::DriverId;
use relix_driver_api::{DRIVER_API_VERSION, DriverOrigin};
use relix_driver_manager::{
    DriverManager, DriverStatus, INSTALLED_FILE, InstallProgress, InstalledDriver, ManagerError,
    PackageOffer, RegistrySource,
};
use semver::Version;
use std::sync::{Arc, Mutex};

fn csv() -> DriverId {
    DriverId::new("csv")
}

async fn install_offer(
    manager: &DriverManager,
    offer: &PackageOffer,
) -> Result<InstalledDriver, ManagerError> {
    manager.install(offer, &|_| {}).await
}

#[tokio::test]
async fn install_update_and_remove_lifecycle() {
    let drivers = tempfile::tempdir().unwrap();
    let mut registry = TestRegistry::new();
    registry.publish(&PackageSpec::new("csv", "1.0.0"), TARGET);
    let manager = registry.manager(drivers.path(), false);

    assert!(manager.driver(&csv()).is_none());
    assert_eq!(manager.refresh_index().await.unwrap().releases, 1);
    let offer = manager.offer_for(&csv()).unwrap();
    assert_eq!(offer.version(), &Version::new(1, 0, 0));
    assert_eq!(offer.registry, "test");

    let events = Mutex::new(Vec::new());
    let installed = manager
        .install(&offer, &|event| events.lock().unwrap().push(event))
        .await
        .unwrap();
    let events = events.into_inner().unwrap();
    assert_eq!(
        events.first(),
        Some(&InstallProgress::Downloading {
            received: 0,
            total: offer.size()
        })
    );
    assert!(events.contains(&InstallProgress::Verifying));
    assert_eq!(events.last(), Some(&InstallProgress::Installing));
    assert!(installed.approved);
    let executable = installed.executable_path(drivers.path());
    assert!(executable.is_file());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&executable).unwrap().permissions().mode();
        assert_eq!(mode & 0o111, 0o111);
    }

    let driver = manager.driver(&csv()).unwrap();
    assert!(matches!(
        driver.metadata().origin,
        DriverOrigin::External { .. }
    ));
    let entry = manager
        .entries()
        .into_iter()
        .find(|entry| entry.metadata.id == csv())
        .unwrap();
    assert_eq!(entry.status, DriverStatus::Ready);
    assert!(manager.outdated().is_empty());

    registry.publish(&PackageSpec::new("csv", "1.1.0"), TARGET);
    manager.refresh_index().await.unwrap();
    let outdated = manager.outdated();
    assert_eq!(outdated.len(), 1);
    assert_eq!(outdated[0].0.version, Version::new(1, 0, 0));
    assert_eq!(outdated[0].1.version(), &Version::new(1, 1, 0));
    let entry = manager
        .entries()
        .into_iter()
        .find(|entry| entry.metadata.id == csv())
        .unwrap();
    assert_eq!(
        entry.status,
        DriverStatus::UpdateAvailable(Version::new(1, 1, 0))
    );

    let updated = install_offer(&manager, &outdated[0].1).await.unwrap();
    assert_eq!(updated.version, Version::new(1, 1, 0));
    assert!(!installed.install_dir(drivers.path()).exists());
    assert!(updated.install_dir(drivers.path()).exists());
    assert!(manager.outdated().is_empty());
    assert_eq!(manager.driver(&csv()).unwrap().metadata().version, "1.1.0");
    assert!(leftovers(drivers.path()).is_empty());

    let restarted = registry.manager(drivers.path(), true);
    assert!(restarted.load_installed().is_empty());
    assert_eq!(restarted.installed(), vec![updated.clone()]);
    assert!(restarted.driver(&csv()).is_some());

    restarted.remove(&csv()).unwrap();
    assert!(restarted.driver(&csv()).is_none());
    assert!(!drivers.path().join("csv").exists());
    assert!(restarted.installed().is_empty());
    let recorded = std::fs::read_to_string(drivers.path().join(INSTALLED_FILE)).unwrap();
    assert!(!recorded.contains("csv"));
    assert!(matches!(
        restarted.remove(&csv()),
        Err(ManagerError::NotInstalled(_))
    ));
}

#[tokio::test]
async fn failed_update_keeps_previous_installation() {
    let drivers = tempfile::tempdir().unwrap();
    let mut registry = TestRegistry::new();
    registry.publish(&PackageSpec::new("csv", "1.0.0"), TARGET);
    let manager = registry.manager(drivers.path(), false);
    manager.refresh_index().await.unwrap();
    let previous = install_offer(&manager, &manager.offer_for(&csv()).unwrap())
        .await
        .unwrap();
    let recorded = std::fs::read(drivers.path().join(INSTALLED_FILE)).unwrap();

    let mut broken = PackageSpec::new("csv", "2.0.0");
    broken.include_executable = false;
    registry.publish(&broken, TARGET);
    let mut mislabelled = PackageSpec::new("csv", "3.0.0");
    mislabelled.manifest_version = Some("9.9.9");
    registry.publish(&mislabelled, TARGET);
    manager.refresh_index().await.unwrap();

    let index = manager.outdated();
    assert_eq!(index[0].1.version(), &Version::new(3, 0, 0));
    let error = install_offer(&manager, &index[0].1).await.unwrap_err();
    assert!(matches!(error, ManagerError::InvalidPackage(_)), "{error}");

    let error = install_offer(&manager, &registry.offer("csv", "2.0.0"))
        .await
        .unwrap_err();
    assert!(matches!(error, ManagerError::InvalidPackage(_)), "{error}");

    assert_eq!(manager.installed(), vec![previous.clone()]);
    assert_eq!(manager.driver(&csv()).unwrap().metadata().version, "1.0.0");
    assert!(previous.executable_path(drivers.path()).is_file());
    assert_eq!(
        std::fs::read(drivers.path().join(INSTALLED_FILE)).unwrap(),
        recorded
    );
    assert!(leftovers(drivers.path()).is_empty());
    let versions: Vec<_> = std::fs::read_dir(drivers.path().join("csv"))
        .unwrap()
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(versions, ["1.0.0"]);
}

#[tokio::test]
async fn offers_require_matching_target_and_api_version() {
    let drivers = tempfile::tempdir().unwrap();
    let mut registry = TestRegistry::new();
    registry.publish(&PackageSpec::new("csv", "1.0.0"), "windows-x86_64");
    let mut future_api = PackageSpec::new("csv", "5.0.0");
    future_api.api_version = DRIVER_API_VERSION + 1;
    registry.publish(&future_api, TARGET);
    let manager = registry.manager(drivers.path(), false);
    manager.refresh_index().await.unwrap();
    assert!(manager.offer_for(&csv()).is_none());

    let windows = registry
        .manager(drivers.path(), false)
        .with_target("windows-x86_64");
    windows.refresh_index().await.unwrap();
    assert_eq!(
        windows.offer_for(&csv()).unwrap().version(),
        &Version::new(1, 0, 0)
    );

    let error = install_offer(&manager, &registry.offer("csv", "1.0.0"))
        .await
        .unwrap_err();
    assert!(matches!(error, ManagerError::Incompatible(_)), "{error}");
    let error = install_offer(&manager, &registry.offer("csv", "5.0.0"))
        .await
        .unwrap_err();
    assert!(matches!(error, ManagerError::Incompatible(_)), "{error}");
    assert!(manager.installed().is_empty());
}

#[tokio::test]
async fn tampered_packages_are_rejected() {
    let drivers = tempfile::tempdir().unwrap();
    let mut registry = TestRegistry::new();
    let url = registry.publish(&PackageSpec::new("csv", "1.0.0"), TARGET);
    let manager = registry.manager(drivers.path(), false);
    manager.refresh_index().await.unwrap();
    let offer = manager.offer_for(&csv()).unwrap();
    let genuine = PackageSpec::new("csv", "1.0.0").archive();

    let mut flipped = genuine.clone();
    let last = flipped.len() - 1;
    flipped[last] ^= 0xff;
    registry.fetcher.put(&url, flipped);
    let error = install_offer(&manager, &offer).await.unwrap_err();
    assert!(
        matches!(error, ManagerError::ChecksumMismatch { .. }),
        "{error}"
    );

    let mut longer = genuine.clone();
    longer.push(0);
    registry.fetcher.put(&url, longer);
    let error = install_offer(&manager, &offer).await.unwrap_err();
    assert!(matches!(error, ManagerError::TooLarge { .. }), "{error}");

    registry
        .fetcher
        .put(&url, genuine[..genuine.len() - 1].to_vec());
    let error = install_offer(&manager, &offer).await.unwrap_err();
    assert!(
        matches!(error, ManagerError::SizeMismatch { .. }),
        "{error}"
    );

    registry.fetcher.put(&url, genuine);
    let mut forged = offer.clone();
    forged.release.description = "still fine".into();
    install_offer(&manager, &forged).await.unwrap();
    manager.remove(&csv()).unwrap();

    let other_key = fixtures::foreign_public_key();
    let untrusting = DriverManager::new(
        drivers.path(),
        vec![RegistrySource::new("test", fixtures::INDEX_URL, other_key)],
        false,
        Arc::clone(&registry.fetcher) as Arc<dyn relix_driver_manager::PackageFetcher>,
    )
    .with_target(TARGET);
    let error = install_offer(&untrusting, &offer).await.unwrap_err();
    assert!(
        matches!(error, ManagerError::InvalidSignature(_)),
        "{error}"
    );

    let mut relabelled = offer.clone();
    relabelled.release.version = Version::new(1, 0, 1);
    let error = install_offer(&manager, &relabelled).await.unwrap_err();
    assert!(
        matches!(error, ManagerError::InvalidSignature(_)),
        "{error}"
    );

    assert!(manager.installed().is_empty());
    assert!(leftovers(drivers.path()).is_empty());
}

#[tokio::test]
async fn insecure_urls_are_refused() {
    let drivers = tempfile::tempdir().unwrap();
    let mut registry = TestRegistry::new();
    registry.publish_at(
        &PackageSpec::new("csv", "1.0.0"),
        TARGET,
        "http://registry.test/csv.tar.gz",
    );
    let manager = registry.manager(drivers.path(), false);
    manager.refresh_index().await.unwrap();
    assert!(manager.offer_for(&csv()).is_none());
    let error = install_offer(&manager, &registry.offer("csv", "1.0.0"))
        .await
        .unwrap_err();
    assert!(matches!(error, ManagerError::InsecureUrl(_)), "{error}");

    let plain = DriverManager::new(
        drivers.path(),
        vec![RegistrySource::new(
            "plain",
            "http://registry.test/index.json",
            registry.public_key(),
        )],
        false,
        Arc::clone(&registry.fetcher) as Arc<dyn relix_driver_manager::PackageFetcher>,
    );
    let error = plain.refresh_index().await.unwrap_err();
    assert!(error.to_string().contains("insecure URL"), "{error}");
}

#[tokio::test]
async fn offline_mode_blocks_downloads() {
    let drivers = tempfile::tempdir().unwrap();
    let mut registry = TestRegistry::new();
    registry.publish(&PackageSpec::new("csv", "1.0.0"), TARGET);
    let manager = registry.manager(drivers.path(), true);
    assert!(matches!(
        manager.refresh_index().await,
        Err(ManagerError::Offline)
    ));
    assert!(manager.offer_for(&csv()).is_none());
    let error = install_offer(&manager, &registry.offer("csv", "1.0.0"))
        .await
        .unwrap_err();
    assert!(matches!(error, ManagerError::Offline));

    manager.set_offline(false);
    manager.refresh_index().await.unwrap();
    let offer = manager.offer_for(&csv()).unwrap();
    manager.set_offline(true);
    assert!(manager.offer_for(&csv()).is_some());
    assert!(matches!(
        install_offer(&manager, &offer).await,
        Err(ManagerError::Offline)
    ));
}

#[tokio::test]
async fn built_in_drivers_cannot_be_replaced_or_removed() {
    let drivers = tempfile::tempdir().unwrap();
    let mut registry = TestRegistry::new();
    registry.publish(&PackageSpec::new("postgres", "9.0.0"), TARGET);
    let manager = registry.manager(drivers.path(), false);
    manager.register_built_in(Arc::new(FakeBuiltIn("postgres")));
    manager.refresh_index().await.unwrap();

    let postgres = DriverId::new("postgres");
    assert!(manager.offer_for(&postgres).is_none());
    let error = install_offer(&manager, &registry.offer("postgres", "9.0.0"))
        .await
        .unwrap_err();
    assert!(matches!(error, ManagerError::BuiltIn(_)));
    assert!(matches!(
        manager.remove(&postgres),
        Err(ManagerError::BuiltIn(_))
    ));
    let entries = manager.entries();
    assert_eq!(entries.len(), 1);
    assert!(entries[0].is_built_in());
    assert_eq!(entries[0].status, DriverStatus::Ready);
    assert_eq!(entries[0].metadata.origin, DriverOrigin::BuiltIn);
}

#[tokio::test]
async fn unusable_installations_are_reported_not_registered() {
    let drivers = tempfile::tempdir().unwrap();
    let mut registry = TestRegistry::new();
    registry.publish(&PackageSpec::new("csv", "1.0.0"), TARGET);
    registry.publish(&PackageSpec::new("parquet", "1.0.0"), TARGET);
    let manager = registry.manager(drivers.path(), false);
    manager.refresh_index().await.unwrap();
    install_offer(&manager, &manager.offer_for(&csv()).unwrap())
        .await
        .unwrap();
    install_offer(
        &manager,
        &manager.offer_for(&DriverId::new("parquet")).unwrap(),
    )
    .await
    .unwrap();

    let path = drivers.path().join(INSTALLED_FILE);
    let edited =
        std::fs::read_to_string(&path)
            .unwrap()
            .replacen("approved = true", "approved = false", 1);
    std::fs::write(&path, edited).unwrap();
    std::fs::create_dir_all(drivers.path().join(".staging-csv-crashed")).unwrap();

    let restarted = registry.manager(drivers.path(), true);
    let warnings = restarted.load_installed();
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(warnings[0].contains("approved"));
    assert!(restarted.driver(&csv()).is_none());
    assert!(restarted.driver(&DriverId::new("parquet")).is_some());
    let entry = restarted
        .entries()
        .into_iter()
        .find(|entry| entry.metadata.id == csv())
        .unwrap();
    assert!(matches!(entry.status, DriverStatus::Incompatible(_)));
    assert!(leftovers(drivers.path()).is_empty());

    let elsewhere = registry
        .manager(drivers.path(), true)
        .with_target("macos-aarch64");
    let warnings = elsewhere.load_installed();
    assert_eq!(warnings.len(), 2, "{warnings:?}");
    assert!(elsewhere.driver(&DriverId::new("parquet")).is_none());
}
