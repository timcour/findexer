# Findex File Indexer Implementation Plan

## Overview

Build a Rust CLI application for indexing and searching file metadata across a filesystem. The app will store metadata in SQLite and support parallel indexing with resume capability.

## Current State Analysis

- Fresh repository with only README.md and requirements document
- No existing code or structure

## Desired End State

A fully functional `findex` CLI with:
- `findex index /path` - Parallel file indexing with resume capability
- `findex search term` - Search by filename, path, hash, or filesize
- SQLite database at `~/.findex/findex.db` with proper migrations
- Makefile-driven build and test workflow

### Verification:
```bash
# Build and test
make build && make test

# Index a directory
./target/release/findex index /some/path

# Interrupt and resume
# (Ctrl+C during indexing, then run same command - should resume)

# Search
./target/release/findex search myfile.txt
./target/release/findex search --short myfile.txt
```

## What We're NOT Doing

- Real-time file watching / automatic re-indexing
- Full-text content search (only metadata)
- Following symlinks
- Network filesystem special handling
- GUI interface

## Implementation Approach

Build incrementally in 5 phases, each with passing tests before proceeding:

1. **Project Setup & Database** - Foundation with migrations
2. **File Discovery & Metadata** - Single-threaded traversal and hashing
3. **Parallel Indexing with Resume** - Concurrency and state management
4. **Search Functionality** - Query implementation with formatting
5. **CLI Polish** - Progress reporting and error handling

---

## Phase 1: Project Setup & Database Foundation

### Overview
Set up Cargo project structure, Makefile, SQLite database connection, and migration system.

### Changes Required:

#### 1. Cargo Project Initialization

**File**: `Cargo.toml`
```toml
[package]
name = "findex"
version = "0.1.0"
edition = "2021"

[dependencies]
rusqlite = { version = "0.31", features = ["bundled"] }
refinery = { version = "0.8", features = ["rusqlite"] }
thiserror = "1.0"
directories = "5.0"

[dev-dependencies]
tempfile = "3.10"
```

#### 2. Makefile

**File**: `Makefile`
```makefile
.PHONY: build test clean

build:
	cargo build --release

test:
	cargo test

clean:
	cargo clean
```

#### 3. Project Structure

Create directory structure:
```
src/
  main.rs
  lib.rs
  db/
    mod.rs
    migrations/
      V1__initial_schema.sql
```

#### 4. Initial Migration

**File**: `src/db/migrations/V1__initial_schema.sql`
```sql
CREATE TABLE files (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    filename TEXT NOT NULL,
    extension TEXT,
    file_path TEXT NOT NULL UNIQUE,
    directory_path TEXT NOT NULL,
    filesize INTEGER NOT NULL,
    hash TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    modified_at INTEGER NOT NULL,
    indexed_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
);

-- Indexes for common search patterns
CREATE INDEX idx_files_filename ON files(filename);
CREATE INDEX idx_files_hash ON files(hash);
CREATE INDEX idx_files_filesize ON files(filesize);
CREATE INDEX idx_files_directory_path ON files(directory_path);
CREATE INDEX idx_files_extension ON files(extension);
```

#### 5. Database Module

