use clap::Parser;
use std::path::PathBuf;

/// Try to approximate fragments of a file
#[derive(Parser)]
#[clap(version)]
pub struct Cli {
    /// The json-file with captured by `laser-precision-adjust-server`
    pub json_file: PathBuf,
}
