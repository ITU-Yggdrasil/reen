use crate::data::Cache;
use std::fs;
use std::path::PathBuf;

/// FileCache is an implementation of the Cache trait that stores cache artifacts
/// in a file structure. Keys are used to derive both the file name and folder structure.
///
/// The cache is organized as: `{folder}/{instructions_model_hash}/{input_hash}.cache`
/// where instructions_model_hash = hash(agent_instructions + model_name)
#[derive(Debug, Clone)]
pub struct FileCache {
    /// The root folder path for the cache (defaults to ".reen")
    folder: String,
    /// Hash of agent instructions + model name (used as subfolder)
    instructions_model_hash: String,
}

impl FileCache {
    /// Creates a new FileCache instance
    ///
    /// # Arguments
    /// * `folder` - Optional root folder path. If None, defaults to ".reen"
    /// * `instructions_model_hash` - Hash of agent instructions + model name (used as subfolder)
    pub fn new(folder: Option<String>, instructions_model_hash: String) -> Self {
        Self {
            folder: folder.unwrap_or_else(|| ".reen".to_string()),
            instructions_model_hash,
        }
    }

    /// Constructs the cache file path for a given key
    ///
    /// Path format: `{folder}/{instructions_model_hash}/{key}.cache`
    /// Note: The key is expected to be a hash value (hex string), which is already safe for filenames.
    fn get_cache_path(&self, key: &str) -> PathBuf {
        let mut path = PathBuf::from(&self.folder);
        path.push(&self.instructions_model_hash);
        path.push(format!("{}.cache", key));
        path
    }

    /// Gets the directory path for cache files (without the filename)
    fn get_cache_dir(&self) -> PathBuf {
        let mut path = PathBuf::from(&self.folder);
        path.push(&self.instructions_model_hash);
        path
    }
}

impl Cache for FileCache {
    /// Retrieves a cached value for the given key
    ///
    /// # Arguments
    /// * `key` - The cache key to look up
    ///
    /// # Returns
    /// * `Some(String)` - The cached value if found and readable
    /// * `None` - If the cache file doesn't exist or cannot be read
    fn get(&self, key: &str) -> Option<String> {
        let path = self.get_cache_path(key);

        match fs::read_to_string(&path) {
            Ok(contents) => Some(contents),
            Err(_) => {
                // File not found or read error - treat as cache miss
                None
            }
        }
    }

    /// Stores a value in the cache for the given key
    ///
    /// Creates necessary directories if they don't exist. Errors are handled
    /// gracefully without panicking.
    ///
    /// # Arguments
    /// * `key` - The cache key to store under
    /// * `value` - The value to cache
    fn set(&self, key: &str, value: &str) {
        let path = self.get_cache_path(key);
        let dir = self.get_cache_dir();

        // Create directories if they don't exist
        if let Err(e) = fs::create_dir_all(&dir) {
            eprintln!("Failed to create cache directory {:?}: {}", dir, e);
            return;
        }

        // Write the cache file
        if let Err(e) = fs::write(&path, value) {
            eprintln!("Failed to write cache file {:?}: {}", path, e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_cache_path_construction() {
        let cache = FileCache::new(Some("/tmp/cache".to_string()), "abc123".to_string());
        let path = cache.get_cache_path("test_key");
        assert_eq!(path.to_str().unwrap(), "/tmp/cache/abc123/test_key.cache");
    }

    #[test]
    fn test_cache_get_set() {
        // Use a temporary directory for testing
        let test_dir = format!("/tmp/reen_test_{}", std::process::id());
        let cache = FileCache::new(Some(test_dir.clone()), "test_hash".to_string());

        // Test cache miss
        assert_eq!(cache.get("nonexistent"), None);

        // Test cache set and get
        cache.set("test_key", "test_value");
        assert_eq!(cache.get("test_key"), Some("test_value".to_string()));

        // Test overwrite
        cache.set("test_key", "new_value");
        assert_eq!(cache.get("test_key"), Some("new_value".to_string()));

        // Cleanup
        let _ = fs::remove_dir_all(&test_dir);
    }

    #[test]
    fn test_default_folder() {
        let cache = FileCache::new(None, "test_hash".to_string());
        assert_eq!(cache.folder, ".reen");
    }
}