**File**: `src/db/mod.rs`
```rust
use refinery::embed_migrations;
use rusqlite::Connection;
use std::path::PathBuf;
use thiserror::Error;

embed_migrations!("src/db/migrations");

#[derive(Error, Debug)]
pub enum DbError {
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("Migration error: {0}")]
    Migration(#[from] refinery::Error),
    #[error("Failed to create database directory: {0}")]
    CreateDir(std::io::Error),
}

pub fn get_db_path() -> PathBuf {
    directories::BaseDirs::new()
        .map(|dirs| dirs.home_dir().join(".findex").join("findex.db"))
        .unwrap_or_else(|| PathBuf::from(".findex/findex.db"))
}

pub fn open_connection(path: &PathBuf) -> Result<Connection, DbError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(DbError::CreateDir)?;
    }
    let conn = Connection::open(path)?;
    Ok(conn)
}

pub fn run_migrations(conn: &mut Connection) -> Result<(), DbError> {
    migrations::runner().run(conn)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_open_and_migrate() {
        let tmp = TempDir::new().unwrap();
        let db_path = tmp.path().join("test.db");
        let mut conn = open_connection(&db_path).unwrap();
        run_migrations(&mut conn).unwrap();

        // Verify tables exist
        let count: i32 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='files'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_migrations_idempotent() {
        let tmp = TempDir::new().unwrap();
        let db_path = tmp.path().join("test.db");
        let mut conn = open_connection(&db_path).unwrap();

        // Run migrations twice - should not error
        run_migrations(&mut conn).unwrap();
        run_migrations(&mut conn).unwrap();
    }
}
```

#### 6. Main Entry Point

**File**: `src/main.rs`
```rust
mod db;

fn main() {
    println!("findex - file indexer");
}
```

**File**: `src/lib.rs`
```rust
pub mod db;
```

### Success Criteria:

#### Automated Verification:
- [ ] Project compiles: `make build`
- [ ] Tests pass: `make test`
- [ ] Database file created at expected location when opened
- [ ] Migration creates `files` table with correct schema
- [ ] Running migrations twice does not error

#### Manual Verification:
- [ ] `./target/release/findex` runs without error

---

## Phase 2: File Discovery & Metadata Extraction

### Overview
Implement file system traversal (skipping symlinks) and metadata extraction including xxHash computation.

### Changes Required:

#### 1. Add Dependencies

**File**: `Cargo.toml` (add to dependencies)
```toml
xxhash-rust = { version = "0.8", features = ["xxh3"] }
walkdir = "2.5"
```

#### 2. File Metadata Types

**File**: `src/file_meta.rs`
```rust
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
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
}
```

#### 3. File Discovery (Directory Walker)

**File**: `src/discovery.rs`
```rust
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
```

#### 4. Update lib.rs

**File**: `src/lib.rs`
```rust
pub mod db;
pub mod discovery;
pub mod file_meta;
```

### Success Criteria:

#### Automated Verification:
- [ ] All tests pass: `make test`
- [ ] File metadata correctly extracted (filename, extension, size, timestamps)
- [ ] xxHash produces consistent 16-char hex strings
- [ ] Symlinks are skipped during discovery
- [ ] File list is sorted for deterministic ordering

#### Manual Verification:
- [ ] Create a test directory with various files and verify discovery works

---

## Phase 3: Parallel Indexing with Resume

### Overview
Implement parallel file processing using rayon, database batch inserts, and state file for resume capability.

### Changes Required:

#### 1. Add Dependencies

**File**: `Cargo.toml` (add to dependencies)
```toml
rayon = "1.10"
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
clap = { version = "4.5", features = ["derive"] }
```

#### 2. State File Management

**File**: `src/state.rs`
```rust
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
}
```

#### 3. Indexer Module

