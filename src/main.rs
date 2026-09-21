use std::process::ExitCode;

fn main() -> ExitCode {
    pomctl::cli::run(std::env::args().skip(1))
}
