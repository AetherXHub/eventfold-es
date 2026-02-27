//! Local file-based snapshot persistence for aggregate state.
//!
//! Snapshots are stored as JSON files at
//! `<base_dir>/snapshots/<aggregate_type>/<instance_id>/snapshot.json`.
//! Writes are atomic via a temp-rename pattern to prevent corruption
//! from crashes mid-write.

use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::aggregate::Aggregate;

/// A point-in-time snapshot of an aggregate's state and stream version.
///
/// Used to avoid replaying the full event history when an aggregate is
/// loaded. The `stream_version` records how many events have been folded
/// into the `state`, so catch-up can resume from `stream_version + 1`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(bound(serialize = "A: Serialize", deserialize = "A: DeserializeOwned"))]
pub struct Snapshot<A> {
    /// The aggregate state at the time of the snapshot.
    pub state: A,
    /// The stream version (number of events applied) at snapshot time.
    pub stream_version: u64,
}

/// Compute the filesystem path for a snapshot file.
///
/// # Arguments
///
/// * `base_dir` - Root directory for local storage (snapshots, checkpoints).
/// * `aggregate_type` - The aggregate type identifier (e.g. `"counter"`).
/// * `instance_id` - The aggregate instance identifier (e.g. `"c-1"`).
///
/// # Returns
///
/// `<base_dir>/snapshots/<aggregate_type>/<instance_id>/snapshot.json`
pub fn snapshot_path(base_dir: &Path, aggregate_type: &str, instance_id: &str) -> PathBuf {
    base_dir
        .join("snapshots")
        .join(aggregate_type)
        .join(instance_id)
        .join("snapshot.json")
}

/// Save an aggregate snapshot atomically to disk.
///
/// Writes to a temporary file (`snapshot.json.tmp`) in the same directory,
/// then renames it to `snapshot.json`. This ensures readers never see a
/// partially-written file.
///
/// # Arguments
///
/// * `base_dir` - Root directory for local storage.
/// * `instance_id` - The aggregate instance identifier.
/// * `snapshot` - The snapshot to persist.
///
/// # Errors
///
/// Returns `io::Error` if directory creation, file writing, or renaming fails.
pub fn save_snapshot<A: Aggregate>(
    base_dir: &Path,
    instance_id: &str,
    snapshot: &Snapshot<A>,
) -> io::Result<()> {
    let path = snapshot_path(base_dir, A::AGGREGATE_TYPE, instance_id);
    let dir = path
        .parent()
        .expect("snapshot_path always has a parent directory");
    std::fs::create_dir_all(dir)?;

    let tmp_path = path.with_extension("json.tmp");
    let json = serde_json::to_vec_pretty(snapshot)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    std::fs::write(&tmp_path, &json)?;
    std::fs::rename(&tmp_path, &path)?;
    Ok(())
}

/// Load an aggregate snapshot from disk.
///
/// # Arguments
///
/// * `base_dir` - Root directory for local storage.
/// * `instance_id` - The aggregate instance identifier.
///
/// # Returns
///
/// - `Ok(Some(snapshot))` if the file exists and deserializes successfully.
/// - `Ok(None)` if the file does not exist or contains invalid JSON.
///   Deserialization failures are logged as warnings via `tracing::warn!`.
///
/// # Errors
///
/// Returns `io::Error` only for unexpected I/O failures (e.g. permission denied).
pub fn load_snapshot<A: Aggregate>(
    base_dir: &Path,
    instance_id: &str,
) -> io::Result<Option<Snapshot<A>>> {
    let path = snapshot_path(base_dir, A::AGGREGATE_TYPE, instance_id);
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };

    match serde_json::from_slice::<Snapshot<A>>(&bytes) {
        Ok(snap) => Ok(Some(snap)),
        Err(e) => {
            tracing::warn!(
                path = %path.display(),
                error = %e,
                "failed to deserialize snapshot; treating as cache miss"
            );
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aggregate::test_fixtures::Counter;
    use std::path::Path;

    #[test]
    fn snapshot_path_returns_expected_path() {
        let base = Path::new("/data/myapp");
        let path = snapshot_path(base, "counter", "c-1");
        assert_eq!(
            path,
            PathBuf::from("/data/myapp/snapshots/counter/c-1/snapshot.json")
        );
    }

    #[test]
    fn save_then_load_roundtrips() {
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let snap = Snapshot {
            state: Counter { value: 42 },
            stream_version: 7,
        };

        save_snapshot::<Counter>(dir.path(), "c-1", &snap).expect("save should succeed");

        let loaded = load_snapshot::<Counter>(dir.path(), "c-1").expect("load should succeed");

        let loaded = loaded.expect("snapshot should exist");
        assert_eq!(loaded.state.value, 42);
        assert_eq!(loaded.stream_version, 7);
    }

    #[test]
    fn load_nonexistent_returns_none() {
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let result =
            load_snapshot::<Counter>(dir.path(), "no-such-id").expect("load should succeed");
        assert!(result.is_none());
    }

    #[test]
    fn load_corrupt_json_returns_none() {
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let path = snapshot_path(dir.path(), "counter", "c-bad");
        std::fs::create_dir_all(path.parent().unwrap()).expect("create dir");
        std::fs::write(&path, b"this is not valid json!!!").expect("write corrupt file");

        let result =
            load_snapshot::<Counter>(dir.path(), "c-bad").expect("load should succeed (not Err)");
        assert!(
            result.is_none(),
            "corrupt JSON should return Ok(None), not Ok(Some(...))"
        );
    }

    #[test]
    fn save_uses_atomic_temp_rename() {
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let snap = Snapshot {
            state: Counter { value: 10 },
            stream_version: 3,
        };

        save_snapshot::<Counter>(dir.path(), "c-atomic", &snap).expect("save should succeed");

        let final_path = snapshot_path(dir.path(), "counter", "c-atomic");
        let tmp_path = final_path.with_extension("json.tmp");

        assert!(final_path.exists(), "final snapshot file should exist");
        assert!(
            !tmp_path.exists(),
            "temp file should not exist after successful save"
        );
    }
}
