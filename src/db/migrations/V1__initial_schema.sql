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
