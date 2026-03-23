mod shell;

use std::process::ExitCode;

use clap::Parser;
use shell::{Cli, run};

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(outcome) => {
            println!("{}", outcome.output);
            ExitCode::from(outcome.exit_code)
        }
        Err(message) => {
            eprintln!("{message}");
            ExitCode::from(2)
        }
    }
}
