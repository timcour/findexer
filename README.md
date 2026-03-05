# findex

A fast file indexer and search tool written in Rust. Index metadata for all files on your filesystem and search by filename, path, hash, or filesize.

## Features

- **Fast parallel indexing** - Uses rayon for parallel file hashing
- **Resume support** - Interrupted indexing jobs resume where they left off
- **Multiple search modes** - Search by filename, path, hash (xxHash), or filesize
- **Duplicate detection** - Identifies files with identical content via hash matching
- **SQLite storage** - Persistent index with efficient queries
- **Progress reporting** - Real-time progress bar with current file display

## Installation

```bash
# Clone the repository
git clone https://github.com/yourusername/findex.git
cd findex

# Build release binary
make build

# Binary is at ./target/release/findex
```

## Usage

### Index a directory

```bash
# Index all files under a path
findex index /path/to/directory

# With custom batch size (default: 1000)
findex index /path/to/directory --batch-size 500
```

The index is stored at `~/.findex/findex.db`.

### Search indexed files

```bash
# Search by filename (partial match)
findex search readme

# Search by path (partial match)
findex search /projects/myapp

# Search by exact hash (16 hex characters)
findex search abc123def456789a

# Search by filesize in bytes
findex search 1024

# Short output format (filepath, hash, duplicate count)
findex search --short readme
```

### Example output

**Default table format:**
```
+-------------+-----------+---------------------------+-------+------------------+---------+----------+
| Filename    | Extension | Path                      | Size  | Hash             | Created | Modified |
+-------------+-----------+---------------------------+-------+------------------+---------+----------+
| readme.txt  | txt       | /home/user/readme.txt     | 1.0K  | abc123def456789a | 2024    | 2024     |
| readme.md   | md        | /home/user/docs/readme.md | 2.0K  | fff000fff000fff0 | 2024    | 2024     |
+-------------+-----------+---------------------------+-------+------------------+---------+----------+
```

**Short format (`--short`):**
```
/home/user/readme.txt abc123def456789a
/home/user/copy.txt abc123def456789a (2 duplicates)
```

## Development

### Prerequisites

- Rust 1.70+ (install via [rustup](https://rustup.rs/))
- Make

### Build and test

```bash
# Build release binary
make build

# Run tests
make test

# Clean build artifacts
make clean
```

### Project structure

```
src/
  main.rs         # CLI entry point
  cli.rs          # Command-line argument parsing (clap)
  db/
    mod.rs        # SQLite connection and migrations
    migrations/   # SQL migration files
  discovery.rs    # File system traversal
  file_meta.rs    # File metadata extraction and hashing
  indexer.rs      # Parallel indexing with resume support
  search.rs       # Search queries and result formatting
  state.rs        # Index state persistence for resume
```

### Database schema

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
    indexed_at INTEGER NOT NULL
);
```

Indexes exist on `filename`, `hash`, `filesize`, `directory_path`, and `extension` for fast queries.

## Configuration

| Setting | Location | Description |
|---------|----------|-------------|
| Database | `~/.findex/findex.db` | SQLite database file |
| State file | `~/.findex/index_state.json` | Resume state (deleted after successful completion) |

## Recommended additional sections

Consider adding these sections as the project evolves:

- **Benchmarks** - Performance metrics for indexing speed and search latency
- **Changelog** - Version history and release notes
- **Contributing** - Guidelines for contributors
- **License** - Software license (MIT, Apache 2.0, etc.)
- **Roadmap** - Planned features (file watching, content search, etc.)
- **FAQ** - Common questions and troubleshooting
- **Comparison** - How findex compares to similar tools (locate, mlocate, fd, etc.)

## License

[Choose a license - MIT, Apache 2.0, GPL, etc.]
