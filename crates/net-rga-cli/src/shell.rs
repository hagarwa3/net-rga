use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};
use net_rga_core::{
    ConfigStore, CorpusConfig, CorpusId, ManifestDb, ProviderConfig, RuntimePaths,
    SearchOutputFormat, SearchRequest, SearchResponse, StateLayout, execute_search,
    export_corpus_bundle, import_corpus_bundle, sync_corpus,
};

#[derive(Debug, Parser)]
#[command(name = "net-rga", about = "Provider-agnostic document search with grep-like affordances")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandOutcome {
    pub output: String,
    pub exit_code: u8,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct SearchRenderOptions {
    stats: bool,
    verbose: bool,
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
    #[arg(short = 'g', long = "glob")]
    pub path_globs: Vec<String>,
    #[arg(long = "type")]
    pub extensions: Vec<String>,
    #[arg(long = "content-type")]
    pub content_types: Vec<String>,
    #[arg(long = "size-min")]
    pub size_min: Option<u64>,
    #[arg(long = "size-max")]
    pub size_max: Option<u64>,
    #[arg(long = "modified-after")]
    pub modified_after: Option<String>,
    #[arg(long = "modified-before")]
    pub modified_before: Option<String>,
    #[arg(long = "max-count")]
    pub limit: Option<u32>,
    #[arg(short = 'F', long = "fixed-strings")]
    pub fixed_strings: bool,
    #[arg(long = "stats")]
    pub stats: bool,
    #[arg(short = 'v', long = "verbose")]
    pub verbose: bool,
    #[arg(long = "json")]
    pub json: bool,
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

pub fn run(cli: Cli) -> Result<CommandOutcome, String> {
    match cli.command {
        Commands::Corpus(corpus) => match corpus.command {
            CorpusSubcommand::Add(args) => handle_corpus_add(args).map(ok_outcome),
            CorpusSubcommand::Remove(args) => handle_corpus_remove(args).map(ok_outcome),
            CorpusSubcommand::List => handle_corpus_list().map(ok_outcome),
        },
        Commands::Sync(args) => handle_sync(args).map(ok_outcome),
        Commands::Search(args) => handle_search(args),
        Commands::Inspect(args) => handle_inspect(args).map(ok_outcome),
        Commands::Export(args) => handle_export(args).map(ok_outcome),
        Commands::Import(args) => handle_import(args).map(ok_outcome),
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
        "synced {}\tpages={}\tlisted={}\tnew={}\tupdated={}\tdeleted={}\tdenied={}\tfailed={}",
        summary.corpus_id,
        summary.pages_processed,
        summary.listed_documents,
        summary.new_documents,
        summary.updated_documents,
        summary.deleted_documents,
        summary.denied_objects,
        summary.failed_objects
    ))
}

fn handle_search(args: SearchArgs) -> Result<CommandOutcome, String> {
    let request = build_search_request(&args);
    let render_options = build_search_render_options(&args);
    let paths = RuntimePaths::from_env().map_err(|error| error.to_string())?;
    handle_search_with_paths(&paths, &request, &render_options)
}

fn handle_inspect(args: InspectArgs) -> Result<String, String> {
    let paths = RuntimePaths::from_env().map_err(|error| error.to_string())?;
    handle_inspect_with_paths(&paths, &args.corpus)
}

