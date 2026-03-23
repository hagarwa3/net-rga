mod shell;

use std::process::ExitCode;

use clap::Parser;
use shell::{Cli, run};

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(message) => {
            println!("{message}");
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}
