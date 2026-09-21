use std::process::ExitCode;

use clap::Parser;
use pomctl::cli::{self, Cli};

fn main() -> ExitCode {
    cli::run(Cli::parse())
}
