use std::fs;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use aws_config::BehaviorVersion;
use aws_sdk_s3::Client;
use aws_sdk_s3::config::{Builder as S3ConfigBuilder, Credentials, Region};
use tokio::runtime::Runtime;

pub const MINIO_IMAGE: &str = "minio/minio:latest";
pub const MINIO_ACCESS_KEY: &str = "minioadmin";
pub const MINIO_SECRET_KEY: &str = "minioadmin123";
pub const MINIO_REGION: &str = "us-east-1";

pub struct MinioHarness {
    pub endpoint: String,
    pub bucket: String,
    container_name: String,
}

impl MinioHarness {
    pub fn start() -> Option<Self> {
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

pub fn build_minio_client(runtime: &Runtime, endpoint: &str) -> Client {
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

pub fn wait_for_minio(runtime: &Runtime, client: &Client) {
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

pub fn temp_dir(prefix: &str) -> PathBuf {
    std::env::temp_dir().join(prefix).join(unique_suffix())
}

pub fn write_fixture(root: &Path, relative: &str, content: &str) {
    let path = root.join(relative);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap_or_else(|error| panic!("fixture parent should create: {error}"));
    }
    fs::write(path, content).unwrap_or_else(|error| panic!("fixture should write: {error}"));
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