**File**: `src/indexer.rs`
```rust
use crate::db::DbError;
use crate::file_meta::{FileMeta, FileMetaError};
use crate::state::{IndexState, StateError};
use rayon::prelude::*;
use rusqlite::Connection;
use std::path::Path;
use std::sync::{Arc, Mutex};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum IndexError {
    #[error("Database error: {0}")]
    Db(#[from] DbError),
    #[error("State error: {0}")]
    State(#[from] StateError),
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
}

pub struct IndexResult {
    pub files_processed: usize,
    pub files_skipped: usize,
    pub errors: usize,
}

/// Insert or update a file record in the database.
pub fn upsert_file(conn: &Connection, meta: &FileMeta) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT INTO files (filename, extension, file_path, directory_path, filesize, hash, created_at, modified_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(file_path) DO UPDATE SET
           filename = excluded.filename,
           extension = excluded.extension,
           directory_path = excluded.directory_path,
           filesize = excluded.filesize,
           hash = excluded.hash,
           created_at = excluded.created_at,
           modified_at = excluded.modified_at,
           indexed_at = strftime('%s', 'now')",
        rusqlite::params![
            meta.filename,
            meta.extension,
            meta.file_path.display().to_string(),
            meta.directory_path.display().to_string(),
            meta.filesize as i64,
            meta.hash,
            meta.created_at,
            meta.modified_at,
        ],
    )?;
    Ok(())
}

/// Index files in parallel, with resume support.
///
/// Strategy:
/// 1. Discover all files (sorted for determinism)
/// 2. Filter out already-processed files from state
/// 3. Process remaining files in parallel batches
/// 4. Collect results and batch-insert to DB (single-threaded for SQLite)
/// 5. Update state after each batch
pub fn index_directory(
    conn: &mut Connection,
    root: &Path,
    batch_size: usize,
) -> Result<IndexResult, IndexError> {
    let root_str = root.display().to_string();

    // Load or create state
    let mut state = IndexState::load()?.unwrap_or_default();

    // Check if we're resuming a different path - if so, start fresh
    if !state.root_path.is_empty() && state.root_path != root_str {
        state = IndexState::default();
    }
    state.root_path = root_str;

    // Discover files
    let all_files = crate::discovery::discover_files(root);
    state.total_discovered = all_files.len();

    // Filter out already processed
    let pending_files: Vec<_> = all_files
        .into_iter()
        .filter(|p| !state.is_processed(p))
        .collect();

    let mut result = IndexResult {
        files_processed: 0,
        files_skipped: state.processed_files.len(),
        errors: 0,
    };

    // Process in batches
    for batch in pending_files.chunks(batch_size) {
        // Parallel metadata extraction
        let metas: Vec<_> = batch
            .par_iter()
            .map(|path| {
                let meta_result = FileMeta::from_path(path);
                (path.clone(), meta_result)
            })
            .collect();

        // Single-threaded database inserts (SQLite limitation)
        let tx = conn.transaction()?;
        for (path, meta_result) in &metas {
            match meta_result {
                Ok(meta) => {
                    if let Err(e) = upsert_file(&tx, meta) {
                        eprintln!("Error inserting {}: {}", path.display(), e);
                        result.errors += 1;
                    } else {
                        result.files_processed += 1;
                    }
                }
                Err(e) => {
                    eprintln!("Error reading {}: {}", path.display(), e);
                    result.errors += 1;
                }
            }
            state.mark_processed(path);
        }
        tx.commit()?;

        // Save state after each batch
        state.save()?;
    }

    // Clear state on successful completion
    IndexState::clear()?;

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{open_connection, run_migrations};
    use tempfile::TempDir;

    fn setup_test_db() -> (TempDir, Connection) {
        let tmp = TempDir::new().unwrap();
        let db_path = tmp.path().join("test.db");
        let mut conn = open_connection(&db_path).unwrap();
        run_migrations(&mut conn).unwrap();
        (tmp, conn)
    }

    #[test]
    fn test_upsert_file() {
        let (_tmp, conn) = setup_test_db();

        let meta = FileMeta {
            filename: "test.txt".to_string(),
            extension: Some("txt".to_string()),
            file_path: "/test/test.txt".into(),
            directory_path: "/test".into(),
            filesize: 100,
            hash: "abc123".to_string(),
            created_at: 1000,
            modified_at: 2000,
        };

        upsert_file(&conn, &meta).unwrap();

        let count: i32 = conn
            .query_row("SELECT COUNT(*) FROM files", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_upsert_updates_existing() {
        let (_tmp, conn) = setup_test_db();

        let meta1 = FileMeta {
            filename: "test.txt".to_string(),
            extension: Some("txt".to_string()),
            file_path: "/test/test.txt".into(),
            directory_path: "/test".into(),
            filesize: 100,
            hash: "abc123".to_string(),
            created_at: 1000,
            modified_at: 2000,
        };

        upsert_file(&conn, &meta1).unwrap();

        let meta2 = FileMeta {
            filesize: 200,
            hash: "def456".to_string(),
            ..meta1.clone()
        };

        upsert_file(&conn, &meta2).unwrap();

        let (size, hash): (i64, String) = conn
            .query_row(
                "SELECT filesize, hash FROM files WHERE file_path = ?1",
                ["/test/test.txt"],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();

        assert_eq!(size, 200);
        assert_eq!(hash, "def456");
    }

    #[test]
    fn test_index_directory() {
        let tmp_files = TempDir::new().unwrap();
        std::fs::write(tmp_files.path().join("a.txt"), "content a").unwrap();
        std::fs::write(tmp_files.path().join("b.txt"), "content b").unwrap();

        let (_tmp_db, mut conn) = setup_test_db();

        let result = index_directory(&mut conn, tmp_files.path(), 10).unwrap();

        assert_eq!(result.files_processed, 2);
        assert_eq!(result.errors, 0);

        let count: i32 = conn
            .query_row("SELECT COUNT(*) FROM files", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 2);
    }
}
```

