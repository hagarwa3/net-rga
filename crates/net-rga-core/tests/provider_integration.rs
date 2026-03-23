use std::fs;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::process::Stdio;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use aws_config::BehaviorVersion;
use aws_sdk_s3::Client;
use aws_sdk_s3::config::{Builder as S3ConfigBuilder, Credentials, Region};
use aws_sdk_s3::primitives::ByteStream;
use net_rga_core::{
    ByteRange, DocumentId, DocumentLocator, LocalFsProvider, Provider, ReadPayload, S3ConnectionConfig,
    S3Provider,
};
use tokio::runtime::Runtime;

const MINIO_IMAGE: &str = "minio/minio:latest";
const MINIO_ACCESS_KEY: &str = "minioadmin";
const MINIO_SECRET_KEY: &str = "minioadmin123";
const MINIO_REGION: &str = "us-east-1";

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

fn build_minio_client(runtime: &Runtime, endpoint: &str) -> Client {
    let shared_config = runtime.block_on(async {
        aws_config::defaults(BehaviorVersion::latest())
            .region(Region::new(MINIO_REGION))
            .credentials_provider(Credentials::new(
                MINIO_ACCESS_KEY,
                MINIO_SECRET_KEY,
                None,
                None,
                "net-rga-tests",
            ))
            .load()
            .await
    });
    let config = S3ConfigBuilder::from(&shared_config)
        .endpoint_url(endpoint)
        .force_path_style(true)
        .build();
    Client::from_conf(config)
}

fn wait_for_minio(runtime: &Runtime, client: &Client) {
    let mut ready = false;
    for _ in 0..40 {
        let result = runtime.block_on(async { client.list_buckets().send().await });
        match result {
            Ok(_) => {
                ready = true;
                break;
            }
            Err(_) => thread::sleep(Duration::from_millis(250)),
        }
    }
    assert!(ready, "MinIO did not become ready in time");
}

struct MinioHarness {
    endpoint: String,
    bucket: String,
    container_name: String,
}

impl MinioHarness {
    fn start() -> Option<Self> {
        if !docker_available() {
            return None;
        }

        let port = free_port();
        let suffix = unique_suffix();
        let container_name = format!("net-rga-minio-{suffix}");
        let bucket = format!("net-rga-{suffix}");
        let port_arg = format!("127.0.0.1:{port}:9000");
        let status = Command::new("docker")
            .args([
                "run",
                "-d",
                "--rm",
                "--name",
                &container_name,
                "-p",
                &port_arg,
                "-e",
                &format!("MINIO_ROOT_USER={MINIO_ACCESS_KEY}"),
                "-e",
                &format!("MINIO_ROOT_PASSWORD={MINIO_SECRET_KEY}"),
                MINIO_IMAGE,
                "server",
                "/data",
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .ok()?;

        if !status.success() {
            return None;
        }

        Some(Self {
            endpoint: format!("http://127.0.0.1:{port}"),
            bucket,
            container_name,
        })
    }
}

impl Drop for MinioHarness {
    fn drop(&mut self) {
        let _ = Command::new("docker")
            .args(["rm", "-f", &self.container_name])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

fn docker_available() -> bool {
    Command::new("docker")
        .args(["info"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap_or_else(|error| panic!("ephemeral port should bind: {error}"))
        .local_addr()
        .unwrap_or_else(|error| panic!("local addr should resolve: {error}"))
        .port()
}

fn unique_suffix() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos().to_string())
        .unwrap_or_else(|_| "fallback".to_owned())
}

fn temp_dir(prefix: &str) -> PathBuf {
    std::env::temp_dir().join(prefix).join(unique_suffix())
}

fn write_fixture(root: &Path, relative: &str, content: &str) {
    let path = root.join(relative);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap_or_else(|error| panic!("fixture parent should create: {error}"));
    }
    fs::write(path, content).unwrap_or_else(|error| panic!("fixture should write: {error}"));
}
