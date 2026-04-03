use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use net_rga_core::{
    ConfigStore, CorpusConfig, CorpusId, ManifestDb, ProviderConfig, RuntimePaths,
    SearchOutputFormat, SearchRequest, build_index, execute_search, export_corpus_bundle,
    import_corpus_bundle, sync_corpus,
};

#[test]
fn bundle_restore_bootstraps_clean_environment_with_search_ready_state() {
    let source_state_root = temp_dir("net-rga-bundle-source");
    let imported_state_root = temp_dir("net-rga-bundle-imported");
    let corpus_root = temp_dir("net-rga-bundle-corpus");
    let bundle_root = temp_dir("net-rga-bundle-output");

    write_fixture(
        &corpus_root,
        "docs/report.txt",
        "riverglass appears here\nsecond line",
    );

    let source_paths = RuntimePaths::from_state_root(source_state_root.clone());
    let source_store = ConfigStore::new(source_paths.clone());
    source_store
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
        .unwrap_or_else(|error| panic!("source corpus should save: {error}"));

    sync_corpus(&source_paths, "local")
        .unwrap_or_else(|error| panic!("source sync should succeed: {error}"));
    let provider = net_rga_core::providers::provider_from_config(&ProviderConfig::LocalFs {
        root: corpus_root.clone(),
    })
    .unwrap_or_else(|error| panic!("local provider should build: {error}"));
    build_index(&source_paths, "local", provider.as_ref())
        .unwrap_or_else(|error| panic!("source index build should succeed: {error}"));
    let first_response = execute_search(
        &source_paths,
        &SearchRequest {
            corpus_id: CorpusId("local".to_owned()),
            query: "riverglass".to_owned(),
            fixed_strings: true,
            path_globs: Vec::new(),
            extensions: Vec::new(),
            content_types: Vec::new(),
            size_min: None,
            size_max: None,
            modified_after: None,
            modified_before: None,
            limit: None,
            output_format: SearchOutputFormat::Json,
        },
    )
    .unwrap_or_else(|error| panic!("source search should succeed: {error}"));

    assert_eq!(first_response.matches.len(), 1);
    assert!(first_response.summary.indexed_candidates >= 1);

    export_corpus_bundle(&source_paths, "local", &bundle_root)
        .unwrap_or_else(|error| panic!("bundle export should succeed: {error}"));

    let imported_paths = RuntimePaths::from_state_root(imported_state_root.clone());
    import_corpus_bundle(&imported_paths, &bundle_root)
        .unwrap_or_else(|error| panic!("bundle import should succeed: {error}"));

    let imported_store = ConfigStore::new(imported_paths.clone());
    let imported_corpora = imported_store
        .list_corpora()
        .unwrap_or_else(|error| panic!("imported config should load: {error}"));
    assert_eq!(imported_corpora.len(), 1);
    assert_eq!(imported_corpora[0].id, "local");

    let imported_layout =
        net_rga_core::StateLayout::for_corpus(&imported_state_root, &CorpusId("local".to_owned()));
    assert!(imported_layout.manifest_db.exists());
    assert!(imported_layout.index_dir.join("index.db").exists());

    let imported_manifest = ManifestDb::open(&imported_layout.manifest_db)
        .unwrap_or_else(|error| panic!("imported manifest should open: {error}"));
    let imported_docs = imported_manifest
        .list_documents("local")
        .unwrap_or_else(|error| panic!("imported documents should list: {error}"));
    let completed_checkpoint = imported_manifest
        .sync_checkpoint("local", "last_sync_completed_at")
        .unwrap_or_else(|error| panic!("sync checkpoint should load: {error}"));

    assert_eq!(imported_docs.len(), 1);
    assert!(completed_checkpoint.is_some());

    let restored_response = execute_search(
        &imported_paths,
        &SearchRequest {
            corpus_id: CorpusId("local".to_owned()),
            query: "riverglass".to_owned(),
            fixed_strings: true,
            path_globs: Vec::new(),
            extensions: Vec::new(),
            content_types: Vec::new(),
            size_min: None,
            size_max: None,
            modified_after: None,
            modified_before: None,
            limit: None,
            output_format: SearchOutputFormat::Json,
        },
    )
    .unwrap_or_else(|error| panic!("restored search should succeed: {error}"));

    assert_eq!(restored_response.matches.len(), 1);
    assert!(restored_response.summary.indexed_candidates >= 1);

    fs::remove_dir_all(source_state_root).ok();
    fs::remove_dir_all(imported_state_root).ok();
    fs::remove_dir_all(corpus_root).ok();
    fs::remove_dir_all(bundle_root).ok();
}

fn temp_dir(prefix: &str) -> PathBuf {
    static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir()
        .join(prefix)
        .join(format!("{suffix}"))
        .join(format!("{}-{counter}", std::process::id()))
}

fn write_fixture(root: &Path, relative: &str, content: &str) {
    let path = root.join(relative);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .unwrap_or_else(|error| panic!("fixture parent should create: {error}"));
    }
    fs::write(path, content).unwrap_or_else(|error| panic!("fixture should write: {error}"));
}