#### 4. Update lib.rs

**File**: `src/lib.rs`
```rust
pub mod db;
pub mod discovery;
pub mod file_meta;
pub mod indexer;
pub mod state;
```

### Success Criteria:

#### Automated Verification:
- [ ] All tests pass: `make test`
- [ ] Files are indexed with correct metadata in database
- [ ] Parallel processing works (rayon)
- [ ] State file created during indexing
- [ ] State file cleared after successful completion

#### Manual Verification:
- [ ] Index a directory, interrupt with Ctrl+C, resume - continues from where it left off
- [ ] State file at `~/.findex/index_state.json` shows progress during indexing

---

## Phase 4: Search Functionality

### Overview
Implement search command with multiple search strategies and formatted output.

### Changes Required:

#### 1. Add Dependencies

**File**: `Cargo.toml` (add to dependencies)
```toml
comfy-table = "7.1"
```

#### 2. Search Module

**File**: `src/search.rs`
```rust
use crate::file_meta::FileMeta;
use rusqlite::Connection;
use std::path::Path;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum SearchError {
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone)]
pub struct SearchResult {
    pub id: i64,
    pub filename: String,
    pub extension: Option<String>,
    pub file_path: String,
    pub directory_path: String,
    pub filesize: i64,
    pub hash: String,
    pub created_at: i64,
    pub modified_at: i64,
    pub duplicate_count: Option<i64>,
}

/// Search for files matching the given term.
/// Searches by: filename (partial), file_path (partial), hash (exact), filesize (exact).
/// If term is a path to an existing file, also searches by that file's hash.
pub fn search(conn: &Connection, term: &str) -> Result<Vec<SearchResult>, SearchError> {
    let mut results = Vec::new();
    let mut seen_ids = std::collections::HashSet::new();

    // Try to parse as filesize
    if let Ok(size) = term.parse::<i64>() {
        let size_results = search_by_filesize(conn, size)?;
        for r in size_results {
            if seen_ids.insert(r.id) {
                results.push(r);
            }
        }
    }

    // Search by hash (exact match, 16 hex chars)
    if term.len() == 16 && term.chars().all(|c| c.is_ascii_hexdigit()) {
        let hash_results = search_by_hash(conn, term)?;
        for r in hash_results {
            if seen_ids.insert(r.id) {
                results.push(r);
            }
        }
    }

    // Search by filename (partial match)
    let name_results = search_by_filename(conn, term)?;
    for r in name_results {
        if seen_ids.insert(r.id) {
            results.push(r);
        }
    }

    // Search by path (partial match)
    let path_results = search_by_path(conn, term)?;
    for r in path_results {
        if seen_ids.insert(r.id) {
            results.push(r);
        }
    }

    // If term is a path to an existing file, hash it and search
    let term_path = Path::new(term);
    if term_path.is_file() {
        if let Ok(hash) = crate::file_meta::compute_hash_public(term_path) {
            let hash_results = search_by_hash(conn, &hash)?;
            for r in hash_results {
                if seen_ids.insert(r.id) {
                    results.push(r);
                }
            }
        }
    }

    // Add duplicate counts
    for result in &mut results {
        result.duplicate_count = Some(count_by_hash(conn, &result.hash)?);
    }

    Ok(results)
}

fn search_by_filename(conn: &Connection, term: &str) -> Result<Vec<SearchResult>, SearchError> {
    let pattern = format!("%{}%", term);
    let mut stmt = conn.prepare(
        "SELECT id, filename, extension, file_path, directory_path, filesize, hash, created_at, modified_at
         FROM files WHERE filename LIKE ?1 LIMIT 100"
    )?;

    collect_results(&mut stmt, &[&pattern])
}

fn search_by_path(conn: &Connection, term: &str) -> Result<Vec<SearchResult>, SearchError> {
    let pattern = format!("%{}%", term);
    let mut stmt = conn.prepare(
        "SELECT id, filename, extension, file_path, directory_path, filesize, hash, created_at, modified_at
         FROM files WHERE file_path LIKE ?1 LIMIT 100"
    )?;

    collect_results(&mut stmt, &[&pattern])
}

fn search_by_hash(conn: &Connection, hash: &str) -> Result<Vec<SearchResult>, SearchError> {
    let mut stmt = conn.prepare(
        "SELECT id, filename, extension, file_path, directory_path, filesize, hash, created_at, modified_at
         FROM files WHERE hash = ?1 LIMIT 100"
    )?;

    collect_results(&mut stmt, &[&hash])
}

fn search_by_filesize(conn: &Connection, size: i64) -> Result<Vec<SearchResult>, SearchError> {
    let mut stmt = conn.prepare(
        "SELECT id, filename, extension, file_path, directory_path, filesize, hash, created_at, modified_at
         FROM files WHERE filesize = ?1 LIMIT 100"
    )?;

    collect_results(&mut stmt, &[&size])
}

fn count_by_hash(conn: &Connection, hash: &str) -> Result<i64, SearchError> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM files WHERE hash = ?1",
        [hash],
        |row| row.get(0),
    )?;
    Ok(count)
}

fn collect_results<P: rusqlite::Params>(
    stmt: &mut rusqlite::Statement,
    params: P,
) -> Result<Vec<SearchResult>, SearchError> {
    let rows = stmt.query_map(params, |row| {
        Ok(SearchResult {
            id: row.get(0)?,
            filename: row.get(1)?,
            extension: row.get(2)?,
            file_path: row.get(3)?,
            directory_path: row.get(4)?,
            filesize: row.get(5)?,
            hash: row.get(6)?,
            created_at: row.get(7)?,
            modified_at: row.get(8)?,
            duplicate_count: None,
        })
    })?;

    let mut results = Vec::new();
    for row in rows {
        results.push(row?);
    }
    Ok(results)
}

/// Format results as a detailed table
pub fn format_table(results: &[SearchResult]) -> String {
    use comfy_table::{Table, ContentArrangement};

    if results.is_empty() {
        return "No results found.".to_string();
    }

    let mut table = Table::new();
    table.set_content_arrangement(ContentArrangement::Dynamic);
    table.set_header(vec![
        "Filename", "Extension", "Path", "Size", "Hash", "Created", "Modified"
    ]);

    for r in results {
        table.add_row(vec![
            &r.filename,
            r.extension.as_deref().unwrap_or("-"),
            &r.file_path,
            &format_size(r.filesize),
            &r.hash[..8], // Truncate hash for display
            &format_timestamp(r.created_at),
            &format_timestamp(r.modified_at),
        ]);
    }

    table.to_string()
}

/// Format results in short format: filepath, hash, duplicate count
pub fn format_short(results: &[SearchResult]) -> String {
    if results.is_empty() {
        return "No results found.".to_string();
    }

    let mut output = String::new();
    for r in results {
        let dups = r.duplicate_count.unwrap_or(1);
        let dup_str = if dups > 1 {
            format!(" ({} duplicates)", dups)
        } else {
            String::new()
        };
        output.push_str(&format!("{} {} {}\n", r.file_path, r.hash, dup_str));
    }
    output.trim_end().to_string()
}

fn format_size(bytes: i64) -> String {
    const KB: i64 = 1024;
    const MB: i64 = KB * 1024;
    const GB: i64 = MB * 1024;

    if bytes >= GB {
        format!("{:.1}G", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1}M", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1}K", bytes as f64 / KB as f64)
    } else {
        format!("{}B", bytes)
    }
}

fn format_timestamp(ts: i64) -> String {
    use std::time::{Duration, UNIX_EPOCH};
    let datetime = UNIX_EPOCH + Duration::from_secs(ts as u64);
    // Simple date format without external crate
    let secs_since_epoch = datetime.duration_since(UNIX_EPOCH).unwrap().as_secs();
    let days = secs_since_epoch / 86400;
    let years = 1970 + days / 365; // Approximate
    format!("{}", years)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{open_connection, run_migrations};
    use crate::indexer::upsert_file;
    use crate::file_meta::FileMeta;
    use tempfile::TempDir;

    fn setup_test_db_with_data() -> (TempDir, Connection) {
        let tmp = TempDir::new().unwrap();
        let db_path = tmp.path().join("test.db");
        let mut conn = open_connection(&db_path).unwrap();
        run_migrations(&mut conn).unwrap();

        // Insert test data
        let files = vec![
            FileMeta {
                filename: "readme.txt".to_string(),
                extension: Some("txt".to_string()),
                file_path: "/home/user/readme.txt".into(),
                directory_path: "/home/user".into(),
                filesize: 1024,
                hash: "abc123def456789a".to_string(),
                created_at: 1000,
                modified_at: 2000,
            },
            FileMeta {
                filename: "readme.md".to_string(),
                extension: Some("md".to_string()),
                file_path: "/home/user/docs/readme.md".into(),
                directory_path: "/home/user/docs".into(),
                filesize: 2048,
                hash: "fff000fff000fff0".to_string(),
                created_at: 1500,
                modified_at: 2500,
            },
            FileMeta {
                filename: "duplicate.txt".to_string(),
                extension: Some("txt".to_string()),
                file_path: "/home/user/duplicate.txt".into(),
                directory_path: "/home/user".into(),
                filesize: 1024,
                hash: "abc123def456789a".to_string(), // Same hash as readme.txt
                created_at: 1000,
                modified_at: 2000,
            },
        ];

        for f in &files {
            upsert_file(&conn, f).unwrap();
        }

        (tmp, conn)
    }

    #[test]
    fn test_search_by_filename() {
        let (_tmp, conn) = setup_test_db_with_data();

        let results = search(&conn, "readme").unwrap();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_search_by_hash() {
        let (_tmp, conn) = setup_test_db_with_data();

        let results = search(&conn, "abc123def456789a").unwrap();
        assert_eq!(results.len(), 2); // Two files with same hash
    }

    #[test]
    fn test_search_by_filesize() {
        let (_tmp, conn) = setup_test_db_with_data();

        let results = search(&conn, "1024").unwrap();
        assert_eq!(results.len(), 2); // Two files with size 1024
    }

    #[test]
    fn test_search_by_path() {
        let (_tmp, conn) = setup_test_db_with_data();

        let results = search(&conn, "/docs/").unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].filename, "readme.md");
    }

    #[test]
    fn test_duplicate_count() {
        let (_tmp, conn) = setup_test_db_with_data();

        let results = search(&conn, "readme.txt").unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].duplicate_count, Some(2)); // Shares hash with duplicate.txt
    }
}
```

