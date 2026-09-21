mod downloader;
mod progress_state;
mod args;
use anyhow::{Context, Result};
use progress_state::DownloadState;
use reqwest::Client;
use std::sync::Arc;
use tokio::sync::Mutex;
use args::Args;
use clap::Parser;

use downloader::default_output_path;

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();


    // TODO: Refactor into stronger directory checking and more robust path checking with downloader and progress_state
    let output_path = match args.output {
        Some(path) if path.is_dir() => path.join(default_output_path(&args.url)), // If user specifies just a directory, download the file there and join the path
        Some(path) => path, // Otherwise, if user specifies a full file path with the name of file included then we don't join the paths
        None => default_output_path(&args.url), // If no -o flag is given at all
    };

    let progress_path = DownloadState::progress_file_for(&output_path);

    let client = Client::new();

    let state = match DownloadState::load(&progress_path)? {

        // Resume if a progress file already exists for this output
        Some(existing) => {
            println!("Resuming existing download: {}", output_path.display());
            existing
        }

        // Otherwise, probe the server and start a fresh download record.
        _ => {
            println!("Starting new download: {}", args.url);
            let (total_size, accepts_ranges) = downloader::probe(&client, &args.url).await?;

            if !accepts_ranges {
                println!(
                    "Warning: server does not advertise range support; \
                     proceeding anyway, but resume/multi-connection may not work."
                );
            }

            downloader::preallocate_file(&output_path, total_size)
                .await
                .context("failed to preallocate output file")?;

            let state = DownloadState::new(
                args.url.clone(),
                output_path.clone(),
                total_size,
                args.connections,
            );
            state.save(&progress_path)?;
            state
        }
    };


    // TODO: Come back to fix already downloaded files being redownloaded again
    if state.is_complete() {
        println!("Download already complete: {}", output_path.display());
        return Ok(());
    }

    let state = Arc::new(Mutex::new(state));
    downloader::run_download(client, Arc::clone(&state), progress_path.clone()).await?;

    let final_state = state.lock().await;

    if final_state.is_complete() {
        println!("Download complete: {}", final_state.output_path.display());
        // Clean up the progress file now that we're done.
        let _ = std::fs::remove_file(&progress_path);
    } else {
        println!("Download did not finish; rerun the same command to resume.");
    }

    Ok(())
}