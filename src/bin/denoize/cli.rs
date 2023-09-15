use std::path::PathBuf;

use clap::Parser;

/// The CLI for the denoize subcommand
#[derive(Parser)]
pub struct Cli {
    #[clap(short, long)]
    pub smooth: Option<f64>,

    #[clap(short='N', long)]
    pub serie: usize,

    pub filename: PathBuf,
}
