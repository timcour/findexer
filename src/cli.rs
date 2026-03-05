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
