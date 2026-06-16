use std::process::ExitCode;

use hypha::cli;

fn main() -> ExitCode {
    cli::execute(cli::parse_or_exit())
}