#### 3. Expose Hash Function Publicly

**File**: `src/file_meta.rs` (add public wrapper)
```rust
// Add this function at the end of the file, before #[cfg(test)]

/// Public wrapper for hash computation (used by search)
pub fn compute_hash_public(path: &std::path::Path) -> Result<String, FileMetaError> {
    compute_hash(path)
}
```

#### 4. Update lib.rs

**File**: `src/lib.rs`
```rust
pub mod db;
pub mod discovery;
pub mod file_meta;
pub mod indexer;
pub mod search;
pub mod state;
```

### Success Criteria:

#### Automated Verification:
- [ ] All tests pass: `make test`
- [ ] Search by filename returns partial matches
- [ ] Search by hash returns exact matches
- [ ] Search by filesize returns exact matches
- [ ] Search by path returns partial matches
- [ ] Duplicate count is calculated correctly
- [ ] Table and short format output correctly

#### Manual Verification:
- [ ] Search results display in readable table format
- [ ] `--short` flag shows compact output

---

## Phase 5: CLI Integration & Polish

### Overview
Wire everything together with clap CLI, progress reporting, and error handling.

### Changes Required:

#### 1. Add Dependencies

**File**: `Cargo.toml` (add to dependencies)
```toml
indicatif = "0.17"
```

#### 2. CLI Module

