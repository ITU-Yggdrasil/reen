// Compatibility shim for legacy `crate::contexts` references.
//
// The canonical cache implementation now lives in `crate::execution::FileCache`.
#[allow(dead_code)]
pub type FileCache = crate::execution::FileCache;
