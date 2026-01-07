/// Cache trait for storing and retrieving string values by key.
///
/// Implementations should handle errors gracefully without panicking.
pub trait Cache {
    /// Retrieves a cached value for the given key.
    ///
    /// # Arguments
    /// * `key` - The cache key to look up
    ///
    /// # Returns
    /// * `Some(String)` - The cached value if found
    /// * `None` - If the key doesn't exist or retrieval fails
    fn get(&self, key: &str) -> Option<String>;

    /// Stores a value in the cache for the given key.
    ///
    /// # Arguments
    /// * `key` - The cache key to store under
    /// * `value` - The value to cache
    ///
    /// # Notes
    /// Errors during storage should be handled gracefully (logged but not panicked).
    /// This method does not return errors to maintain fire-and-forget semantics.
    fn set(&self, key: &str, value: &str);
}
