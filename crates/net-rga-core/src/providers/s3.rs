use aws_config::BehaviorVersion;
use aws_sdk_s3::Client;
use aws_sdk_s3::config::{Builder as S3ConfigBuilder, Region};
use aws_sdk_s3::error::{ProvideErrorMetadata, SdkError};
use aws_sdk_s3::operation::get_object::GetObjectError;
use aws_sdk_s3::operation::head_object::HeadObjectError;
use aws_sdk_s3::operation::list_objects_v2::ListObjectsV2Error;
use tokio::runtime::Runtime;

use crate::config::ProviderConfig;
use crate::contracts::{ByteRange, ContractError, ListPage, Provider, ReadPayload, ResolvedDocument};
use crate::domain::{DocumentId, DocumentLocator, DocumentMeta};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct S3ConnectionConfig {
    pub bucket: String,
    pub prefix: Option<String>,
    pub region: Option<String>,
    pub endpoint: Option<String>,
    pub profile: Option<String>,
}

impl S3ConnectionConfig {
    pub fn from_provider_config(provider: &ProviderConfig) -> Result<Self, ContractError> {
        match provider {
            ProviderConfig::S3 {
                bucket,
                prefix,
                region,
                endpoint,
                profile,
            } => Ok(Self {
                bucket: bucket.clone(),
                prefix: normalize_prefix(prefix.clone()),
                region: region.clone(),
                endpoint: endpoint.clone(),
                profile: profile.clone(),
            }),
            _ => Err(ContractError::Invalid(
                "expected s3 provider configuration".to_owned(),
            )),
        }
    }

    pub fn object_key(&self, document_id: &str) -> String {
        match &self.prefix {
            Some(prefix) => format!("{prefix}{document_id}"),
            None => document_id.to_owned(),
        }
    }

    pub fn list_prefix(&self, prefix: &str) -> Option<String> {
        let normalized = normalize_relative_prefix(prefix);
        match (&self.prefix, normalized) {
            (Some(base), Some(relative)) => Some(format!("{base}{relative}")),
            (Some(base), None) => Some(base.clone()),
            (None, Some(relative)) => Some(relative),
            (None, None) => None,
        }
    }

    pub fn strip_prefix(&self, object_key: &str) -> Option<String> {
        match &self.prefix {
            Some(prefix) => object_key.strip_prefix(prefix).map(ToOwned::to_owned),
            None => Some(object_key.to_owned()),
        }
    }
}

pub struct S3Provider {
    config: S3ConnectionConfig,
    runtime: Runtime,
    client: Client,
}

impl S3Provider {
    pub fn new(config: S3ConnectionConfig) -> Result<Self, ContractError> {
        let runtime = Runtime::new().map_err(|error| ContractError::Io(error.to_string()))?;
        let client = build_client(&runtime, &config)?;
        Ok(Self::from_parts(config, runtime, client))
    }

    pub fn from_parts(config: S3ConnectionConfig, runtime: Runtime, client: Client) -> Self {
        Self {
            config,
            runtime,
            client,
        }
    }

    pub fn config(&self) -> &S3ConnectionConfig {
        &self.config
    }

    pub fn client(&self) -> &Client {
        &self.client
    }

    pub fn runtime(&self) -> &Runtime {
        &self.runtime
    }
}

impl Provider for S3Provider {
    fn list(&self, prefix: &str, cursor: Option<&str>) -> Result<ListPage, ContractError> {
        let list_prefix = self.config.list_prefix(prefix);
        let bucket = self.config.bucket.clone();
        let client = self.client.clone();
        let response = self.runtime.block_on(async move {
            let mut request = client.list_objects_v2().bucket(bucket).max_keys(1000);
            if let Some(value) = list_prefix {
                request = request.prefix(value);
            }
            if let Some(token) = cursor {
                request = request.continuation_token(token.to_owned());
            }
            request.send().await
        });

        let output = response.map_err(map_list_error)?;
        let mut documents = Vec::new();
        for object in output.contents() {
            if let Some(document) = document_meta_from_object(&self.config, object.key(), object.size(), object.last_modified(), object.e_tag(), None) {
                documents.push(document);
            }
        }

        Ok(ListPage {
            documents,
            next_cursor: output
                .next_continuation_token()
                .map(ToOwned::to_owned),
        })
    }

    fn stat(&self, document_id: &DocumentId) -> Result<DocumentMeta, ContractError> {
        let key = self.config.object_key(&document_id.0);
        let request_key = key.clone();
        let bucket = self.config.bucket.clone();
        let client = self.client.clone();
        let response = self.runtime.block_on(async move {
            client
                .head_object()
                .bucket(bucket)
                .key(&request_key)
                .send()
                .await
        });

        let output = response.map_err(map_head_error)?;
        document_meta_from_object(
            &self.config,
            Some(&key),
            output.content_length(),
            output.last_modified(),
            output.e_tag(),
            output.content_type(),
        )
        .ok_or_else(|| ContractError::NotFound(document_id.0.clone()))
    }

