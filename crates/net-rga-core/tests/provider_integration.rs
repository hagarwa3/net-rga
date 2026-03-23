use std::fs;

use aws_sdk_s3::primitives::ByteStream;
use net_rga_core::{
    ByteRange, DocumentId, DocumentLocator, LocalFsProvider, Provider, ReadPayload, S3ConnectionConfig,
    S3Provider,
};
use tokio::runtime::Runtime;

mod common;

use common::{MINIO_REGION, MinioHarness, build_minio_client, temp_dir, wait_for_minio, write_fixture};

#[test]
fn local_filesystem_provider_round_trips_real_files() {
    let root = temp_dir("net-rga-provider-local");
    write_fixture(&root, "docs/report.txt", "riverglass quarterly report");
    write_fixture(&root, "logs/app.log", "bootstrap complete");

    let provider = LocalFsProvider::new(root.clone());

    let page = provider
        .list("docs", None)
        .unwrap_or_else(|error| panic!("list should succeed: {error}"));
    assert_eq!(page.documents.len(), 1);
    assert_eq!(page.documents[0].locator.path, "docs/report.txt");

    let stat = provider
        .stat(&DocumentId("docs/report.txt".to_owned()))
        .unwrap_or_else(|error| panic!("stat should succeed: {error}"));
    assert_eq!(stat.size_bytes, 27);

    let payload = provider
        .read(
            &DocumentId("docs/report.txt".to_owned()),
            Some(ByteRange {
                start: 0,
                end: Some(10),
            }),
        )
        .unwrap_or_else(|error| panic!("read should succeed: {error}"));
    assert_eq!(payload, ReadPayload { bytes: b"riverglass".to_vec() });

    let resolved = provider
        .resolve(&DocumentLocator {
            path: "docs/report.txt".to_owned(),
        })
        .unwrap_or_else(|error| panic!("resolve should succeed: {error}"));
    assert_eq!(resolved.id.0, "docs/report.txt");

    fs::remove_dir_all(root).ok();
}

#[test]
fn s3_provider_round_trips_against_minio() {
    let Some(harness) = MinioHarness::start() else {
        eprintln!("skipping MinIO integration test because Docker is unavailable");
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
                .body(ByteStream::from(b"riverglass quarter".to_vec()))
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
        client,
    );

    let page = provider
        .list("docs", None)
        .unwrap_or_else(|error| panic!("s3 list should succeed: {error}"));
    assert_eq!(page.documents.len(), 1);
    assert_eq!(page.documents[0].locator.path, "docs/report.txt");

    let stat = provider
        .stat(&DocumentId("docs/report.txt".to_owned()))
        .unwrap_or_else(|error| panic!("s3 stat should succeed: {error}"));
    assert_eq!(stat.content_type.as_deref(), Some("text/plain"));

    let payload = provider
        .read(
            &DocumentId("docs/report.txt".to_owned()),
            Some(ByteRange {
                start: 0,
                end: Some(10),
            }),
        )
        .unwrap_or_else(|error| panic!("s3 read should succeed: {error}"));
    assert_eq!(payload.bytes, b"riverglass".to_vec());

    let resolved = provider
        .resolve(&DocumentLocator {
            path: "docs/report.txt".to_owned(),
        })
        .unwrap_or_else(|error| panic!("s3 resolve should succeed: {error}"));
    assert_eq!(resolved.id.0, "docs/report.txt");
}
