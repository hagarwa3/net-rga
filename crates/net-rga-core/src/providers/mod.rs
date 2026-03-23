mod local_fs;
mod s3;

use crate::config::ProviderConfig;
use crate::contracts::{ContractError, Provider};

pub use local_fs::LocalFsProvider;
pub use s3::{S3ConnectionConfig, S3Provider};

pub fn provider_from_config(config: &ProviderConfig) -> Result<Box<dyn Provider>, ContractError> {
    match config {
        ProviderConfig::LocalFs { root } => Ok(Box::new(LocalFsProvider::new(root.clone()))),
        ProviderConfig::S3 { .. } => {
            let connection = S3ConnectionConfig::from_provider_config(config)?;
            Ok(Box::new(S3Provider::new(connection)?))
        }
    }
}
