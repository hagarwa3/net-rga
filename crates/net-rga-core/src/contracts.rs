use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::domain::{Anchor, DocumentId, DocumentLocator, DocumentMeta};

#[derive(Debug, Error)]
pub enum ContractError {
    #[error("not found: {0}")]
    NotFound(String),
    #[error("permission denied: {0}")]
    PermissionDenied(String),
    #[error("throttled: {0}")]
    Throttled(String),
    #[error("transient error: {0}")]
    Transient(String),
    #[error("unsupported: {0}")]
    Unsupported(String),
    #[error("invalid input: {0}")]
    Invalid(String),
    #[error("io error: {0}")]
    Io(String),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ByteRange {
    pub start: u64,
    pub end: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadPayload {
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListPage {
    pub documents: Vec<DocumentMeta>,
    pub next_cursor: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedDocument {
    pub id: DocumentId,
    pub locator: DocumentLocator,
    pub meta: Option<DocumentMeta>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeltaEvent {
    Upsert,
    Delete,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeltaPage {
    pub events: Vec<(DeltaEvent, DocumentMeta)>,
    pub next_cursor: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionEntry {
    pub principal: String,
    pub role: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchQuerySpec {
    pub query: String,
    pub limit: Option<u32>,
    pub path_globs: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SearchCandidate {
    pub document_id: DocumentId,
    pub anchor: Option<Anchor>,
    pub snippet: Option<String>,
    pub score: Option<f32>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtractionWarning {
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtractedDocument {
    pub text: String,
    pub anchors: Vec<Anchor>,
    pub warnings: Vec<ExtractionWarning>,
    pub unsupported_reason: Option<String>,
}

pub trait Provider {
    fn list(&self, prefix: &str, cursor: Option<&str>) -> Result<ListPage, ContractError>;
    fn stat(&self, document_id: &DocumentId) -> Result<DocumentMeta, ContractError>;
    fn read(&self, document_id: &DocumentId, range: Option<ByteRange>)
        -> Result<ReadPayload, ContractError>;
    fn resolve(&self, locator: &DocumentLocator) -> Result<ResolvedDocument, ContractError>;
}

pub trait DeltaCapability {
    fn delta(&self, cursor: Option<&str>) -> Result<DeltaPage, ContractError>;
}

pub trait PermissionsCapability {
    fn permissions(&self, document_id: &DocumentId) -> Result<Vec<PermissionEntry>, ContractError>;
}

pub trait OpenUrlCapability {
    fn open_url(&self, document_id: &DocumentId) -> Result<String, ContractError>;
}

pub trait ProviderSearchCapability {
    fn search(&self, request: &SearchQuerySpec) -> Result<Vec<SearchCandidate>, ContractError>;
}

pub trait SearchBackend {
    fn query(&self, request: &SearchQuerySpec) -> Result<Vec<SearchCandidate>, ContractError>;
}

pub trait Extractor {
    fn can_handle(&self, meta: &DocumentMeta, sniff: &[u8]) -> bool;
    fn extract(&self, bytes: &[u8], meta: &DocumentMeta) -> Result<ExtractedDocument, ContractError>;
}

#[cfg(test)]
mod tests {
    use super::{ByteRange, SearchQuerySpec};

    #[test]
    fn byte_range_allows_open_ended_reads() {
        let range = ByteRange {
            start: 128,
            end: None,
        };
        assert_eq!(range.start, 128);
        assert_eq!(range.end, None);
    }

    #[test]
    fn search_query_spec_supports_empty_path_filters() {
        let query = SearchQuerySpec {
            query: "riverglass".to_owned(),
            limit: Some(5),
            path_globs: Vec::new(),
        };
        assert_eq!(query.limit, Some(5));
        assert!(query.path_globs.is_empty());
    }
}

