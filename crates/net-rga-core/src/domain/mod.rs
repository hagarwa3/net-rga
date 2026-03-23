mod anchor;
mod canonical;
mod corpus;
mod document;
mod search;

pub use anchor::{Anchor, AnchorKind, AnchorLocator, AnchorParseError};
pub use canonical::{CanonicalChunk, CanonicalContentKind, CanonicalDocument};
pub use corpus::{CorpusDescriptor, CorpusId, ProviderKind};
pub use document::{DocumentId, DocumentLocator, DocumentMeta};
pub use search::{
    CoverageCounts, CoverageStatus, SearchMatch, SearchOutputFormat, SearchRequest, SearchResponse,
    SearchSummary,
};
