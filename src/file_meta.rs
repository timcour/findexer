use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq)]
pub struct FileMeta {
    pub filename: String,
    pub extension: Option<String>,
    pub file_path: PathBuf,
    pub directory_path: PathBuf,
    pub filesize: u64,
    pub hash: String,
    pub created_at: i64,
    pub modified_at: i64,
}

#[derive(Error, Debug)]
pub enum FileMetaError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Invalid path: {0}")]
    InvalidPath(String),
}

impl FileMeta {
    pub fn from_path(path: &std::path::Path) -> Result<Self, FileMetaError> {
        let metadata = std::fs::metadata(path)?;

        let filename = path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| FileMetaError::InvalidPath(path.display().to_string()))?
            .to_string();

        let extension = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|s| s.to_string());

        let directory_path = path
            .parent()
            .ok_or_else(|| FileMetaError::InvalidPath(path.display().to_string()))?
            .to_path_buf();

        let created_at = metadata
            .created()
            .map(|t| t.duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64)
            .unwrap_or(0);

        let modified_at = metadata
            .modified()
            .map(|t| t.duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64)
            .unwrap_or(0);

        let hash = compute_hash(path)?;

        Ok(FileMeta {
            filename,
            extension,
            file_path: path.to_path_buf(),
            directory_path,
            filesize: metadata.len(),
            hash,
            created_at,
            modified_at,
        })
    }
}

fn compute_hash(path: &std::path::Path) -> Result<String, FileMetaError> {
    use std::io::Read;
    use xxhash_rust::xxh3::xxh3_64;

    let mut file = std::fs::File::open(path)?;
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer)?;

    let hash = xxh3_64(&buffer);
    Ok(format!("{:016x}", hash))
}

/// Public wrapper for hash computation (used by search)
pub fn compute_hash_public(path: &std::path::Path) -> Result<String, FileMetaError> {
    compute_hash(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_file_meta_from_path() {
        let tmp = TempDir::new().unwrap();
        let file_path = tmp.path().join("test.txt");
        std::fs::write(&file_path, "hello world").unwrap();

        let meta = FileMeta::from_path(&file_path).unwrap();

        assert_eq!(meta.filename, "test.txt");
        assert_eq!(meta.extension, Some("txt".to_string()));
        assert_eq!(meta.filesize, 11);
        assert!(!meta.hash.is_empty());
    }

    #[test]
    fn test_hash_deterministic() {
        let tmp = TempDir::new().unwrap();
        let file_path = tmp.path().join("test.txt");
        std::fs::write(&file_path, "hello world").unwrap();

        let hash1 = compute_hash(&file_path).unwrap();
        let hash2 = compute_hash(&file_path).unwrap();

        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_hash_different_content() {
        let tmp = TempDir::new().unwrap();
        let file1 = tmp.path().join("file1.txt");
        let file2 = tmp.path().join("file2.txt");
        std::fs::write(&file1, "hello").unwrap();
        std::fs::write(&file2, "world").unwrap();

        let hash1 = compute_hash(&file1).unwrap();
        let hash2 = compute_hash(&file2).unwrap();

        assert_ne!(hash1, hash2);
    }

    #[test]
    fn test_hash_is_16_chars() {
        let tmp = TempDir::new().unwrap();
        let file_path = tmp.path().join("test.txt");
        std::fs::write(&file_path, "hello world").unwrap();

        let hash = compute_hash(&file_path).unwrap();

        assert_eq!(hash.len(), 16);
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
