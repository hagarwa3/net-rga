use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};
use net_rga_core::{ConfigStore, CorpusConfig, ProviderConfig, RuntimePaths, sync_corpus};

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
        Commands::Sync(args) => handle_sync(args),
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

fn handle_sync(args: SyncArgs) -> Result<String, String> {
    let paths = RuntimePaths::from_env().map_err(|error| error.to_string())?;
    handle_sync_with_paths(&paths, &args.corpus)
}

fn handle_sync_with_paths(paths: &RuntimePaths, corpus: &str) -> Result<String, String> {
    let summary = sync_corpus(paths, corpus).map_err(|error| error.to_string())?;
    Ok(format!(
        "synced {}\tpages={}\tlisted={}",
        summary.corpus_id, summary.pages_processed, summary.listed_documents
    ))
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use std::env;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use net_rga_core::{ConfigStore, CorpusConfig, ProviderConfig, RuntimePaths};

    use super::{Cli, Commands, CorpusSubcommand, handle_sync_with_paths, run};

    fn temp_state_root() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();
        env::temp_dir().join("net-rga-cli-tests").join(format!("state-{nanos}"))
    }

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

    #[test]
    fn sync_command_runs_real_sync_path() {
        let state_root = temp_state_root();
        let corpus_root = state_root.join("fixtures");
        fs::create_dir_all(corpus_root.join("docs"))
            .unwrap_or_else(|error| panic!("fixture dir should create: {error}"));
        fs::write(corpus_root.join("docs/report.txt"), "riverglass")
            .unwrap_or_else(|error| panic!("fixture should write: {error}"));

        let paths = RuntimePaths::from_state_root(state_root.clone());
        let store = ConfigStore::new(paths);
        store
            .add_corpus(CorpusConfig {
                id: "local".to_owned(),
                display_name: Some("Local".to_owned()),
                provider: ProviderConfig::LocalFs {
                    root: corpus_root.clone(),
                },
                include_globs: Vec::new(),
                exclude_globs: Vec::new(),
                backend: None,
            })
            .unwrap_or_else(|error| panic!("corpus should save: {error}"));

        let output = handle_sync_with_paths(&RuntimePaths::from_state_root(state_root.clone()), "local")
            .unwrap_or_else(|error| panic!("sync should succeed: {error}"));
        assert!(output.contains("synced local"));
        fs::remove_dir_all(state_root).ok();
    }
}
