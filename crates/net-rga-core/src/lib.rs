pub mod config;
pub mod contracts;
pub mod domain;

pub use config::{
    AppConfig, BackendBinding, CorpusConfig, ProviderConfig, StateLayout, DEFAULT_STATE_DIR_NAME,
};
pub use contracts::{
    ByteRange, ContractError, DeltaCapability, DeltaEvent, DeltaPage, ExtractedDocument,
    ExtractionWarning, Extractor, ListPage, OpenUrlCapability, PermissionEntry,
    PermissionsCapability, Provider, ProviderSearchCapability, ReadPayload, ResolvedDocument,
    SearchBackend, SearchCandidate, SearchQuerySpec,
};
pub use domain::{
    Anchor, AnchorKind, AnchorLocator, CorpusDescriptor, CorpusId, CoverageCounts, CoverageStatus,
    DocumentId, DocumentLocator, DocumentMeta, ProviderKind, SearchMatch, SearchSummary,
};
