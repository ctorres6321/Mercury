use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "Mercury", about = "Concurrent, resumable file downloader")]
pub struct Args {
    /// URL of the file to download
    pub url: String,

    /// Output file path (defaults to the filename from the URL)
    #[arg(short = 'o', long)]
    pub output: Option<PathBuf>,

    /// Number of concurrent connections
    #[arg(short = 'n', long, default_value_t = 4)]
    pub connections: u64,
}