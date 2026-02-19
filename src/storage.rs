//! Event storage trait and built-in backends.

use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Manages the on-disk directory layout for aggregate event streams.
///
/// The layout follows this structure:
/// ```text
/// <base_dir>/
///     streams/
///         <aggregate_type>/
///             <instance_id>/      -- standard eventfold log directory
///                 app.jsonl
///                 views/
///     projections/
///         <projection_name>/
///     meta/
///         streams.jsonl           -- stream registry
/// ```
///
/// `StreamLayout` is cheap to clone (it wraps a single `PathBuf`) and provides
/// path helpers plus stream lifecycle management (creation and listing).
#[derive(Debug, Clone)]
pub struct StreamLayout {
    base_dir: PathBuf,
}

impl StreamLayout {
    /// Create a new `StreamLayout` rooted at the given base directory.
    ///
    /// # Arguments
    ///
    /// * `base_dir` - Root directory for all event store data.
    ///   The directory does not need to exist yet; it will be created
    ///   lazily when [`ensure_stream`](StreamLayout::ensure_stream) is called.
    pub fn new(base_dir: impl Into<PathBuf>) -> Self {
        Self {
            base_dir: base_dir.into(),
        }
    }

    /// Returns the root directory of this layout.
    pub fn base_dir(&self) -> &Path {
        &self.base_dir
    }

    /// Returns the path to a specific aggregate instance's stream directory.
    ///
    /// # Arguments
    ///
    /// * `aggregate_type` - The aggregate type name (e.g. `"order"`).
    /// * `instance_id` - The unique instance identifier within that type.
    ///
    /// # Returns
    ///
    /// `<base_dir>/streams/<aggregate_type>/<instance_id>`
    pub fn stream_dir(&self, aggregate_type: &str, instance_id: &str) -> PathBuf {
        self.base_dir
            .join("streams")
            .join(aggregate_type)
            .join(instance_id)
    }

    /// Returns the path to a specific aggregate instance's views directory.
    ///
    /// # Arguments
    ///
    /// * `aggregate_type` - The aggregate type name (e.g. `"order"`).
    /// * `instance_id` - The unique instance identifier within that type.
    ///
    /// # Returns
    ///
    /// `<base_dir>/streams/<aggregate_type>/<instance_id>/views`
    pub fn views_dir(&self, aggregate_type: &str, instance_id: &str) -> PathBuf {
        self.stream_dir(aggregate_type, instance_id).join("views")
    }

    /// Returns the path to the projections directory.
    ///
    /// # Returns
    ///
    /// `<base_dir>/projections`
    pub fn projections_dir(&self) -> PathBuf {
        self.base_dir.join("projections")
    }

    /// Returns the path to the metadata directory.
    ///
    /// # Returns
    ///
    /// `<base_dir>/meta`
    pub fn meta_dir(&self) -> PathBuf {
        self.base_dir.join("meta")
    }

    /// Ensures that the stream directory and registry entry exist for the
    /// given aggregate type and instance ID.
    ///
    /// This method is **idempotent**: calling it multiple times with the same
    /// arguments will not create duplicate directory trees or registry entries.
    ///
    /// # Arguments
    ///
    /// * `aggregate_type` - The aggregate type name (e.g. `"order"`).
    /// * `instance_id` - The unique instance identifier within that type.
    ///
    /// # Returns
    ///
    /// The stream directory path on success.
    ///
    /// # Errors
    ///
    /// Returns `std::io::Error` if directory creation or file I/O fails.
    pub fn ensure_stream(
        &self,
        aggregate_type: &str,
        instance_id: &str,
    ) -> std::io::Result<PathBuf> {
        let dir = self.stream_dir(aggregate_type, instance_id);
        fs::create_dir_all(&dir)?;

        let meta = self.meta_dir();
        fs::create_dir_all(&meta)?;

        let registry_path = meta.join("streams.jsonl");

        // Check whether an entry already exists for this (type, id) pair.
        // If the registry file doesn't exist yet, we skip straight to appending.
        let already_registered = registry_path
            .exists()
            .then(|| -> std::io::Result<bool> {
                let file = fs::File::open(&registry_path)?;
                let reader = BufReader::new(file);
                for line in reader.lines() {
                    let line = line?;
                    if line.is_empty() {
                        continue;
                    }
                    // Parse the JSON to compare fields structurally rather
                    // than relying on string matching, which would be fragile
                    // if field ordering ever changed.
                    if let Ok(entry) = serde_json::from_str::<serde_json::Value>(&line)
                        && entry.get("type").and_then(|v| v.as_str()) == Some(aggregate_type)
                        && entry.get("id").and_then(|v| v.as_str()) == Some(instance_id)
                    {
                        return Ok(true);
                    }
                }
                Ok(false)
            })
            .transpose()?
            .unwrap_or(false);

        if !already_registered {
            let ts = SystemTime::UNIX_EPOCH
                .elapsed()
                .expect("system clock is before Unix epoch")
                .as_secs();

            let entry = serde_json::json!({
                "type": aggregate_type,
                "id": instance_id,
                "ts": ts,
            });

            let mut file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&registry_path)?;

            // Each entry is a single line of JSON followed by a newline.
            writeln!(file, "{entry}")?;
        }