**File**: `src/cli.rs`
```rust
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "findex")]
#[command(about = "Fast file indexer and search tool")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Index files in a directory
    Index {
        /// Path to index
        path: PathBuf,

        /// Batch size for processing
        #[arg(long, default_value = "1000")]
        batch_size: usize,
    },
    /// Search indexed files
    Search {
        /// Search term (filename, path, hash, or filesize)
        term: String,

        /// Short output format (path, hash, duplicate count)
        #[arg(short, long)]
        short: bool,
    },
}
```

#### 3. Main Implementation

**File**: `src/main.rs`
```rust
mod cli;
pub mod db;
pub mod discovery;
pub mod file_meta;
pub mod indexer;
pub mod search;
pub mod state;

use clap::Parser;
use cli::{Cli, Commands};
use indicatif::{ProgressBar, ProgressStyle};

fn main() {
    if let Err(e) = run() {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Index { path, batch_size } => {
            run_index(&path, batch_size)?;
        }
        Commands::Search { term, short } => {
            run_search(&term, short)?;
        }
    }

    Ok(())
}

fn run_index(path: &std::path::Path, batch_size: usize) -> Result<(), Box<dyn std::error::Error>> {
    // Validate path
    if !path.exists() {
        return Err(format!("Path does not exist: {}", path.display()).into());
    }
    if !path.is_dir() {
        return Err(format!("Path is not a directory: {}", path.display()).into());
    }

    // Open database
    let db_path = db::get_db_path();
    println!("Database: {}", db_path.display());
    let mut conn = db::open_connection(&db_path)?;
    db::run_migrations(&mut conn)?;

    // Check for resume
    if let Some(state) = state::IndexState::load()? {
        if state.root_path == path.display().to_string() {
            println!(
                "Resuming previous indexing: {}/{} files processed",
                state.processed_files.len(),
                state.total_discovered
            );
        }
    }

    // Discover files
    println!("Discovering files in {}...", path.display());
    let files = discovery::discover_files(path);
    println!("Found {} files", files.len());

    // Setup progress bar
    let pb = ProgressBar::new(files.len() as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} ({eta})")
            .unwrap()
            .progress_chars("#>-"),
    );

    // Index with progress updates
    let result = indexer::index_directory(&mut conn, path, batch_size)?;

    pb.finish_with_message("done");

    println!("\nIndexing complete:");
    println!("  Files processed: {}", result.files_processed);
    println!("  Files skipped (already indexed): {}", result.files_skipped);
    println!("  Errors: {}", result.errors);

    Ok(())
}

fn run_search(term: &str, short: bool) -> Result<(), Box<dyn std::error::Error>> {
    // Open database
    let db_path = db::get_db_path();
    if !db_path.exists() {
        return Err("Database not found. Run 'findex index <path>' first.".into());
    }

    let conn = db::open_connection(&db_path)?;

    // Search
    let results = search::search(&conn, term)?;

    // Output
    if short {
        println!("{}", search::format_short(&results));
    } else {
        println!("{}", search::format_table(&results));
    }

    Ok(())
}
```

