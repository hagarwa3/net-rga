pub mod bundle;
pub mod config;
pub mod contracts;
pub mod domain;
pub mod extraction;
mod extractor_adapters;
pub mod index;
pub mod index_lifecycle;
mod lexical;
pub mod providers;
pub mod runtime;
pub mod search_engine;
pub mod state;
pub mod sync;

pub use bundle::{
    BUNDLE_SCHEMA_VERSION, BundleError, BundleManifest, BundlePayload, export_corpus_bundle,
    import_corpus_bundle,
};
pub use config::{
    AppConfig, BackendBinding, CorpusConfig, DEFAULT_STATE_DIR_NAME, ProviderConfig, StateLayout,
};
pub use contracts::{
    ByteRange, ContractError, DeltaCapability, DeltaEvent, DeltaPage, ExtractedChunk,
    ExtractedDocument, ExtractionWarning, Extractor, ListPage, OpenUrlCapability, PermissionEntry,
    PermissionsCapability, Provider, ProviderSearchCapability, ReadPayload, ResolvedDocument,
    SearchBackend, SearchCandidate, SearchQuerySpec,
};
pub use domain::{
    Anchor, AnchorKind, AnchorLocator, AnchorParseError, CanonicalChunk, CanonicalContentKind,
    CanonicalDocument, CorpusDescriptor, CorpusId, CoverageCounts, CoverageStatus, DocumentId,
    DocumentLocator, DocumentMeta, ProviderKind, SearchMatch, SearchOutputFormat, SearchRequest,
    SearchResponse, SearchSummary,
};
pub use extraction::{ExtractionPlan, ExtractorRegistry};
pub use extractor_adapters::PdfExtractor;
pub use index::{INDEX_SCHEMA_V1, IndexError, IndexUpdateStrategy, IndexedChunkHit, LexicalIndex};
pub use providers::{LocalFsProvider, S3ConnectionConfig, S3Provider};
pub use runtime::{ConfigStore, RuntimeError, RuntimePaths};
pub use search_engine::{
    SearchEngineError, execute_search, filter_manifest_documents, rank_documents,
};
pub use state::MANIFEST_SCHEMA_V1;
pub use state::{ManifestDb, ManifestError, apply_manifest_migrations, open_manifest_db};
pub use sync::{
    SyncCheckpointName, SyncError, SyncRunSummary, sync_corpus, sync_corpus_with_provider,
};

pub use index_lifecycle::{
    IndexBuildSummary, IndexLifecycleError, IndexStatus, build_index, clear_index, index_status,
    rebuild_index,
};