fn handle_inspect_with_paths(paths: &RuntimePaths, corpus_id: &str) -> Result<String, String> {
    let store = ConfigStore::new(paths.clone());
    let corpus = store
        .list_corpora()
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|candidate| candidate.id == corpus_id)
        .ok_or_else(|| format!("unknown corpus {corpus_id}"))?;
    let layout = StateLayout::for_corpus(&paths.state_root, &CorpusId(corpus.id.clone()));

    let mut lines = vec![
        format!("corpus={}", corpus.id),
        format!("provider={}", provider_label(&corpus.provider)),
        format!("source={}", provider_source(&corpus.provider)),
        format!("manifest={}", layout.manifest_db.display()),
        format!("index={}", layout.index_dir.join("index.db").display()),
        format!("cache={}", layout.cache_dir.display()),
    ];

    if layout.manifest_db.exists() {
        let manifest = ManifestDb::open(&layout.manifest_db).map_err(|error| error.to_string())?;
        lines.push(format!(
            "documents={}",
            manifest.document_count(&corpus.id).map_err(|error| error.to_string())?
        ));
        lines.push(format!(
            "tombstones={}",
            manifest.tombstone_count(&corpus.id).map_err(|error| error.to_string())?
        ));
        lines.push(format!(
            "failures={}",
            manifest
                .failure_record_count(&corpus.id)
                .map_err(|error| error.to_string())?
        ));
        lines.push(format!(
            "last_sync_started_at={}",
            manifest
                .sync_checkpoint(&corpus.id, "last_sync_started_at")
                .map_err(|error| error.to_string())?
                .unwrap_or_else(|| "-".to_owned())
        ));
        lines.push(format!(
            "last_sync_completed_at={}",
            manifest
                .sync_checkpoint(&corpus.id, "last_sync_completed_at")
                .map_err(|error| error.to_string())?
                .unwrap_or_else(|| "-".to_owned())
        ));
    } else {
        lines.push("documents=unsynced".to_owned());
    }

    lines.push(format!(
        "index_present={}",
        layout.index_dir.join("index.db").exists()
    ));
    lines.push(format!("cache_present={}", layout.cache_dir.exists()));

    Ok(lines.join("\n"))
}

fn handle_export(args: ExportArgs) -> Result<String, String> {
    let paths = RuntimePaths::from_env().map_err(|error| error.to_string())?;
    let manifest = export_corpus_bundle(&paths, &args.corpus, &PathBuf::from(&args.bundle))
        .map_err(|error| error.to_string())?;
    Ok(format!(
        "exported {}\tbundle={}\tindex={}\tcache={}",
        manifest.corpus.id,
        args.bundle,
        manifest.artifacts.index_dir.as_deref().unwrap_or("-"),
        manifest.artifacts.cache_dir.as_deref().unwrap_or("-"),
    ))
}

fn handle_import(args: ImportArgs) -> Result<String, String> {
    let paths = RuntimePaths::from_env().map_err(|error| error.to_string())?;
    let manifest =
        import_corpus_bundle(&paths, &PathBuf::from(&args.bundle)).map_err(|error| error.to_string())?;
    Ok(format!(
        "imported {}\tbundle={}\tindex={}\tcache={}",
        manifest.corpus.id,
        args.bundle,
        manifest.artifacts.index_dir.as_deref().unwrap_or("-"),
        manifest.artifacts.cache_dir.as_deref().unwrap_or("-"),
    ))
}

fn handle_search_with_paths(
    paths: &RuntimePaths,
    request: &SearchRequest,
    render_options: &SearchRenderOptions,
) -> Result<CommandOutcome, String> {
    let response = execute_search(paths, request).map_err(|error| error.to_string())?;
    Ok(CommandOutcome {
        output: render_search_output(&response, render_options)?,
        exit_code: search_exit_code(&response),
    })
}

fn build_search_request(args: &SearchArgs) -> SearchRequest {
    SearchRequest {
        corpus_id: CorpusId(args.corpus.clone()),
        query: args.pattern.clone(),
        fixed_strings: args.fixed_strings,
        path_globs: args.path_globs.clone(),
        extensions: args.extensions.clone(),
        content_types: args.content_types.clone(),
        size_min: args.size_min,
        size_max: args.size_max,
        modified_after: args.modified_after.clone(),
        modified_before: args.modified_before.clone(),
        limit: args.limit,
        output_format: if args.json {
            SearchOutputFormat::Json
        } else {
            SearchOutputFormat::Text
        },
    }
}

fn build_search_render_options(args: &SearchArgs) -> SearchRenderOptions {
    SearchRenderOptions {
        stats: args.stats,
        verbose: args.verbose,
    }
}