#### 4. Update lib.rs for Integration Tests

**File**: `src/lib.rs`
```rust
pub mod db;
pub mod discovery;
pub mod file_meta;
pub mod indexer;
pub mod search;
pub mod state;
```

#### 5. Integration Test

**File**: `tests/integration_test.rs`
```rust
use std::process::Command;
use tempfile::TempDir;

#[test]
fn test_cli_index_and_search() {
    // Create test directory with files
    let tmp = TempDir::new().unwrap();
    std::fs::write(tmp.path().join("hello.txt"), "hello world").unwrap();
    std::fs::write(tmp.path().join("test.rs"), "fn main() {}").unwrap();

    // Build the binary
    let status = Command::new("cargo")
        .args(["build", "--release"])
        .status()
        .expect("Failed to build");
    assert!(status.success());

    // Note: Full CLI tests would require setting up a test database path
    // This is a placeholder for the structure
}
```

#### 6. Update Makefile

**File**: `Makefile`
```makefile
.PHONY: build test clean run-index run-search

build:
	cargo build --release

test:
	cargo test

clean:
	cargo clean

# Development helpers
run-index:
	cargo run --release -- index $(PATH)

run-search:
	cargo run --release -- search $(TERM)
```

### Success Criteria:

#### Automated Verification:
- [ ] Build succeeds: `make build`
- [ ] All tests pass: `make test`
- [ ] CLI parses arguments correctly
- [ ] `findex --help` shows usage
- [ ] `findex index --help` shows index options
- [ ] `findex search --help` shows search options

