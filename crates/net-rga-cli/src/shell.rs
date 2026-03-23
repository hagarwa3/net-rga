use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};
use net_rga_core::{ConfigStore, CorpusConfig, ProviderConfig, RuntimePaths};

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
    #[arg(long, value_enum)]
    pub provider: ProviderArg,
    #[arg(long)]
    pub root: Option<PathBuf>,
    #[arg(long)]
    pub bucket: Option<String>,
    #[arg(long)]
    pub prefix: Option<String>,
    #[arg(long)]
    pub region: Option<String>,
    #[arg(long)]
    pub endpoint: Option<String>,
    #[arg(long)]
    pub profile: Option<String>,
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

#[derive(Clone, Debug, ValueEnum)]
pub enum ProviderArg {
    LocalFs,
    S3,
}

pub fn run(cli: Cli) -> Result<String, String> {
    match cli.command {
        Commands::Corpus(corpus) => match corpus.command {
            CorpusSubcommand::Add(args) => handle_corpus_add(args),
            CorpusSubcommand::Remove(args) => handle_corpus_remove(args),
            CorpusSubcommand::List => handle_corpus_list(),
        },
        Commands::Sync(args) => Ok(format!("placeholder: sync {}", args.corpus)),
        Commands::Search(args) => Ok(format!("placeholder: search {} {}", args.pattern, args.corpus)),
        Commands::Inspect(args) => Ok(format!("placeholder: inspect {}", args.corpus)),
        Commands::Export(args) => Ok(format!("placeholder: export {} {}", args.corpus, args.bundle)),
        Commands::Import(args) => Ok(format!("placeholder: import {}", args.bundle)),
    }
}

fn handle_corpus_add(args: CorpusAddArgs) -> Result<String, String> {
    let provider = match args.provider {
        ProviderArg::LocalFs => {
            let root = args
                .root
                .ok_or_else(|| "--root is required for --provider local-fs".to_owned())?;
            ProviderConfig::LocalFs { root }
        }
        ProviderArg::S3 => {
            let bucket = args
                .bucket
                .ok_or_else(|| "--bucket is required for --provider s3".to_owned())?;
            ProviderConfig::S3 {
                bucket,
                prefix: args.prefix,
                region: args.region,
                endpoint: args.endpoint,
                profile: args.profile,
            }
        }
    };

    let store = ConfigStore::new(RuntimePaths::from_env().map_err(|error| error.to_string())?);
    store
        .add_corpus(CorpusConfig {
            id: args.name.clone(),
            display_name: Some(args.name.clone()),
            provider,
            include_globs: Vec::new(),
            exclude_globs: Vec::new(),
            backend: None,
        })
        .map_err(|error| error.to_string())?;

    Ok(format!("added corpus {}", args.name))
}

fn handle_corpus_remove(args: CorpusRemoveArgs) -> Result<String, String> {
    let store = ConfigStore::new(RuntimePaths::from_env().map_err(|error| error.to_string())?);
    store
        .remove_corpus(&args.name)
        .map_err(|error| error.to_string())?;
    Ok(format!("removed corpus {}", args.name))
}

fn handle_corpus_list() -> Result<String, String> {
    let store = ConfigStore::new(RuntimePaths::from_env().map_err(|error| error.to_string())?);
    let corpora = store.list_corpora().map_err(|error| error.to_string())?;
    if corpora.is_empty() {
        return Ok("no corpora configured".to_owned());
    }

    let lines = corpora
        .into_iter()
        .map(|corpus| match corpus.provider {
            ProviderConfig::LocalFs { root } => format!("{}\tlocal_fs\t{}", corpus.id, root.display()),
            ProviderConfig::S3 {
                bucket,
                prefix,
                endpoint,
                ..
            } => {
                let suffix = prefix.unwrap_or_default();
                let endpoint_text = endpoint.unwrap_or_else(|| "aws".to_owned());
                format!("{}\ts3\t{}:{}\t{}", corpus.id, bucket, suffix, endpoint_text)
            }
        })
        .collect::<Vec<_>>();

    Ok(lines.join("\n"))
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::{Cli, Commands, CorpusSubcommand, run};

    #[test]
    fn parses_corpus_add_subcommand() {
        let cli = Cli::parse_from([
            "net-rga",
            "corpus",
            "add",
            "local",
            "--provider",
            "local-fs",
            "--root",
            "/data",
        ]);
        match cli.command {
            Commands::Corpus(command) => match command.command {
                CorpusSubcommand::Add(args) => {
                    assert_eq!(args.name, "local");
                    assert!(args.root.is_some());
                }
                _ => panic!("expected corpus add"),
            },
            _ => panic!("expected corpus subcommand"),
        }
    }

    #[test]
    fn renders_placeholder_search_output() {
        let cli = Cli::parse_from(["net-rga", "search", "riverglass", "local"]);
        let output = run(cli).unwrap_or_else(|error| panic!("search placeholder should render: {error}"));
        assert_eq!(output, "placeholder: search riverglass local");
    }
}
