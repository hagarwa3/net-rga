use std::fs;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use aws_config::BehaviorVersion;
use aws_sdk_s3::config::{Builder as S3ConfigBuilder, Credentials, Region};
use aws_sdk_s3::primitives::ByteStream;
use net_rga_core::{
    CorpusConfig, LocalFsProvider, ManifestDb, ProviderConfig, RuntimePaths, S3ConnectionConfig,
    S3Provider, sync_corpus_with_provider,
};
use tokio::runtime::Runtime;

mod common;

use common::{MINIO_REGION, MinioHarness, build_minio_client, temp_dir, wait_for_minio, write_fixture};

#[test]
fn local_sync_repeated_runs_do_not_duplicate_documents() {
    let state_root = temp_dir("net-rga-sync-local-state");
    let corpus_root = temp_dir("net-rga-sync-local-corpus");
    write_fixture(&corpus_root, "docs/report.txt", "riverglass");

    let paths = RuntimePaths::from_state_root(state_root.clone());
    let corpus = CorpusConfig {
        id: "local".to_owned(),
        display_name: Some("Local".to_owned()),
        provider: ProviderConfig::LocalFs {
            root: corpus_root.clone(),
        },
        include_globs: Vec::new(),
        exclude_globs: Vec::new(),
        backend: None,
    };
    let provider = LocalFsProvider::new(corpus_root.clone());

    let first = sync_corpus_with_provider(&paths, &corpus, &provider)
        .unwrap_or_else(|error| panic!("first sync should succeed: {error}"));
    let second = sync_corpus_with_provider(&paths, &corpus, &provider)
        .unwrap_or_else(|error| panic!("second sync should succeed: {error}"));

    let manifest = ManifestDb::open(&state_root.join("corpora/local/manifest.db"))
        .unwrap_or_else(|error| panic!("manifest should open: {error}"));
    assert_eq!(first.new_documents, 1);
    assert_eq!(second.new_documents, 0);
    assert_eq!(second.updated_documents, 0);
    assert_eq!(
        manifest
            .document_count("local")
            .unwrap_or_else(|error| panic!("document count should query: {error}")),
        1
    );

    fs::remove_dir_all(state_root).ok();
    fs::remove_dir_all(corpus_root).ok();
}

#[cfg(unix)]
#[test]
fn local_sync_records_permission_drift() {
    let state_root = temp_dir("net-rga-sync-local-rbac-state");
    let corpus_root = temp_dir("net-rga-sync-local-rbac-corpus");
    write_fixture(&corpus_root, "docs/report.txt", "riverglass");

    let paths = RuntimePaths::from_state_root(state_root.clone());
    let corpus = CorpusConfig {
        id: "local".to_owned(),
        display_name: Some("Local".to_owned()),
        provider: ProviderConfig::LocalFs {
            root: corpus_root.clone(),
        },
        include_globs: Vec::new(),
        exclude_globs: Vec::new(),
        backend: None,
    };
    let provider = LocalFsProvider::new(corpus_root.clone());

    sync_corpus_with_provider(&paths, &corpus, &provider)
        .unwrap_or_else(|error| panic!("initial sync should succeed: {error}"));

    let original_permissions = fs::metadata(&corpus_root)
        .unwrap_or_else(|error| panic!("metadata should load: {error}"))
        .permissions();
    let mut locked_permissions = original_permissions.clone();
    locked_permissions.set_mode(0o000);
    fs::set_permissions(&corpus_root, locked_permissions)
        .unwrap_or_else(|error| panic!("permissions should tighten: {error}"));

    let result = sync_corpus_with_provider(&paths, &corpus, &provider);

    fs::set_permissions(&corpus_root, original_permissions)
        .unwrap_or_else(|error| panic!("permissions should restore: {error}"));

    assert!(result.is_err());
    let manifest = ManifestDb::open(&state_root.join("corpora/local/manifest.db"))
        .unwrap_or_else(|error| panic!("manifest should open: {error}"));
    assert_eq!(
        manifest
            .failure_record_count("local")
            .unwrap_or_else(|error| panic!("failure count should query: {error}")),
        1
    );
    assert_eq!(
        manifest
            .latest_failure_kind("local")
            .unwrap_or_else(|error| panic!("failure kind should query: {error}"))
            .as_deref(),
        Some("permission_denied")
    );

    fs::remove_dir_all(state_root).ok();
    fs::remove_dir_all(corpus_root).ok();
}

