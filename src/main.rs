//! Container entry: read `/work/request.json`, Skadi mirror under `SPLAT_CACHE`, write SPLAT-shaped outputs.

use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

mod dem;
mod skadi_fetch;
mod engine;
mod hash;
mod kml;
mod lora;
mod ppm;
mod ray_cache;

use engine::{run_batch_coverage, run_coverage};
use hash::{splat_input_sha256, Request as CovRequest};

#[derive(Parser)]
#[command(name = "splatter")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run one coverage request from ``request.json`` (object) into ``--work-dir``.
    Run {
        #[arg(long, default_value = "/work")]
        work_dir: PathBuf,
        /// Progress messages on stderr (mirror, tiles, raster phases).
        #[arg(short, long)]
        verbose: bool,
    },
    /// Run many requests from ``request.json`` (array); writes ``<work-dir>/<digest>/`` per entry.
    RunBatch {
        #[arg(long, default_value = "/work")]
        work_dir: PathBuf,
        #[arg(short, long)]
        verbose: bool,
    },
    /// Print ``splat_input_sha256`` for a ``request.json`` (host parity checks).
    InputSha256 {
        #[arg(long)]
        request: PathBuf,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Run {
            work_dir,
            verbose,
        } => run_coverage(&work_dir, verbose)
            .with_context(|| format!("run coverage work_dir={}", work_dir.display())),
        Command::RunBatch {
            work_dir,
            verbose,
        } => run_batch_coverage(&work_dir, verbose)
            .with_context(|| format!("run batch work_dir={}", work_dir.display())),
        Command::InputSha256 { request } => {
            let raw =
                fs::read_to_string(&request).with_context(|| request.display().to_string())?;
            let parsed: CovRequest =
                serde_json::from_str(&raw).with_context(|| format!("{}", request.display()))?;
            println!("{}", splat_input_sha256(&parsed)?);
            Ok(())
        }
    }
}
