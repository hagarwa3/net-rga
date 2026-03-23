use serde::{Deserialize, Serialize};

use crate::domain::{Anchor, CorpusId, DocumentId};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CoverageStatus {
    Complete,
    Partial,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoverageCounts {
    pub deleted_count: u64,
    pub denied_count: u64,
    pub stale_count: u64,
    pub unsupported_count: u64,
    pub failure_count: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchMatch {
    pub document_id: DocumentId,
    pub anchor: Anchor,
    pub snippet: String,
    pub verified: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchSummary {
    pub corpus_id: CorpusId,
    pub query: String,
    pub total_candidates: u64,
    pub indexed_candidates: u64,
    pub fetched_candidates: u64,
    pub verified_matches: u64,
    pub coverage_status: CoverageStatus,
    pub coverage_counts: CoverageCounts,
}

#[cfg(test)]
mod tests {
    use super::{CoverageCounts, CoverageStatus, SearchSummary};
    use crate::domain::CorpusId;

    #[test]
    fn search_summary_retains_coverage_details() {
        let summary = SearchSummary {
            corpus_id: CorpusId("tier0.local_fs".to_owned()),
            query: "riverglass".to_owned(),
            total_candidates: 7,
            indexed_candidates: 0,
            fetched_candidates: 6,
            verified_matches: 1,
            coverage_status: CoverageStatus::Partial,
            coverage_counts: CoverageCounts {
                unsupported_count: 1,
                ..CoverageCounts::default()
            },
        };

        assert_eq!(summary.verified_matches, 1);
        assert_eq!(summary.coverage_status, CoverageStatus::Partial);
        assert_eq!(summary.coverage_counts.unsupported_count, 1);
    }
}