#[test]
fn s3_sync_repeated_runs_and_deletions_reconcile_cleanly() {
    let Some(harness) = MinioHarness::start() else {
        eprintln!("skipping MinIO sync integration test because Docker is unavailable");
        return;
    };

    let runtime = Runtime::new().unwrap_or_else(|error| panic!("runtime should build: {error}"));
    let client = build_minio_client(&runtime, &harness.endpoint);
    wait_for_minio(&runtime, &client);

    runtime
        .block_on(async {
            client
                .create_bucket()
                .bucket(&harness.bucket)
                .send()
                .await
                .map_err(|error| error.to_string())?;
            client
                .put_object()
                .bucket(&harness.bucket)
                .key("team-a/docs/report.txt")
                .body(ByteStream::from(b"riverglass".to_vec()))
                .content_type("text/plain")
                .send()
                .await
                .map_err(|error| error.to_string())?;
            Result::<(), String>::Ok(())
        })
        .unwrap_or_else(|error| panic!("minio setup should succeed: {error}"));

    let provider = S3Provider::from_parts(
        S3ConnectionConfig {
            bucket: harness.bucket.clone(),
            prefix: Some("team-a/".to_owned()),
            region: Some(MINIO_REGION.to_owned()),
            endpoint: Some(harness.endpoint.clone()),
            profile: None,
        },
        runtime,
        client.clone(),
    );
    let state_root = temp_dir("net-rga-sync-s3-state");
    let paths = RuntimePaths::from_state_root(state_root.clone());
    let corpus = CorpusConfig {
        id: "s3".to_owned(),
        display_name: Some("S3".to_owned()),
        provider: ProviderConfig::S3 {
            bucket: harness.bucket.clone(),
            prefix: Some("team-a".to_owned()),
            region: Some(MINIO_REGION.to_owned()),
            endpoint: Some(harness.endpoint.clone()),
            profile: None,
        },
        include_globs: Vec::new(),
        exclude_globs: Vec::new(),
        backend: None,
    };

    let first = sync_corpus_with_provider(&paths, &corpus, &provider)
        .unwrap_or_else(|error| panic!("first sync should succeed: {error}"));
    let second = sync_corpus_with_provider(&paths, &corpus, &provider)
        .unwrap_or_else(|error| panic!("second sync should succeed: {error}"));

    provider
        .runtime()
        .block_on(async {
            provider
                .client()
                .delete_object()
                .bucket(&harness.bucket)
                .key("team-a/docs/report.txt")
                .send()
                .await
                .map_err(|error| error.to_string())
        })
        .unwrap_or_else(|error| panic!("object delete should succeed: {error}"));

    let third = sync_corpus_with_provider(&paths, &corpus, &provider)
        .unwrap_or_else(|error| panic!("third sync should succeed: {error}"));

    let manifest = ManifestDb::open(&state_root.join("corpora/s3/manifest.db"))
        .unwrap_or_else(|error| panic!("manifest should open: {error}"));
    assert_eq!(first.new_documents, 1);
    assert_eq!(second.new_documents, 0);
    assert_eq!(third.deleted_documents, 1);
    assert_eq!(
        manifest
            .document_count("s3")
            .unwrap_or_else(|error| panic!("document count should query: {error}")),
        0
    );
    assert_eq!(
        manifest
            .tombstone_count("s3")
            .unwrap_or_else(|error| panic!("tombstone count should query: {error}")),
        1
    );

    fs::remove_dir_all(state_root).ok();
}

