use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum StateError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct IndexState {
    pub root_path: String,
    pub processed_files: HashSet<String>,
    pub total_discovered: usize,
}

impl IndexState {
    pub fn state_file_path() -> PathBuf {
        directories::BaseDirs::new()
            .map(|dirs| dirs.home_dir().join(".findex").join("index_state.json"))
            .unwrap_or_else(|| PathBuf::from(".findex/index_state.json"))
    }

    pub fn load() -> Result<Option<Self>, StateError> {
        let path = Self::state_file_path();
        if !path.exists() {
            return Ok(None);
        }
        let content = std::fs::read_to_string(&path)?;
        let state: Self = serde_json::from_str(&content)?;
        Ok(Some(state))
    }

    pub fn save(&self) -> Result<(), StateError> {
        let path = Self::state_file_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = serde_json::to_string_pretty(self)?;
        std::fs::write(&path, content)?;
        Ok(())
    }

    pub fn clear() -> Result<(), StateError> {
        let path = Self::state_file_path();
        if path.exists() {
            std::fs::remove_file(&path)?;
        }
        Ok(())
    }

    pub fn is_processed(&self, path: &Path) -> bool {
        self.processed_files.contains(&path.display().to_string())
    }

    pub fn mark_processed(&mut self, path: &Path) {
        self.processed_files.insert(path.display().to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_state_save_load() {
        let tmp = TempDir::new().unwrap();
        let state_path = tmp.path().join("state.json");

        let mut state = IndexState::default();
        state.root_path = "/test/path".to_string();
        state.mark_processed(Path::new("/test/path/file.txt"));
        state.total_discovered = 100;

        // Save
        let content = serde_json::to_string_pretty(&state).unwrap();
        std::fs::write(&state_path, &content).unwrap();

        // Load
        let loaded_content = std::fs::read_to_string(&state_path).unwrap();
        let loaded: IndexState = serde_json::from_str(&loaded_content).unwrap();

        assert_eq!(loaded.root_path, "/test/path");
        assert!(loaded.is_processed(Path::new("/test/path/file.txt")));
        assert_eq!(loaded.total_discovered, 100);
    }

    #[test]
    fn test_mark_processed() {
        let mut state = IndexState::default();
        let path = Path::new("/some/file.txt");

        assert!(!state.is_processed(path));
        state.mark_processed(path);
        assert!(state.is_processed(path));
    }
}