    fn read(&self, document_id: &DocumentId, range: Option<ByteRange>) -> Result<ReadPayload, ContractError> {
        if let Some(ref requested_range) = range
            && let Some(end) = requested_range.end
        {
            if end < requested_range.start {
                return Err(ContractError::Invalid("invalid byte range".to_owned()));
            }
            if end == requested_range.start {
                return Ok(ReadPayload { bytes: Vec::new() });
            }
        }

        let key = self.config.object_key(&document_id.0);
        let bucket = self.config.bucket.clone();
        let client = self.client.clone();
        let response = self.runtime.block_on(async move {
            let mut request = client.get_object().bucket(bucket).key(key);
            if let Some(value) = range_to_header(range.as_ref()) {
                request = request.range(value);
            }
            request.send().await
        });

        let output = response.map_err(map_get_error)?;
        let bytes = self.runtime.block_on(async move {
            output
                .body
                .collect()
                .await
                .map(|payload| payload.into_bytes().to_vec())
        });

        bytes
            .map(|payload| ReadPayload { bytes: payload })
            .map_err(|error| ContractError::Io(error.to_string()))
    }

    fn resolve(&self, locator: &DocumentLocator) -> Result<ResolvedDocument, ContractError> {
        let document_id = DocumentId(locator.path.clone());
        let meta = self.stat(&document_id)?;
        Ok(ResolvedDocument {
            id: document_id,
            locator: locator.clone(),
            meta: Some(meta),
        })
    }
}

fn build_client(runtime: &Runtime, config: &S3ConnectionConfig) -> Result<Client, ContractError> {
    let mut loader = aws_config::defaults(BehaviorVersion::latest());
    if let Some(profile) = &config.profile {
        loader = loader.profile_name(profile);
    }
    if let Some(region) = &config.region {
        loader = loader.region(Region::new(region.clone()));
    }

    let shared_config = runtime.block_on(loader.load());
    let mut builder = S3ConfigBuilder::from(&shared_config);
    if let Some(endpoint) = &config.endpoint {
        builder = builder.endpoint_url(endpoint).force_path_style(true);
    }

    Ok(Client::from_conf(builder.build()))
}

fn document_meta_from_object(
    config: &S3ConnectionConfig,
    key: Option<&str>,
    size: Option<i64>,
    modified_at: Option<&aws_sdk_s3::primitives::DateTime>,
    version: Option<&str>,
    content_type: Option<&str>,
) -> Option<DocumentMeta> {
    let key = key?;
    let document_id = config.strip_prefix(key)?;
    let modified_at = modified_at.map(|value| value.secs().to_string());
    let extension = document_id
        .rsplit_once('.')
        .map(|(_, extension)| extension.to_owned());
    let content_type = content_type
        .map(ToOwned::to_owned)
        .or_else(|| guess_content_type(extension.as_deref()));

    Some(DocumentMeta {
        id: DocumentId(document_id.clone()),
        locator: DocumentLocator { path: document_id },
        extension,
        content_type,
        version: version.map(ToOwned::to_owned),
        size_bytes: size.and_then(|value| u64::try_from(value).ok()).unwrap_or_default(),
        modified_at,
    })
}

fn range_to_header(range: Option<&ByteRange>) -> Option<String> {
    range.map(|value| match value.end {
        Some(end) => format!("bytes={}-{}", value.start, end.saturating_sub(1)),
        None => format!("bytes={}-", value.start),
    })
}

fn guess_content_type(extension: Option<&str>) -> Option<String> {
    let content_type = match extension {
        Some("csv") => "text/csv",
        Some("docx") => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        Some("gz") => "application/gzip",
        Some("json") => "application/json",
        Some("log") | Some("md") | Some("txt") => "text/plain",
        Some("pdf") => "application/pdf",
        Some("pptx") => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        Some("xlsx") => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        Some("zip") => "application/zip",
        _ => return None,
    };
    Some(content_type.to_owned())
}

fn map_list_error(error: SdkError<ListObjectsV2Error>) -> ContractError {
    map_sdk_error(error)
}

fn map_head_error(error: SdkError<HeadObjectError>) -> ContractError {
    map_sdk_error(error)
}

fn map_get_error(error: SdkError<GetObjectError>) -> ContractError {
    map_sdk_error(error)
}

