/*
 * @author t0bx
 * @since 23.10.2026
 */

use async_trait::async_trait;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use ed25519_dalek::{Signer, SigningKey};
use flate2::Compression;
use flate2::write::GzEncoder;
use relix_core::{ConnectionConfig, DriverId};
use relix_driver_api::{
    ConnectionTarget, DatabaseConnection, DatabaseDriver, DriverCapabilities, DriverError,
    DriverMetadata,
};
use relix_driver_manager::{
    DownloadProgress, DriverManager, INDEX_FORMAT_VERSION, ManagerError, PackageArtifact,
    PackageFetcher, PackageIndex, PackageOffer, PackageRelease, RegistrySource, sha256_hex,
    signing_message,
};
use semver::Version;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

pub const TARGET: &str = "linux-x86_64";
pub const INDEX_URL: &str = "https://registry.test/index.json";

#[derive(Default)]
pub struct MemoryFetcher {
    files: Mutex<HashMap<String, Vec<u8>>>,
}

impl MemoryFetcher {
    pub fn put(&self, url: &str, bytes: Vec<u8>) {
        self.files.lock().unwrap().insert(url.to_owned(), bytes);
    }

    fn get(&self, url: &str, max_bytes: u64) -> Result<Vec<u8>, ManagerError> {
        let bytes = self
            .files
            .lock()
            .unwrap()
            .get(url)
            .cloned()
            .ok_or_else(|| ManagerError::Download(format!("404 for {url}")))?;
        if bytes.len() as u64 > max_bytes {
            return Err(ManagerError::TooLarge { limit: max_bytes });
        }
        Ok(bytes)
    }
}

#[async_trait]
impl PackageFetcher for MemoryFetcher {
    async fn fetch_index(&self, url: &str, max_bytes: u64) -> Result<Vec<u8>, ManagerError> {
        self.get(url, max_bytes)
    }

    async fn download(
        &self,
        url: &str,
        max_bytes: u64,
        progress: DownloadProgress<'_>,
    ) -> Result<Vec<u8>, ManagerError> {
        let bytes = self.get(url, max_bytes)?;
        progress(bytes.len() as u64);
        Ok(bytes)
    }
}

pub struct PackageSpec {
    pub id: &'static str,
    pub version: &'static str,
    pub api_version: u32,
    pub manifest_version: Option<&'static str>,
    pub include_executable: bool,
}

impl PackageSpec {
    pub fn new(id: &'static str, version: &'static str) -> Self {
        Self {
            id,
            version,
            api_version: 1,
            manifest_version: None,
            include_executable: true,
        }
    }

    pub fn archive(&self) -> Vec<u8> {
        let manifest = format!(
            "id = \"{}\"\nname = \"{} connector\"\nversion = \"{}\"\napi_version = {}\n\
             executable = \"bin/connector\"\ndialect = \"sqlite\"\n\n[target]\nkind = \"file\"\n",
            self.id,
            self.id,
            self.manifest_version.unwrap_or(self.version),
            self.api_version
        );
        let mut builder = tar::Builder::new(GzEncoder::new(Vec::new(), Compression::fast()));
        append(&mut builder, "connector.toml", manifest.as_bytes());
        if self.include_executable {
            append(&mut builder, "bin/connector", b"#!/bin/sh\nexit 0\n");
        }
        builder.into_inner().unwrap().finish().unwrap()
    }
}

fn append(builder: &mut tar::Builder<GzEncoder<Vec<u8>>>, path: &str, contents: &[u8]) {
    let mut header = tar::Header::new_gnu();
    header.set_size(contents.len() as u64);
    header.set_mode(0o644);
    builder.append_data(&mut header, path, contents).unwrap();
}

pub struct TestRegistry {
    pub key: SigningKey,
    pub fetcher: Arc<MemoryFetcher>,
    releases: Vec<PackageRelease>,
}

impl TestRegistry {
    pub fn new() -> Self {
        let registry = Self {
            key: SigningKey::from_bytes(&[42; 32]),
            fetcher: Arc::new(MemoryFetcher::default()),
            releases: Vec::new(),
        };
        registry.write_index();
        registry
    }

    pub fn public_key(&self) -> String {
        STANDARD.encode(self.key.verifying_key().to_bytes())
    }

    pub fn source(&self) -> RegistrySource {
        RegistrySource::new("test", INDEX_URL, self.public_key())
    }

    pub fn manager(&self, drivers_dir: &Path, offline: bool) -> DriverManager {
        DriverManager::new(
            drivers_dir,
            vec![self.source()],
            offline,
            Arc::clone(&self.fetcher) as Arc<dyn PackageFetcher>,
        )
        .with_target(TARGET)
    }

    pub fn publish(&mut self, spec: &PackageSpec, target: &str) -> String {
        let url = format!(
            "https://registry.test/{}-{}-{target}.tar.gz",
            spec.id, spec.version
        );
        self.publish_at(spec, target, &url)
    }

    pub fn publish_at(&mut self, spec: &PackageSpec, target: &str, url: &str) -> String {
        let bytes = spec.archive();
        let mut release = PackageRelease {
            id: DriverId::new(spec.id),
            name: format!("{} connector", spec.id),
            version: Version::parse(spec.version).unwrap(),
            api_version: spec.api_version,
            description: "test package".into(),
            artifacts: Vec::new(),
        };
        let mut artifact = PackageArtifact {
            target: target.into(),
            url: url.into(),
            size: bytes.len() as u64,
            sha256: sha256_hex(&bytes),
            signature: String::new(),
        };
        let signature = self
            .key
            .sign(signing_message(&release, &artifact).as_bytes());
        artifact.signature = STANDARD.encode(signature.to_bytes());
        release.artifacts.push(artifact);
        self.fetcher.put(url, bytes);
        self.releases.push(release);
        self.write_index();
        url.to_owned()
    }

    pub fn offer(&self, id: &str, version: &str) -> PackageOffer {
        let release = self
            .releases
            .iter()
            .find(|release| release.id.as_str() == id && release.version.to_string() == version)
            .unwrap()
            .clone();
        PackageOffer {
            registry: "test".into(),
            artifact: release.artifacts[0].clone(),
            release,
        }
    }

    fn write_index(&self) {
        let index = PackageIndex {
            format_version: INDEX_FORMAT_VERSION,
            releases: self.releases.clone(),
        };
        self.fetcher
            .put(INDEX_URL, serde_json::to_vec(&index).unwrap());
    }
}

pub fn foreign_public_key() -> String {
    STANDARD.encode(SigningKey::from_bytes(&[7; 32]).verifying_key().to_bytes())
}

pub struct FakeBuiltIn(pub &'static str);

#[async_trait]
impl DatabaseDriver for FakeBuiltIn {
    fn metadata(&self) -> DriverMetadata {
        DriverMetadata::built_in(self.0, self.0, None, ConnectionTarget::File)
    }

    fn capabilities(&self) -> DriverCapabilities {
        DriverCapabilities::relational()
    }

    async fn connect(
        &self,
        _config: &ConnectionConfig,
    ) -> Result<Box<dyn DatabaseConnection>, DriverError> {
        Err(DriverError::unsupported("fake driver"))
    }
}

pub fn leftovers(drivers_dir: &Path) -> Vec<String> {
    std::fs::read_dir(drivers_dir)
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .filter(|name| name.starts_with('.'))
                .collect()
        })
        .unwrap_or_default()
}
