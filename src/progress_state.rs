use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// One chunk of the file being downloaded.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Segment {
    pub start: u64,
    pub end: u64,        // inclusive
    pub downloaded: u64, // bytes downloaded so far within this segment
}

impl Segment {
    pub fn remaining_start(&self) -> u64 {
        self.start + self.downloaded
    }

    pub fn is_complete(&self) -> bool {
        self.downloaded >= (self.end - self.start + 1)
    }
}

/// The full on-disk progress record for one download.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadState {
    pub url: String,
    pub output_path: PathBuf,
    pub total_size: u64,
    pub segments: Vec<Segment>,
}

impl DownloadState {
    pub fn new(url: String, output_path: PathBuf, total_size: u64, num_connections: u64) -> Self {
        let segments = Self::split_into_segments(total_size, num_connections);
        Self {
            url,
            output_path,
            total_size,
            segments,
        }
    }

    /// Return the path to the json file for a given output file that we are downloading
    pub fn progress_file_for(output_path: &Path) -> PathBuf {
        let mut p = output_path.as_os_str().to_owned();
        p.push(".json");
        PathBuf::from(p)
    }

    /// Load a DownloadState from disk, if it exists.
    pub fn load(path: &Path) -> Result<Option<Self>> {
        // If the progress file doesn't exist, return None rather than an error
        if !path.exists() {
            return Ok(None);
        }

        let data = std::fs::read_to_string(path)?;

        // If the file exists we read from it and continue
        let state: DownloadState = serde_json::from_str(&data)?;
        Ok(Some(state))
    }

    /// Save the DownloadState to disk as a JSON file.
    pub fn save(&self, path: &Path) -> Result<()> {
        let data = serde_json::to_string_pretty(self)?;
        std::fs::write(path, data)?;
        Ok(())
    }

    /// Check if all segments in the vector of segments the are complete  
    pub fn is_complete(&self) -> bool {
        self.segments.iter().all(|s| s.is_complete())
    }

    /// Returns a vector of the segements so we can download them.
    /// We specify the amount of connecitions to use and the total size of the file to download.
    pub fn split_into_segments(total_size: u64, num_connections: u64) -> Vec<Segment> {
    // Ensure that we do not have not have more connections than the total size of the file, and that we have at least one connection.
    // If not then we get an error since we get an underflow other wise.
    let num_connections = num_connections.max(1).min(total_size.max(1));

    let chunk_size = total_size / num_connections;
    let mut segments = Vec::with_capacity(num_connections as usize);

    for i in 0..num_connections {
        let start = i * chunk_size;

        let end = if i == num_connections - 1 {
            total_size.saturating_sub(1) // last chunk absorbs the remainder
        } 
        else {
            start.saturating_add(chunk_size).saturating_sub(1)
        };
        segments.push(Segment {
            start,
            end,
            downloaded: 0,
        });
    }
        segments
    }
}