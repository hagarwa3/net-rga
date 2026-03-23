use serde::{Deserialize, Serialize};

use crate::domain::{Anchor, CorpusId, DocumentId};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchOutputFormat {
    Text,
    Json,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchRequest {
    pub corpus_id: CorpusId,
    pub query: String,
    pub fixed_strings: bool,
    pub path_globs: Vec<String>,
    pub extensions: Vec<String>,
    pub content_types: Vec<String>,
    pub size_min: Option<u64>,
    pub size_max: Option<u64>,
    pub modified_after: Option<String>,
    pub modified_before: Option<String>,
    pub limit: Option<u32>,
    pub output_format: SearchOutputFormat,
}

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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchResponse {
    pub request: SearchRequest,
    pub matches: Vec<SearchMatch>,
    pub summary: SearchSummary,
}

#[cfg(test)]
mod tests {
    use super::{CoverageCounts, CoverageStatus, SearchOutputFormat, SearchRequest, SearchSummary};
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

    #[test]
    fn search_request_captures_output_and_filter_preferences() {
        let request = SearchRequest {
            corpus_id: CorpusId("local".to_owned()),
            query: "riverglass".to_owned(),
            fixed_strings: true,
            path_globs: vec!["docs/**".to_owned()],
            extensions: vec!["txt".to_owned()],
            content_types: vec!["text/plain".to_owned()],
            size_min: Some(16),
            size_max: Some(4096),
            modified_after: Some("1000".to_owned()),
            modified_before: Some("2000".to_owned()),
            limit: Some(5),
            output_format: SearchOutputFormat::Json,
        };

        assert_eq!(request.corpus_id.0, "local");
        assert!(request.fixed_strings);
        assert_eq!(request.path_globs, vec!["docs/**"]);
        assert_eq!(request.output_format, SearchOutputFormat::Json);
    }
}
