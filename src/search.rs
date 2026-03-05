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
            r.filename.clone(),
            r.extension.clone().unwrap_or_else(|| "-".to_string()),
            r.file_path.clone(),
            format_size(r.filesize),
            r.hash.clone(),
            format_timestamp(r.created_at),
            format_timestamp(r.modified_at),
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
        output.push_str(&format!("{} {}{}\n", r.file_path, r.hash, dup_str));
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

    #[test]
    fn test_format_size() {
        assert_eq!(format_size(500), "500B");
        assert_eq!(format_size(1024), "1.0K");
        assert_eq!(format_size(1536), "1.5K");
        assert_eq!(format_size(1048576), "1.0M");
        assert_eq!(format_size(1073741824), "1.0G");
    }

    #[test]
    fn test_format_short() {
        let results = vec![
            SearchResult {
                id: 1,
                filename: "test.txt".to_string(),
                extension: Some("txt".to_string()),
                file_path: "/path/to/test.txt".to_string(),
                directory_path: "/path/to".to_string(),
                filesize: 100,
                hash: "abc123def456789a".to_string(),
                created_at: 0,
                modified_at: 0,
                duplicate_count: Some(1),
            },
            SearchResult {
                id: 2,
                filename: "dup.txt".to_string(),
                extension: Some("txt".to_string()),
                file_path: "/path/to/dup.txt".to_string(),
                directory_path: "/path/to".to_string(),
                filesize: 100,
                hash: "abc123def456789a".to_string(),
                created_at: 0,
                modified_at: 0,
                duplicate_count: Some(2),
            },
        ];

        let output = format_short(&results);
        assert!(output.contains("/path/to/test.txt"));
        assert!(output.contains("abc123def456789a"));
        assert!(output.contains("(2 duplicates)"));
    }

    #[test]
    fn test_format_table_empty() {
        let results: Vec<SearchResult> = vec![];
        assert_eq!(format_table(&results), "No results found.");
    }
}
