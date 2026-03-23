use clap::{Args, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "net-rga", about = "Provider-agnostic document search with grep-like affordances")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    Corpus(CorpusCommand),
    Sync(SyncArgs),
    Search(SearchArgs),
    Inspect(InspectArgs),
    Export(ExportArgs),
    Import(ImportArgs),
}

#[derive(Debug, Args)]
pub struct CorpusCommand {
    #[command(subcommand)]
    pub command: CorpusSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum CorpusSubcommand {
    Add(CorpusAddArgs),
    Remove(CorpusRemoveArgs),
    List,
}

#[derive(Debug, Args)]
pub struct CorpusAddArgs {
    pub name: String,
}

#[derive(Debug, Args)]
pub struct CorpusRemoveArgs {
    pub name: String,
}

#[derive(Debug, Args)]
pub struct SyncArgs {
    pub corpus: String,
}

#[derive(Debug, Args)]
pub struct SearchArgs {
    pub pattern: String,
    pub corpus: String,
}

#[derive(Debug, Args)]
pub struct InspectArgs {
    pub corpus: String,
}

#[derive(Debug, Args)]
pub struct ExportArgs {
    pub corpus: String,
    pub bundle: String,
}

#[derive(Debug, Args)]
pub struct ImportArgs {
    pub bundle: String,
}

pub fn run(cli: Cli) -> String {
    match cli.command {
        Commands::Corpus(corpus) => match corpus.command {
            CorpusSubcommand::Add(args) => format!("placeholder: corpus add {}", args.name),
            CorpusSubcommand::Remove(args) => format!("placeholder: corpus remove {}", args.name),
            CorpusSubcommand::List => "placeholder: corpus list".to_owned(),
        },
        Commands::Sync(args) => format!("placeholder: sync {}", args.corpus),
        Commands::Search(args) => format!("placeholder: search {} {}", args.pattern, args.corpus),
        Commands::Inspect(args) => format!("placeholder: inspect {}", args.corpus),
        Commands::Export(args) => format!("placeholder: export {} {}", args.corpus, args.bundle),
        Commands::Import(args) => format!("placeholder: import {}", args.bundle),
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::{Cli, Commands, CorpusSubcommand, run};

    #[test]
    fn parses_corpus_add_subcommand() {
        let cli = Cli::parse_from(["net-rga", "corpus", "add", "local"]);
        match cli.command {
            Commands::Corpus(command) => match command.command {
                CorpusSubcommand::Add(args) => assert_eq!(args.name, "local"),
                _ => panic!("expected corpus add"),
            },
            _ => panic!("expected corpus subcommand"),
        }
    }

    #[test]
    fn renders_placeholder_search_output() {
        let cli = Cli::parse_from(["net-rga", "search", "riverglass", "local"]);
        assert_eq!(run(cli), "placeholder: search riverglass local");
    }
}

