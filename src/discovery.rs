use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// Discover all files under a path, skipping symlinks.
/// Returns paths in sorted order for deterministic processing.
pub fn discover_files(root: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = WalkDir::new(root)
        .follow_links(false) // Skip symlinks
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file()) // Only files, not dirs or symlinks
        .map(|e| e.path().to_path_buf())
        .collect();

    // Sort for deterministic ordering
    files.sort();
    files
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;
    use tempfile::TempDir;

    #[test]
    fn test_discover_files_basic() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("a.txt"), "a").unwrap();
        std::fs::write(tmp.path().join("b.txt"), "b").unwrap();
        std::fs::create_dir(tmp.path().join("subdir")).unwrap();
        std::fs::write(tmp.path().join("subdir/c.txt"), "c").unwrap();

        let files = discover_files(tmp.path());

        assert_eq!(files.len(), 3);
        assert!(files[0].ends_with("a.txt"));
        assert!(files[1].ends_with("b.txt"));
        assert!(files[2].ends_with("subdir/c.txt"));
    }

    #[test]
    fn test_discover_files_skips_symlinks() {
        let tmp = TempDir::new().unwrap();
        let real_file = tmp.path().join("real.txt");
        let link_file = tmp.path().join("link.txt");

        std::fs::write(&real_file, "content").unwrap();
        symlink(&real_file, &link_file).unwrap();

        let files = discover_files(tmp.path());

        assert_eq!(files.len(), 1);
        assert!(files[0].ends_with("real.txt"));
    }

    #[test]
    fn test_discover_files_sorted() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("z.txt"), "z").unwrap();
        std::fs::write(tmp.path().join("a.txt"), "a").unwrap();
        std::fs::write(tmp.path().join("m.txt"), "m").unwrap();

        let files = discover_files(tmp.path());

        assert!(files[0] < files[1]);
        assert!(files[1] < files[2]);
    }
}
