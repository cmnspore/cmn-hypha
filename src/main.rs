use std::process::ExitCode;

use hypha::cli;

fn main() -> ExitCode {
    let raw: Vec<String> = std::env::args().collect();
    let _stream_redirect = match agent_first_data::stream_redirect::install_from_raw_args(raw) {
        Ok(installed) => installed,
        Err(err) => {
            let event = agent_first_data::build_cli_error(&err.to_string(), None);
            let output = agent_first_data::render(
                event.as_value(),
                agent_first_data::OutputFormat::Json,
                &agent_first_data::OutputOptions::default(),
            );
            let _ = std::io::Write::write_all(&mut std::io::stdout(), output.as_bytes());
            let _ = std::io::Write::write_all(&mut std::io::stdout(), b"\n");
            return ExitCode::from(2);
        }
    };
    cli::execute(cli::parse_or_exit())
}
