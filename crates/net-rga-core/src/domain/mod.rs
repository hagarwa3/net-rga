mod anchor;
mod corpus;
mod document;
mod search;

pub use anchor::{Anchor, AnchorKind, AnchorLocator};
pub use corpus::{CorpusDescriptor, CorpusId, ProviderKind};
pub use document::{DocumentId, DocumentLocator, DocumentMeta};
pub use search::{CoverageCounts, CoverageStatus, SearchMatch, SearchSummary};

