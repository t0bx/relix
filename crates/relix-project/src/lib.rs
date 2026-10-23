/*
 * @author t0bx
 * @since 23.10.2026
 */

pub mod error;
pub mod jsonl;
pub mod model;
mod platform_store;
pub mod portable;
pub mod recent;
pub mod secrets;
pub mod session;
pub mod store;

pub use error::ProjectError;
pub use model::{Project, ProjectSettings};
pub use recent::RecentProjects;
pub use secrets::{FallbackSecretStore, MemorySecretStore, SecretKey, SecretStore};
pub use session::{
    EditorSnapshot, KeyValueSnapshot, LayoutSnapshot, PaneSnapshot, SessionState, SplitDirection,
    TabSnapshot, TableSnapshot,
};
pub use store::ProjectStore;
