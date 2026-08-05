use std::process::ExitCode;

use hypha::cli;

fn main() -> ExitCode {
    let (args, _stream_redirect) = cli::parse_or_exit();
    cli::execute(args)
}
