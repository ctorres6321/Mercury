# Mercury

A concurrent, resumable file downloader written in Rust, with inspiration taken from [axel](https://github.com/axel-download-accelerator/axel), splitting a download across multiple simultaneous connections and persisting progress so interrupted downloads can pick up where they left off.

## Features

- **Multi-connection downloads** — splits the target file into byte-range segments and downloads them concurrently.
- **Resumable** — progress is persisted to a `.json` file next to the output; rerunning the same command picks up an interrupted download instead of starting over.
- **Per-segment progress bars** — live download progress for each concurrent connection via `indicatif`.
- **Range-aware probing** — determines file size and range support with a ranged GET rather than trusting `HEAD` since some servers report size unreliably on HEAD requests.

## Usage

```
mercury [OPTIONS] <URL>
```

| Flag | Description | Default |
|---|---|---|
| `-n, --connections <N>` | Number of concurrent connections |Ex: `4` |
| `-o, --output <PATH>` | Output file path, or a directory to download into (filename derived from the URL) | ~/Downloads|

**Examples:**

```sh
# Basic download, default 4 connections, filename inferred from URL
mercury https://example.com/file.mp4

# 8 connections, explicit output path
mercury -n 8 -o ~/Downloads/file.mp4 https://example.com/file.mp4

# Output into an existing directory
mercury -o ~/Downloads/ https://example.com/file.mp4
```

If a download is interrupted, rerunning the exact same command resumes from the last saved progress rather than starting over.

## Building

```sh
cargo build --release
```

### A note for Nix/NixOS users

By default, `reqwest` (via `native-tls`) links against system OpenSSL, which requires `pkg-config` and OpenSSL dev headers to be available — not something NixOS provides globally. This project uses `reqwest`'s `rustls-tls` feature instead, which is a pure-Rust TLS implementation with no system OpenSSL dependency, so `cargo build` should work out of the box without a Nix dev shell.

## Architecture

- **`main.rs`** — CLI entry point: argument parsing, resolving the output path, and orchestrating probe → preallocate → download → cleanup.
- **`args.rs`** — `clap`-derived CLI argument definitions.
- **`progress_state.rs`** — the resumable `DownloadState`/`Segment` types, persisted to disk as JSON via `serde`.
- **`downloader.rs`** — the actual download logic: probing the server, preallocating the output file, and running concurrent ranged-GET downloads per segment with live progress bars.

## Known limitations / TODO

- Re-running the tool against an already-completed download re-downloads the whole file, since the progress file is deleted on success and there's no check against the existing output file yet.
- Output path handling doesn't yet infer a file extension when the URL itself doesn't cleanly end in a filename.
- If a server doesn't honor range requests at all, the tool currently only warns and still attempts a multi-connection download, which will fail per-segment rather than falling back to a genuine single-connection download.

## Dependencies

`clap`, `tokio`, `reqwest` (rustls-tls), `serde`/`serde_json`, `indicatif`, `anyhow`, `futures`
