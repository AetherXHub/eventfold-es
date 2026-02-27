//! Directory layout helpers for local file-based storage.
//!
//! After the gRPC migration, the on-disk layout only manages local
//! caches: snapshots, projection checkpoints, and process manager
//! checkpoints. All event data lives in the remote `eventfold-db` server.
//!
//! ```text
//! <base_dir>/
//!     snapshots/
//!     projections/
//!     process_managers/
//! ```

use std::path::{Path, PathBuf};

/// Manages the on-disk directory layout for local caches.
///
/// After the gRPC migration, only snapshot, projection checkpoint,
/// and process manager checkpoint directories are managed locally.
/// Event streams are stored in the remote `eventfold-db` server.
///
/// `StreamLayout` is cheap to clone (it wraps a single `PathBuf`).
#[derive(Debug, Clone)]
#[allow(dead_code)] // Utility type for directory layout; not yet wired into store.
pub(crate) struct StreamLayout {
    base_dir: PathBuf,
}

#[allow(dead_code)]
impl StreamLayout {
    /// Create a new `StreamLayout` rooted at the given base directory.
    ///
    /// # Arguments
    ///
    /// * `base_dir` - Root directory for local cache data (snapshots,
    ///   projection checkpoints, process manager checkpoints).
    pub(crate) fn new(base_dir: impl Into<PathBuf>) -> Self {
        Self {
            base_dir: base_dir.into(),
        }
    }

    /// Returns the root directory of this layout.
    pub(crate) fn base_dir(&self) -> &Path {
        &self.base_dir
    }

    /// Returns the path to the snapshots directory.
    ///
    /// # Returns
    ///
    /// `<base_dir>/snapshots`
    pub(crate) fn snapshots_dir(&self) -> PathBuf {
        self.base_dir.join("snapshots")
    }

    /// Returns the path to the projections directory.
    ///
    /// # Returns
    ///
    /// `<base_dir>/projections`
    pub(crate) fn projections_dir(&self) -> PathBuf {
        self.base_dir.join("projections")
    }

    /// Returns the path to the process managers directory.
    ///
    /// # Returns
    ///
    /// `<base_dir>/process_managers`
    pub(crate) fn process_managers_dir(&self) -> PathBuf {
        self.base_dir.join("process_managers")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn base_dir_returns_root() {
        let tmp = TempDir::new().expect("failed to create temp dir");
        let layout = StreamLayout::new(tmp.path());
        assert_eq!(layout.base_dir(), tmp.path());
    }

    #[test]
    fn snapshots_dir_returns_base_dir_slash_snapshots() {
        let tmp = TempDir::new().expect("failed to create temp dir");
        let layout = StreamLayout::new(tmp.path());
        assert_eq!(layout.snapshots_dir(), tmp.path().join("snapshots"));
    }

    #[test]
    fn projections_dir_returns_base_dir_slash_projections() {
        let tmp = TempDir::new().expect("failed to create temp dir");
        let layout = StreamLayout::new(tmp.path());
        assert_eq!(layout.projections_dir(), tmp.path().join("projections"));
    }

    #[test]
    fn process_managers_dir_returns_base_dir_slash_process_managers() {
        let tmp = TempDir::new().expect("failed to create temp dir");
        let layout = StreamLayout::new(tmp.path());
        assert_eq!(
            layout.process_managers_dir(),
            tmp.path().join("process_managers")
        );
    }
}