fn map_sdk_error<E>(error: SdkError<E>) -> ContractError
where
    E: ProvideErrorMetadata + std::fmt::Display + std::fmt::Debug + std::error::Error + Send + Sync + 'static,
{
    if let Some(service_error) = error.as_service_error() {
        let message = service_error
            .message()
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| service_error.to_string());
        return match service_error.code() {
            Some("AccessDenied") | Some("Forbidden") => ContractError::PermissionDenied(message),
            Some("SlowDown") | Some("Throttling") | Some("ThrottlingException")
            | Some("TooManyRequestsException") | Some("RequestTimeout") => ContractError::Throttled(message),
            Some("NoSuchBucket") | Some("NoSuchKey") | Some("NotFound") | Some("404") => ContractError::NotFound(message),
            _ => ContractError::Io(message),
        };
    }

    match error {
        SdkError::TimeoutError(timeout) => ContractError::Transient(format!("{timeout:?}")),
        SdkError::DispatchFailure(dispatch) => ContractError::Transient(format!("{dispatch:?}")),
        SdkError::ResponseError(response) => ContractError::Transient(format!("{response:?}")),
        SdkError::ConstructionFailure(construction) => ContractError::Invalid(format!("{construction:?}")),
        SdkError::ServiceError(_) => unreachable!("service errors are handled above"),
        _ => ContractError::Transient("unhandled sdk error".to_owned()),
    }
}

fn normalize_prefix(prefix: Option<String>) -> Option<String> {
    prefix.and_then(|value| {
        let trimmed = value.trim_matches('/').to_owned();
        if trimmed.is_empty() {
            None
        } else {
            Some(format!("{trimmed}/"))
        }
    })
}

fn normalize_relative_prefix(prefix: &str) -> Option<String> {
    let trimmed = prefix.trim_matches('/');
    if trimmed.is_empty() {
        None
    } else {
        Some(format!("{trimmed}/"))
    }
}

#[cfg(test)]
mod tests {
    use crate::config::ProviderConfig;

    use super::{S3ConnectionConfig, normalize_relative_prefix, range_to_header};

    #[test]
    fn normalizes_s3_prefixes() {
        let config = S3ConnectionConfig::from_provider_config(&ProviderConfig::S3 {
            bucket: "bucket".to_owned(),
            prefix: Some("/docs/reports/".to_owned()),
            region: None,
            endpoint: None,
            profile: None,
        })
        .unwrap_or_else(|error| panic!("s3 config should parse: {error}"));

        assert_eq!(config.prefix.as_deref(), Some("docs/reports/"));
        assert_eq!(config.object_key("file.txt"), "docs/reports/file.txt");
        assert_eq!(config.strip_prefix("docs/reports/file.txt").as_deref(), Some("file.txt"));
    }

    #[test]
    fn empty_prefix_becomes_none() {
        let config = S3ConnectionConfig::from_provider_config(&ProviderConfig::S3 {
            bucket: "bucket".to_owned(),
            prefix: Some("/".to_owned()),
            region: Some("us-west-2".to_owned()),
            endpoint: Some("http://localhost:9000".to_owned()),
            profile: Some("dev".to_owned()),
        })
        .unwrap_or_else(|error| panic!("s3 config should parse: {error}"));

        assert_eq!(config.prefix, None);
        assert_eq!(config.region.as_deref(), Some("us-west-2"));
        assert_eq!(config.endpoint.as_deref(), Some("http://localhost:9000"));
    }

    #[test]
    fn list_prefix_combines_corpus_and_request_prefixes() {
        let config = S3ConnectionConfig::from_provider_config(&ProviderConfig::S3 {
            bucket: "bucket".to_owned(),
            prefix: Some("docs".to_owned()),
            region: None,
            endpoint: None,
            profile: None,
        })
        .unwrap_or_else(|error| panic!("s3 config should parse: {error}"));

        assert_eq!(config.list_prefix("reports"), Some("docs/reports/".to_owned()));
        assert_eq!(config.list_prefix(""), Some("docs/".to_owned()));
    }

    #[test]
    fn relative_prefix_normalization_is_empty_safe() {
        assert_eq!(normalize_relative_prefix(""), None);
        assert_eq!(normalize_relative_prefix("/nested/path/"), Some("nested/path/".to_owned()));
    }

    #[test]
    fn range_header_uses_exclusive_end_offsets() {
        assert_eq!(
            range_to_header(Some(&crate::contracts::ByteRange {
                start: 4,
                end: Some(8),
            })),
            Some("bytes=4-7".to_owned())
        );
        assert_eq!(
            range_to_header(Some(&crate::contracts::ByteRange {
                start: 4,
                end: None,
            })),
            Some("bytes=4-".to_owned())
        );
    }
}