#[test]
fn s3_sync_records_permission_drift() {
    let Some(harness) = MinioHarness::start() else {
        eprintln!("skipping MinIO sync RBAC test because Docker is unavailable");
        return;
    };

    let runtime = Runtime::new().unwrap_or_else(|error| panic!("runtime should build: {error}"));
    let client = build_minio_client(&runtime, &harness.endpoint);
    wait_for_minio(&runtime, &client);

    runtime
        .block_on(async {
            client
                .create_bucket()
                .bucket(&harness.bucket)
                .send()
                .await
                .map_err(|error| error.to_string())?;
            client
                .put_object()
                .bucket(&harness.bucket)
                .key("team-a/docs/report.txt")
                .body(ByteStream::from(b"riverglass".to_vec()))
                .content_type("text/plain")
                .send()
                .await
                .map_err(|error| error.to_string())?;
            Result::<(), String>::Ok(())
        })
        .unwrap_or_else(|error| panic!("minio setup should succeed: {error}"));

    let good_provider = S3Provider::from_parts(
        S3ConnectionConfig {
            bucket: harness.bucket.clone(),
            prefix: Some("team-a/".to_owned()),
            region: Some(MINIO_REGION.to_owned()),
            endpoint: Some(harness.endpoint.clone()),
            profile: None,
        },
        runtime,
        client,
    );
    let state_root = temp_dir("net-rga-sync-s3-rbac-state");
    let paths = RuntimePaths::from_state_root(state_root.clone());
    let corpus = CorpusConfig {
        id: "s3".to_owned(),
        display_name: Some("S3".to_owned()),
        provider: ProviderConfig::S3 {
            bucket: harness.bucket.clone(),
            prefix: Some("team-a".to_owned()),
            region: Some(MINIO_REGION.to_owned()),
            endpoint: Some(harness.endpoint.clone()),
            profile: None,
        },
        include_globs: Vec::new(),
        exclude_globs: Vec::new(),
        backend: None,
    };

    sync_corpus_with_provider(&paths, &corpus, &good_provider)
        .unwrap_or_else(|error| panic!("initial sync should succeed: {error}"));

    let denied_runtime = Runtime::new().unwrap_or_else(|error| panic!("runtime should build: {error}"));
    let denied_shared_config = denied_runtime.block_on(async {
        aws_config::defaults(BehaviorVersion::latest())
            .region(Region::new(MINIO_REGION))
            .credentials_provider(Credentials::new(
                "wrong-user",
                "wrong-secret",
                None,
                None,
                "net-rga-tests",
            ))
            .load()
            .await
    });
    let denied_client = aws_sdk_s3::Client::from_conf(
        S3ConfigBuilder::from(&denied_shared_config)
            .endpoint_url(&harness.endpoint)
            .force_path_style(true)
            .build(),
    );
    let denied_provider = S3Provider::from_parts(
        S3ConnectionConfig {
            bucket: harness.bucket.clone(),
            prefix: Some("team-a/".to_owned()),
            region: Some(MINIO_REGION.to_owned()),
            endpoint: Some(harness.endpoint.clone()),
            profile: None,
        },
        denied_runtime,
        denied_client,
    );

    let result = sync_corpus_with_provider(&paths, &corpus, &denied_provider);
    assert!(result.is_err());

    let manifest = ManifestDb::open(&state_root.join("corpora/s3/manifest.db"))
        .unwrap_or_else(|error| panic!("manifest should open: {error}"));
    assert_eq!(
        manifest
            .failure_record_count("s3")
            .unwrap_or_else(|error| panic!("failure count should query: {error}")),
        1
    );
    assert_eq!(
        manifest
            .latest_failure_kind("s3")
            .unwrap_or_else(|error| panic!("failure kind should query: {error}"))
            .as_deref(),
        Some("permission_denied")
    );

    fs::remove_dir_all(state_root).ok();
}
