mod shell;

use clap::Parser;
use shell::{Cli, run};

fn main() {
    let cli = Cli::parse();
    println!("{}", run(cli));
}

