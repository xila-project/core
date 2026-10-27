use std::path::PathBuf;

/// Use an explicitly selected Applications workspace when testing the split.
/// Until the migration is complete, the existing workspace remains a fallback.
pub fn build(name: &str) -> Result<PathBuf, String> {
    let manifest = std::env::var_os("XILA_APPLICATIONS_MANIFEST").map(PathBuf::from);
    xila::executable::build_crate_with_manifest(name, manifest.as_deref())
}
