use crate::progress_state::{DownloadState, Segment};
use anyhow::{anyhow, Context, Result};
use futures::stream::StreamExt;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use reqwest::Client;
use std::path::Path;
use std::sync::Arc;
use tokio::fs::OpenOptions;
use tokio::io::{AsyncSeekExt, AsyncWriteExt, SeekFrom};
use tokio::sync::Mutex;
use std::path::PathBuf;

/// Gets our default download path
pub fn default_output_path(url: &str) -> PathBuf {
    url.rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("downloaded_file")) // TODO: Parse link for correct extension if output path throws an error
}

/// Figure out the remote file size and whether it supports byte ranges.
pub async fn probe(client: &Client, url: &str) -> Result<(u64, bool)> {
    let resp = client
        .get(url)
        .header(reqwest::header::RANGE, "bytes=0-0")
        .send()
        .await
        .context("probe GET request failed")?;

    // 206 Partial Content means the server honored the range request, which
    // both confirms range support and lets us read the real total size 
    if resp.status() == reqwest::StatusCode::PARTIAL_CONTENT {
        let total_size = resp
            .headers()
            .get(reqwest::header::CONTENT_RANGE) // From the header get content range
            .and_then(|v| v.to_str().ok()) // Then we turn it into a string
            .and_then(|s| s.rsplit('/').next()) // We then split the second half since headers look like : bytes 0-1023/146515
            .and_then(|s| s.parse::<u64>().ok()) // We parse the second part
            .ok_or_else(|| anyhow!("server sent 206 but no usable Content-Range"))?; // Return our result or throw an error

        return Ok((total_size, true));
    }

    // Server ignored the Range header and sent the whole file (200 OK) —
    // fall back to Content-Length, but treat ranges as unsupported.
    // TODO: Implement fall back to single connection download if ranges are not supported by server
    let total_size = resp
        .content_length()
        .ok_or_else(|| anyhow!("server did not return Content-Length"))?;

    Ok((total_size, false))
}

/// Pre-allocate the output file to its final size so each task can seek+write
/// its own region independently.
pub async fn preallocate_file(path: &Path, size: u64) -> Result<()> {

    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .open(path)
        .await
        .context("failed to create output file")?;

    file.set_len(size).await.context("failed to preallocate file size")?;
    Ok(())
}

/// Makes our progress bar and keeps our bar logic outside the actual download implementation
pub fn make_progress_bar (multi: &MultiProgress, idx: usize, segment: &Segment) -> Result<ProgressBar> {
    
    let style = ProgressStyle::with_template(
        "{prefix:>3} [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({bytes_per_sec})",
    )?.progress_chars("=>-");

    let bar = multi.add(ProgressBar::new(segment.end - segment.start + 1));
    bar.set_style(style.clone());
    bar.set_prefix(format!("#{idx}"));
    bar.set_position(segment.downloaded);

    Ok(bar)
}


/// Run all segment downloads concurrently, saving progress periodically so
/// the download can be resumed if interrupted.
pub async fn run_download(
    client: Client,
    state: Arc<Mutex<DownloadState>>,
    progress_path: std::path::PathBuf,
) -> Result<()> {

    let multi = MultiProgress::new(); // Make our multibar for so we can add our progress bars in the loop

    let (url, output_path, segments) = {
        let s = state.lock().await;
        (s.url.clone(), s.output_path.clone(), s.segments.clone())
    };

    let mut tasks = Vec::new();

    for (idx, segment) in segments.into_iter().enumerate() {
        if segment.is_complete() {
            continue; // nothing to do for this segment on resume
        }

        let bar = make_progress_bar(&multi, idx, &segment)?;

        let client = client.clone();
        let url = url.clone();
        let output_path = output_path.clone();
        let state = Arc::clone(&state);
        let progress_path = progress_path.clone();

        tasks.push(tokio::spawn(async move {
            download_segment(client, url, output_path, idx, segment, bar, state, progress_path)
                .await
        }));
    }

    for task in tasks {
        task.await.context("segment task panicked")??; // Error if one of our tokio tasks panics
    }

    multi.clear()?;
    Ok(())
}

/// Download a single byte-range segment, writing directly into its region of
/// the output file and periodically persisting progress for resume support.
async fn download_segment(
    client: Client,
    url: String,
    output_path: std::path::PathBuf,
    idx: usize,
    mut segment: Segment,
    bar: ProgressBar,
    state: Arc<Mutex<DownloadState>>,
    progress_path: std::path::PathBuf,
) -> Result<()> {

    let range_header = format!("bytes={}-{}", segment.remaining_start(), segment.end);

    let resp = client
        .get(&url)
        .header(reqwest::header::RANGE, range_header)
        .send()
        .await
        .context("range request failed")?;


    if !resp.status().is_success() {
        return Err(anyhow!("segment {idx} got status {}", resp.status())); 
    }

    let mut file = OpenOptions::new()
        .write(true)
        .open(&output_path)
        .await
        .context("failed to open output file for writing")?;

    file.seek(SeekFrom::Start(segment.remaining_start())).await?;

    let mut stream = resp.bytes_stream();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.context("error while reading response stream")?;
        file.write_all(&chunk).await?;

        segment.downloaded += chunk.len() as u64;
        bar.set_position(segment.downloaded);

        // Persist progress every so often rather than on every chunk, to
        // avoid hammering disk I/O with JSON writes and let us resume on failure.
        if segment.downloaded % (1024 * 1024) < chunk.len() as u64 {
            let mut s = state.lock().await;
            s.segments[idx] = segment.clone();
            s.save(&progress_path)?;
        }
    }

    // Final save for this segment so a mid-stream interruption right after
    // the last chunk still records completion.

    { // Braces create a lock
        let mut s = state.lock().await;
        s.segments[idx] = segment.clone();
        s.save(&progress_path)?;
    } // Lock is released here


    bar.finish_with_message("Done!");
    Ok(())
}