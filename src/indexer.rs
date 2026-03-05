use crate::db::DbError;
use crate::file_meta::FileMeta;
use crate::state::{IndexState, StateError};
use rayon::prelude::*;
use rusqlite::Connection;
use std::path::{Path, PathBuf};
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

/// Progress update sent during indexing
pub struct ProgressUpdate {
    pub current_file: PathBuf,
    pub files_completed: usize,
    pub total_files: usize,
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
///
/// The optional `progress_callback` is called for each file processed.
pub fn index_directory<F>(
    conn: &mut Connection,
    root: &Path,
    batch_size: usize,
    mut progress_callback: Option<F>,
) -> Result<IndexResult, IndexError>
where
    F: FnMut(ProgressUpdate),
{
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
    let total_files = all_files.len();

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

    let mut files_completed = result.files_skipped;

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
            files_completed += 1;

            // Report progress
            if let Some(ref mut callback) = progress_callback {
                callback(ProgressUpdate {
                    current_file: path.clone(),
                    files_completed,
                    total_files,
                });
            }
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

        let result = index_directory::<fn(ProgressUpdate)>(&mut conn, tmp_files.path(), 10, None).unwrap();

        assert_eq!(result.files_processed, 2);
        assert_eq!(result.errors, 0);

        let count: i32 = conn
            .query_row("SELECT COUNT(*) FROM files", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn test_index_directory_with_nested() {
        let tmp_files = TempDir::new().unwrap();
        std::fs::write(tmp_files.path().join("a.txt"), "content a").unwrap();
        std::fs::create_dir(tmp_files.path().join("subdir")).unwrap();
        std::fs::write(tmp_files.path().join("subdir/b.txt"), "content b").unwrap();

        let (_tmp_db, mut conn) = setup_test_db();

        let result = index_directory::<fn(ProgressUpdate)>(&mut conn, tmp_files.path(), 10, None).unwrap();

        assert_eq!(result.files_processed, 2);

        // Verify the paths are stored correctly
        let paths: Vec<String> = {
            let mut stmt = conn.prepare("SELECT file_path FROM files ORDER BY file_path").unwrap();
            stmt.query_map([], |row| row.get(0))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap()
        };

        assert_eq!(paths.len(), 2);
        assert!(paths[0].ends_with("a.txt"));
        assert!(paths[1].ends_with("subdir/b.txt"));
    }

    #[test]
    fn test_index_directory_with_progress() {
        let tmp_files = TempDir::new().unwrap();
        std::fs::write(tmp_files.path().join("a.txt"), "content a").unwrap();
        std::fs::write(tmp_files.path().join("b.txt"), "content b").unwrap();

        let (_tmp_db, mut conn) = setup_test_db();

        let mut progress_count = 0;
        let result = index_directory(
            &mut conn,
            tmp_files.path(),
            10,
            Some(|_update: ProgressUpdate| {
                progress_count += 1;
            }),
        ).unwrap();

        assert_eq!(result.files_processed, 2);
        assert_eq!(progress_count, 2);
    }
}
