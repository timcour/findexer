mod cli;
mod db;
mod discovery;
mod file_meta;
mod indexer;
mod search;
mod state;

use clap::Parser;
use cli::{Cli, Commands};
use indicatif::{ProgressBar, ProgressStyle};
use std::cell::Cell;
use std::time::{Duration, Instant};

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

    // Setup progress bar with message template
    let pb = ProgressBar::new(files.len() as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} ({eta})\n{msg}")
            .unwrap()
            .progress_chars("#>-"),
    );

    // Throttle progress updates to every 50ms
    let last_update = Cell::new(Instant::now());
    let update_interval = Duration::from_millis(50);

    // Index with progress updates
    let result = indexer::index_directory(&mut conn, path, batch_size, Some(|update: indexer::ProgressUpdate| {
        let now = Instant::now();
        if now.duration_since(last_update.get()) >= update_interval {
            pb.set_position(update.files_completed as u64);
            pb.set_message(update.current_file.display().to_string());
            last_update.set(now);
        }
    }))?;

    pb.set_position(pb.length().unwrap_or(0));
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