        Ok(dir)
    }

    /// Lists all instance IDs for the given aggregate type.
    ///
    /// # Arguments
    ///
    /// * `aggregate_type` - The aggregate type name to list instances for.
    ///
    /// # Returns
    ///
    /// A sorted `Vec<String>` of instance IDs. Returns an empty vector
    /// if the aggregate type directory does not exist.
    ///
    /// # Errors
    ///
    /// Returns `std::io::Error` if reading the directory fails for a reason
    /// other than the directory not existing.
    pub fn list_streams(&self, aggregate_type: &str) -> std::io::Result<Vec<String>> {
        let type_dir = self.base_dir.join("streams").join(aggregate_type);

        let entries = match fs::read_dir(&type_dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e),
        };

        let mut ids: Vec<String> = entries
            .filter_map(|entry| {
                let entry = entry.ok()?;
                // Only include directories (each directory is a stream instance).
                entry
                    .file_type()
                    .ok()?
                    .is_dir()
                    .then(|| entry.file_name().to_string_lossy().into_owned())
            })
            .collect();

        ids.sort();
        Ok(ids)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn path_helpers_correct() {
        let tmp = TempDir::new().expect("failed to create temp dir");
        let layout = StreamLayout::new(tmp.path());

        assert_eq!(layout.base_dir(), tmp.path());

        assert_eq!(
            layout.stream_dir("order", "abc-123"),
            tmp.path().join("streams/order/abc-123")
        );

        assert_eq!(
            layout.views_dir("order", "abc-123"),
            tmp.path().join("streams/order/abc-123/views")
        );

        assert_eq!(layout.projections_dir(), tmp.path().join("projections"));

        assert_eq!(layout.meta_dir(), tmp.path().join("meta"));
    }

    #[test]
    fn ensure_stream_creates_dirs() {
        let tmp = TempDir::new().expect("failed to create temp dir");
        let layout = StreamLayout::new(tmp.path());

        let dir = layout
            .ensure_stream("order", "abc-123")
            .expect("ensure_stream should succeed");

        assert!(dir.is_dir(), "stream directory should exist on disk");
        assert_eq!(dir, tmp.path().join("streams/order/abc-123"));

        let registry = tmp.path().join("meta/streams.jsonl");
        assert!(registry.is_file(), "registry file should exist");
    }

    #[test]
    fn ensure_stream_idempotent() {
        let tmp = TempDir::new().expect("failed to create temp dir");
        let layout = StreamLayout::new(tmp.path());

        layout
            .ensure_stream("order", "abc-123")
            .expect("first ensure_stream should succeed");
        layout
            .ensure_stream("order", "abc-123")
            .expect("second ensure_stream should succeed");

        let registry = tmp.path().join("meta/streams.jsonl");
        let contents = fs::read_to_string(&registry).expect("failed to read registry");

        let matching_entries: Vec<&str> = contents
            .lines()
            .filter(|line| {
                let v: serde_json::Value =
                    serde_json::from_str(line).expect("line should be valid JSON");
                v.get("type").and_then(|t| t.as_str()) == Some("order")
                    && v.get("id").and_then(|i| i.as_str()) == Some("abc-123")
            })
            .collect();

        assert_eq!(
            matching_entries.len(),
            1,
            "registry should contain exactly one entry for (order, abc-123)"
        );
    }

    #[test]
    fn list_streams_empty_for_unknown_type() {
        let tmp = TempDir::new().expect("failed to create temp dir");
        let layout = StreamLayout::new(tmp.path());

        let streams = layout
            .list_streams("nonexistent")
            .expect("list_streams should succeed for unknown type");

        assert!(streams.is_empty());
    }

    #[test]
    fn list_streams_after_create() {
        let tmp = TempDir::new().expect("failed to create temp dir");
        let layout = StreamLayout::new(tmp.path());

        // Create streams in non-sorted order to verify sorting.
        layout
            .ensure_stream("order", "charlie")
            .expect("ensure_stream should succeed");
        layout
            .ensure_stream("order", "alpha")
            .expect("ensure_stream should succeed");
        layout
            .ensure_stream("order", "bravo")
            .expect("ensure_stream should succeed");

        let streams = layout
            .list_streams("order")
            .expect("list_streams should succeed");

        assert_eq!(streams, vec!["alpha", "bravo", "charlie"]);
    }

    #[test]
    fn registry_entries_valid_json() {
        let tmp = TempDir::new().expect("failed to create temp dir");
        let layout = StreamLayout::new(tmp.path());

        layout
            .ensure_stream("order", "abc-123")
            .expect("ensure_stream should succeed");
        layout
            .ensure_stream("cart", "xyz-789")
            .expect("ensure_stream should succeed");

        let registry = tmp.path().join("meta/streams.jsonl");
        let contents = fs::read_to_string(&registry).expect("failed to read registry");

        for (i, line) in contents.lines().enumerate() {
            let entry: serde_json::Value = serde_json::from_str(line)
                .unwrap_or_else(|e| panic!("line {i} is not valid JSON: {e}"));

            assert!(
                entry.get("type").and_then(|v| v.as_str()).is_some(),
                "line {i} should have a string 'type' field"
            );
            assert!(
                entry.get("id").and_then(|v| v.as_str()).is_some(),
                "line {i} should have a string 'id' field"
            );
            assert!(
                entry.get("ts").and_then(|v| v.as_u64()).is_some(),
                "line {i} should have a numeric 'ts' field"
            );
        }
    }
}
