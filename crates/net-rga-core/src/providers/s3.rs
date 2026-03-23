use aws_config::BehaviorVersion;
use aws_sdk_s3::Client;
use aws_sdk_s3::config::{Builder as S3ConfigBuilder, Region};
use tokio::runtime::Runtime;

use crate::config::ProviderConfig;
use crate::contracts::ContractError;

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
        Ok(Self {
            config,
            runtime,
            client,
        })
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
        builder = builder.endpoint_url(endpoint);
    }

    Ok(Client::from_conf(builder.build()))
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

#[cfg(test)]
mod tests {
    use crate::config::ProviderConfig;

    use super::S3ConnectionConfig;

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
}

