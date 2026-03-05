# Goal
Let's create a Rust app for deriving and indexing metadata for all
files on a user's hard disk allowing for quick file lookups.

## Milestone 1
- Create the sqlite database under ~/.findex/findex.db with the schema
applied via automated migration.
-


# Requirements
Building: all building and testing should be done via Makefile. There must be targets for:
- `make build` build the binary
- `make test` run the unit tests
Language: Rust
Database: SQLite, by default, stored at ~/.findex/findex.db.
Migrations: use DB migrations under source control.
Unit testing: always write unit tests for new functionality.
Phase Implementation: no phase of implementation is done until we have useful unit tests and the unit test pass.
File metadata: for each file under a specified filesystem path, the following should be stored in the files DB table:
- filename - just the name of the file, no directories
- created_at - unix epoch timestamp when the file was created.
- modified_at - unix epoch timestamp when the file was last modified.
- file_path - full system path to the file
- directory_path - full path to the directory in which the file resides
- hash - fast-hash of the file: use xxHash.
- filesize - size of the file in bytes
- extension - filename's extension

## Base functionality
I should be able to run `findex` via commandline. The initial functionality should be as follows:

Start indexing all files descending from a specified directory:
``` shell
findex index /start/path
```

file indexing sequence should be deterministic. if indexing previously
did not complete, resume where we left out. don't worry about new
files created in the meantime if the previous indexing job already
past them up. resuming indexing is more important than picking up the
slack from the last indexing job.

Search for a file and return a list of all relevant entries in the DB:
``` shell
findex search term
```

The term can be a filename, a path, a hash, a filesize. Return all
entries to the term. E.g. if i specify a path, search the DB for the
expected path, and the filename in the path, and if there currently
exists a file at that path, hash the file and search the DB for the
hash.

# Tips
- Ask clarifying questions.
- Indexing speed is more important than being bullet proof.
- query speed is important, use appropriate SQLite indexes.
- Small changes - keep the changes small. ensure unit tests pass. git
  commit each change with a useful concise commit message.
- whenever there's a new build, test, debug, etc. command, create a
  `make` target for it if it does not exist and execute via the make
  target.
