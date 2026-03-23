pub mod config;
pub mod contracts;
pub mod domain;
pub mod extraction;
pub mod providers;
pub mod runtime;
pub mod search_engine;
pub mod state;
pub mod sync;

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
    DocumentId, DocumentLocator, DocumentMeta, ProviderKind, SearchMatch, SearchOutputFormat,
    SearchRequest, SearchResponse, SearchSummary,
};
pub use extraction::{CanonicalDocument, ExtractionPlan, ExtractorRegistry};
pub use providers::{LocalFsProvider, S3ConnectionConfig, S3Provider};
pub use runtime::{ConfigStore, RuntimeError, RuntimePaths};
pub use search_engine::{SearchEngineError, execute_search, filter_manifest_documents, rank_documents};
pub use state::MANIFEST_SCHEMA_V1;
pub use state::{ManifestDb, ManifestError, apply_manifest_migrations, open_manifest_db};
pub use sync::{SyncCheckpointName, SyncError, SyncRunSummary, sync_corpus, sync_corpus_with_provider};
