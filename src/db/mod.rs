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