fn render_search_text(response: &SearchResponse, render_options: &SearchRenderOptions) -> String {
    let mut lines = render_match_lines(response);
    let partial_notice = render_partial_notice(response);

    if let Some(notice) = partial_notice.as_deref() {
        lines.push(notice.to_owned());
    }
    if render_options.stats {
        if !lines.is_empty() {
            lines.push(String::new());
        }
        lines.extend(render_stats_lines(response));
    }
    if render_options.verbose {
        if !lines.is_empty() {
            lines.push(String::new());
        }
        lines.push(render_verbose_summary(response));
    }

    lines.join("\n")
}

fn render_match_lines(response: &SearchResponse) -> Vec<String> {
    response
        .matches
        .iter()
        .map(|search_match| {
            let path = search_match
                .anchor
                .locator
                .path
                .as_deref()
                .unwrap_or(search_match.document_id.0.as_str());
            match search_match.anchor.locator.line_start {
                Some(line_start) => format!("{path}:{line_start}:{}", search_match.snippet),
                None => format!("{path}:{}", search_match.snippet),
            }
        })
        .collect()
}

fn render_partial_notice(response: &SearchResponse) -> Option<String> {
    if !matches!(response.summary.coverage_status, net_rga_core::CoverageStatus::Partial) {
        return None;
    }

    let counters = nonzero_coverage_counts(response);
    if counters.is_empty() {
        Some("[partial coverage]".to_owned())
    } else {
        Some(format!("[partial coverage: {}]", counters.join(" ")))
    }
}

fn nonzero_coverage_counts(response: &SearchResponse) -> Vec<String> {
    let counts = &response.summary.coverage_counts;
    [
        ("deleted", counts.deleted_count),
        ("denied", counts.denied_count),
        ("stale", counts.stale_count),
        ("unsupported", counts.unsupported_count),
        ("failed", counts.failure_count),
    ]
    .into_iter()
    .filter(|&(_label, value)| value > 0)
    .map(|(label, value)| format!("{label}={value}"))
    .collect()
}

fn render_stats_lines(response: &SearchResponse) -> Vec<String> {
    let matched_documents = response
        .matches
        .iter()
        .map(|search_match| search_match.document_id.0.as_str())
        .collect::<std::collections::BTreeSet<_>>()
        .len();
    let coverage = match response.summary.coverage_status {
        net_rga_core::CoverageStatus::Complete => "complete",
        net_rga_core::CoverageStatus::Partial => "partial",
    };
    let counts = &response.summary.coverage_counts;

    vec![
        format!("{} verified matches", response.summary.verified_matches),
        format!("{} matched anchors", response.matches.len()),
        format!("{matched_documents} documents contained matches"),
        format!("{} candidates considered", response.summary.total_candidates),
        format!("{} candidates fetched", response.summary.fetched_candidates),
        format!("{} indexed candidates", response.summary.indexed_candidates),
        format!("coverage: {coverage}"),
        format!("{} deleted candidates", counts.deleted_count),
        format!("{} denied candidates", counts.denied_count),
        format!("{} stale candidates", counts.stale_count),
        format!("{} unsupported candidates", counts.unsupported_count),
        format!("{} failed candidates", counts.failure_count),
    ]
}

fn render_verbose_summary(response: &SearchResponse) -> String {
    format!(
        "-- summary: corpus={} matches={} candidates={} fetched={} indexed={} coverage={} deleted={} denied={} stale={} unsupported={} failed={}",
        response.summary.corpus_id.0,
        response.summary.verified_matches,
        response.summary.total_candidates,
        response.summary.fetched_candidates,
        response.summary.indexed_candidates,
        match response.summary.coverage_status {
            net_rga_core::CoverageStatus::Complete => "complete",
            net_rga_core::CoverageStatus::Partial => "partial",
        },
        response.summary.coverage_counts.deleted_count,
        response.summary.coverage_counts.denied_count,
        response.summary.coverage_counts.stale_count,
        response.summary.coverage_counts.unsupported_count,
        response.summary.coverage_counts.failure_count,
    )
}