#### Manual Verification:
- [ ] `findex index /some/path` indexes files with progress bar
- [ ] Ctrl+C during indexing, then re-run - resumes from where it stopped
- [ ] `findex search myfile` returns results in table format
- [ ] `findex search --short myfile` returns compact output
- [ ] Error messages are clear and helpful
- [ ] Database created at `~/.findex/findex.db`

---

## Testing Strategy

### Unit Tests (per module):
- `db`: Connection opening, migration application
- `file_meta`: Metadata extraction, hash computation
- `discovery`: File traversal, symlink skipping, sorting
- `state`: Save/load state, marking processed
- `indexer`: Upsert, batch processing
- `search`: All search strategies, formatting

### Integration Tests:
- Full index + search workflow
- Resume after interrupt
- Search with various term types

### Manual Testing Steps:
1. Create a test directory with varied files (different sizes, extensions, nested dirs)
2. Run `findex index ./test_dir`
3. Interrupt with Ctrl+C partway through
4. Re-run same command - verify it resumes
5. Run various searches and verify results
6. Test `--short` output format

## Performance Considerations

- **Parallel hashing**: rayon for CPU-bound hash computation
- **Batch inserts**: Transaction batching for SQLite writes
- **SQLite indexes**: On filename, hash, filesize, directory_path, extension
- **Memory**: Stream file reading for hashing large files (future optimization)

## References

- Original requirements: `findex-requirements.md`
- Rust SQLite: rusqlite crate
- Migrations: refinery crate
- Hashing: xxhash-rust crate (xxh3_64)
- Parallel: rayon crate
- CLI: clap crate
