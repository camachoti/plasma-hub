//! Filesystem and OS helpers that do not belong to a specific product domain.

use std::path::Path;

/// Returns whether a path exists, mirroring the old `path_exists` command.
pub fn path_exists(path: &str) -> bool {
    Path::new(path).exists()
}

/// Creates a directory and every missing parent.
pub fn ensure_dir(path: &str) -> Result<(), String> {
    std::fs::create_dir_all(path).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_nested_directories() {
        let root =
            std::env::temp_dir().join(format!("plasma-platform-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let target = root.join("a").join("b");
        ensure_dir(target.to_string_lossy().as_ref()).unwrap();
        assert!(path_exists(target.to_string_lossy().as_ref()));
        assert!(!path_exists(
            root.join("missing").to_string_lossy().as_ref()
        ));
        let _ = std::fs::remove_dir_all(&root);
    }
}