fn render_search_output(response: &SearchResponse, render_options: &SearchRenderOptions) -> Result<String, String> {
    match response.request.output_format {
        SearchOutputFormat::Text => Ok(render_search_text(response, render_options)),
        SearchOutputFormat::Json => serde_json::to_string_pretty(response).map_err(|error| error.to_string()),
    }
}

fn search_exit_code(response: &SearchResponse) -> u8 {
    if matches!(response.summary.coverage_status, net_rga_core::CoverageStatus::Partial) {
        return 3;
    }
    if response.matches.is_empty() {
        1
    } else {
        0
    }
}

fn ok_outcome(output: String) -> CommandOutcome {
    CommandOutcome { output, exit_code: 0 }
}

fn provider_label(provider: &ProviderConfig) -> &'static str {
    match provider {
        ProviderConfig::LocalFs { .. } => "local_fs",
        ProviderConfig::S3 { .. } => "s3",
    }
}

fn provider_source(provider: &ProviderConfig) -> String {
    match provider {
        ProviderConfig::LocalFs { root } => root.display().to_string(),
        ProviderConfig::S3 { bucket, prefix, .. } => match prefix {
            Some(prefix) => format!("{bucket}/{prefix}"),
            None => bucket.clone(),
        },
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use std::env;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    use net_rga_core::{ConfigStore, CorpusConfig, ProviderConfig, RuntimePaths, SearchOutputFormat};

    use super::{
        Cli, Commands, CorpusSubcommand, SearchRenderOptions, build_search_render_options,
        build_search_request, handle_inspect_with_paths, handle_search_with_paths,
        handle_sync_with_paths,
    };

    static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_state_root() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();
        let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        env::temp_dir()
            .join("net-rga-cli-tests")
            .join(format!("state-{}-{nanos}-{counter}", std::process::id()))
    }

    fn snapshot(name: &str) -> String {
        fs::read_to_string(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests")
                .join("snapshots")
                .join(name),
        )
        .map(|value| value.trim_end_matches('\n').to_owned())
        .unwrap_or_else(|error| panic!("snapshot should load: {error}"))
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
        let state_root = temp_state_root();
        let corpus_root = state_root.join("fixtures");
        fs::create_dir_all(corpus_root.join("docs"))
            .unwrap_or_else(|error| panic!("fixture dir should create: {error}"));
        fs::write(corpus_root.join("docs/report.txt"), "riverglass appears here\nother line")
            .unwrap_or_else(|error| panic!("fixture should write: {error}"));

        let paths = RuntimePaths::from_state_root(state_root.clone());
        let store = ConfigStore::new(paths.clone());
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
        handle_sync_with_paths(&paths, "local")
            .unwrap_or_else(|error| panic!("sync should succeed: {error}"));

        let cli = Cli::parse_from(["net-rga", "search", "riverglass", "local"]);
        let request = match cli.command {
            Commands::Search(args) => build_search_request(&args),
            _ => panic!("expected search command"),
        };
        let outcome = handle_search_with_paths(&paths, &request, &SearchRenderOptions::default())
            .unwrap_or_else(|error| panic!("search should render: {error}"));
        assert_eq!(outcome.output, snapshot("search_text.snap"));
        assert_eq!(outcome.exit_code, 0);

        fs::remove_dir_all(state_root).ok();
    }

    #[test]
    fn search_json_output_includes_summary_fields() {
        let state_root = temp_state_root();
        let corpus_root = state_root.join("fixtures");
        fs::create_dir_all(corpus_root.join("docs"))
            .unwrap_or_else(|error| panic!("fixture dir should create: {error}"));
        fs::write(corpus_root.join("docs/report.txt"), "riverglass appears here")
            .unwrap_or_else(|error| panic!("fixture should write: {error}"));

        let paths = RuntimePaths::from_state_root(state_root.clone());
        let store = ConfigStore::new(paths.clone());
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
        handle_sync_with_paths(&paths, "local")
            .unwrap_or_else(|error| panic!("sync should succeed: {error}"));

        let cli = Cli::parse_from(["net-rga", "search", "riverglass", "local", "--json"]);
        let request = match cli.command {
            Commands::Search(args) => build_search_request(&args),
            _ => panic!("expected search command"),
        };
        let output = handle_search_with_paths(&paths, &request, &SearchRenderOptions::default())
            .unwrap_or_else(|error| panic!("json search should render: {error}"));
        assert_eq!(output.output, snapshot("search_json.snap"));
        assert_eq!(output.exit_code, 0);

        fs::remove_dir_all(state_root).ok();
    }

    #[test]
    fn search_args_build_rich_request_model() {
        let cli = Cli::parse_from([
            "net-rga",
            "search",
            "riverglass",
            "local",
            "--glob",
            "docs/**",
            "--type",
            "txt",
            "--content-type",
            "text/plain",
            "--size-min",
            "16",
            "--size-max",
            "4096",
            "--modified-after",
            "1000",
            "--modified-before",
            "2000",
            "--max-count",
            "3",
            "--fixed-strings",
            "--stats",
            "--verbose",
            "--json",
        ]);

        let (request, render_options) = match cli.command {
            Commands::Search(args) => (build_search_request(&args), build_search_render_options(&args)),
            _ => panic!("expected search command"),
        };

        assert_eq!(request.corpus_id.0, "local");
        assert_eq!(request.path_globs, vec!["docs/**"]);
        assert_eq!(request.extensions, vec!["txt"]);
        assert!(request.fixed_strings);
        assert_eq!(request.limit, Some(3));
        assert_eq!(request.output_format, SearchOutputFormat::Json);
        assert!(render_options.stats);
        assert!(render_options.verbose);
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

    #[test]
    fn search_exit_code_is_one_for_complete_no_match() {
        let state_root = temp_state_root();
        let corpus_root = state_root.join("fixtures");
        fs::create_dir_all(corpus_root.join("docs"))
            .unwrap_or_else(|error| panic!("fixture dir should create: {error}"));
        fs::write(corpus_root.join("docs/report.txt"), "different content")
            .unwrap_or_else(|error| panic!("fixture should write: {error}"));

        let paths = RuntimePaths::from_state_root(state_root.clone());
        let store = ConfigStore::new(paths.clone());
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
        handle_sync_with_paths(&paths, "local")
            .unwrap_or_else(|error| panic!("sync should succeed: {error}"));

        let cli = Cli::parse_from(["net-rga", "search", "riverglass", "local"]);
        let request = match cli.command {
            Commands::Search(args) => build_search_request(&args),
            _ => panic!("expected search command"),
        };
        let outcome = handle_search_with_paths(&paths, &request, &SearchRenderOptions::default())
            .unwrap_or_else(|error| panic!("search should render: {error}"));

        assert_eq!(outcome.exit_code, 1);
        assert!(outcome.output.is_empty());

        fs::remove_dir_all(state_root).ok();
    }

    #[test]
    fn search_exit_code_is_three_for_partial_coverage() {
        let state_root = temp_state_root();
        let corpus_root = state_root.join("fixtures");
        fs::create_dir_all(corpus_root.join("docs"))
            .unwrap_or_else(|error| panic!("fixture dir should create: {error}"));
        fs::create_dir_all(corpus_root.join("media"))
            .unwrap_or_else(|error| panic!("fixture dir should create: {error}"));
        fs::write(corpus_root.join("docs/report.txt"), "riverglass appears here")
            .unwrap_or_else(|error| panic!("fixture should write: {error}"));
        fs::write(corpus_root.join("media/video.mp4"), b"binary-data")
            .unwrap_or_else(|error| panic!("fixture should write: {error}"));

        let paths = RuntimePaths::from_state_root(state_root.clone());
        let store = ConfigStore::new(paths.clone());
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
        handle_sync_with_paths(&paths, "local")
            .unwrap_or_else(|error| panic!("sync should succeed: {error}"));

        let cli = Cli::parse_from(["net-rga", "search", "riverglass", "local"]);
        let request = match cli.command {
            Commands::Search(args) => build_search_request(&args),
            _ => panic!("expected search command"),
        };
        let outcome = handle_search_with_paths(&paths, &request, &SearchRenderOptions::default())
            .unwrap_or_else(|error| panic!("search should render: {error}"));

        assert_eq!(outcome.exit_code, 3);
        assert!(outcome.output.contains("[partial coverage: unsupported=1]"));

        fs::remove_dir_all(state_root).ok();
    }

    #[test]
    fn search_stats_output_emits_rg_like_summary_block() {
        let state_root = temp_state_root();
        let corpus_root = state_root.join("fixtures");
        fs::create_dir_all(corpus_root.join("docs"))
            .unwrap_or_else(|error| panic!("fixture dir should create: {error}"));
        fs::write(corpus_root.join("docs/report.txt"), "riverglass appears here")
            .unwrap_or_else(|error| panic!("fixture should write: {error}"));

        let paths = RuntimePaths::from_state_root(state_root.clone());
        let store = ConfigStore::new(paths.clone());
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
        handle_sync_with_paths(&paths, "local")
            .unwrap_or_else(|error| panic!("sync should succeed: {error}"));

        let cli = Cli::parse_from(["net-rga", "search", "riverglass", "local", "--stats"]);
        let (request, render_options) = match cli.command {
            Commands::Search(args) => (build_search_request(&args), build_search_render_options(&args)),
            _ => panic!("expected search command"),
        };
        let outcome = handle_search_with_paths(&paths, &request, &render_options)
            .unwrap_or_else(|error| panic!("search should render: {error}"));

        assert!(outcome.output.contains("1 verified matches"));
        assert!(outcome.output.contains("1 documents contained matches"));
        assert!(outcome.output.contains("coverage: complete"));

        fs::remove_dir_all(state_root).ok();
    }

    #[test]
    fn search_verbose_output_emits_detailed_summary_line() {
        let state_root = temp_state_root();
        let corpus_root = state_root.join("fixtures");
        fs::create_dir_all(corpus_root.join("docs"))
            .unwrap_or_else(|error| panic!("fixture dir should create: {error}"));
        fs::write(corpus_root.join("docs/report.txt"), "riverglass appears here")
            .unwrap_or_else(|error| panic!("fixture should write: {error}"));

        let paths = RuntimePaths::from_state_root(state_root.clone());
        let store = ConfigStore::new(paths.clone());
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
        handle_sync_with_paths(&paths, "local")
            .unwrap_or_else(|error| panic!("sync should succeed: {error}"));

        let cli = Cli::parse_from(["net-rga", "search", "riverglass", "local", "--verbose"]);
        let (request, render_options) = match cli.command {
            Commands::Search(args) => (build_search_request(&args), build_search_render_options(&args)),
            _ => panic!("expected search command"),
        };
        let outcome = handle_search_with_paths(&paths, &request, &render_options)
            .unwrap_or_else(|error| panic!("search should render: {error}"));

        assert!(outcome.output.contains("-- summary: corpus=local"));

        fs::remove_dir_all(state_root).ok();
    }

    #[test]
    fn inspect_reports_manifest_and_sync_state() {
        let state_root = temp_state_root();
        let corpus_root = state_root.join("fixtures");
        fs::create_dir_all(corpus_root.join("docs"))
            .unwrap_or_else(|error| panic!("fixture dir should create: {error}"));
        fs::write(corpus_root.join("docs/report.txt"), "riverglass appears here")
            .unwrap_or_else(|error| panic!("fixture should write: {error}"));

        let paths = RuntimePaths::from_state_root(state_root.clone());
        let store = ConfigStore::new(paths.clone());
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
        handle_sync_with_paths(&paths, "local")
            .unwrap_or_else(|error| panic!("sync should succeed: {error}"));

        let output = handle_inspect_with_paths(&paths, "local")
        .unwrap_or_else(|error| panic!("inspect should succeed: {error}"));

        assert!(output.contains("corpus=local"));
        assert!(output.contains("provider=local_fs"));
        assert!(output.contains("documents=1"));
        assert!(output.contains("last_sync_completed_at="));

        fs::remove_dir_all(state_root).ok();
    }
}
