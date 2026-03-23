use globset::{Glob, GlobSet, GlobSetBuilder};
use regex::Regex;
use thiserror::Error;

use crate::config::CorpusConfig;
use crate::contracts::{ContractError, Provider};
use crate::domain::{
    CoverageCounts, CoverageStatus, DocumentMeta, SearchMatch, SearchRequest, SearchResponse,
    SearchSummary,
};
use crate::extraction::ExtractorRegistry;
use crate::providers::provider_from_config;
use crate::runtime::{ConfigStore, RuntimePaths};
use crate::state::{ManifestDb, ManifestError};

#[derive(Debug, Error)]
pub enum SearchEngineError {
    #[error("{0}")]
    Manifest(#[from] ManifestError),
    #[error("{0}")]
    Runtime(#[from] crate::runtime::RuntimeError),
    #[error("{0}")]
    Contract(#[from] ContractError),
    #[error("invalid glob pattern: {0}")]
    InvalidGlob(String),
    #[error("invalid search pattern: {0}")]
    InvalidPattern(String),
}

pub fn filter_manifest_documents(
    paths: &RuntimePaths,
    request: &SearchRequest,
) -> Result<Vec<DocumentMeta>, SearchEngineError> {
    let manifest = ManifestDb::open(
        &crate::config::StateLayout::for_corpus(&paths.state_root, &request.corpus_id).manifest_db,
    )?;
    let documents = manifest.list_documents(&request.corpus_id.0)?;
    filter_documents(documents, request)
}

pub fn rank_documents(mut documents: Vec<DocumentMeta>, request: &SearchRequest) -> Vec<DocumentMeta> {
    let query_lower = request.query.to_ascii_lowercase();
    documents.sort_by(|left, right| {
        let left_score = ranking_key(left, &query_lower);
        let right_score = ranking_key(right, &query_lower);
        left_score.cmp(&right_score)
    });
    documents
}

pub fn filter_documents(
    documents: Vec<DocumentMeta>,
    request: &SearchRequest,
) -> Result<Vec<DocumentMeta>, SearchEngineError> {
    let glob_matcher = build_glob_matcher(&request.path_globs)?;
    let filtered = documents
        .into_iter()
        .filter(|document| matches_path_globs(&glob_matcher, &document.locator.path))
        .filter(|document| matches_extensions(request, document))
        .filter(|document| matches_content_types(request, document))
        .filter(|document| matches_size(request, document))
        .filter(|document| matches_modified_time(request, document))
        .collect();
    Ok(filtered)
}

pub fn execute_search(
    paths: &RuntimePaths,
    request: &SearchRequest,
) -> Result<SearchResponse, SearchEngineError> {
    let store = ConfigStore::new(paths.clone());
    let corpus = store
        .list_corpora()?
        .into_iter()
        .find(|candidate| candidate.id == request.corpus_id.0)
        .ok_or_else(|| SearchEngineError::InvalidPattern(format!("unknown corpus {}", request.corpus_id.0)))?;
    let provider = provider_from_config(&corpus.provider)?;
    let candidates = rank_documents(filter_manifest_documents(paths, request)?, request);
    execute_search_with_provider(request, &corpus, provider.as_ref(), candidates)
}

pub fn execute_search_with_provider(
    request: &SearchRequest,
    _corpus: &CorpusConfig,
    provider: &dyn Provider,
    candidates: Vec<DocumentMeta>,
) -> Result<SearchResponse, SearchEngineError> {
    let matcher = SearchMatcher::new(request)?;
    let mut matches = Vec::new();
    let mut fetched_candidates = 0_u64;
    let mut coverage_counts = CoverageCounts::default();
    let total_candidates = u64::try_from(candidates.len()).unwrap_or_default();

    for document in candidates {
        if !is_extractable_candidate(&document) {
            coverage_counts.unsupported_count += 1;
            continue;
        }

        fetched_candidates += 1;
        let payload = match provider.read(&document.id, None) {
            Ok(payload) => payload,
            Err(ContractError::NotFound(_)) => {
                coverage_counts.deleted_count += 1;
                continue;
            }
            Err(ContractError::PermissionDenied(_)) => {
                coverage_counts.denied_count += 1;
                continue;
            }
            Err(_) => {
                coverage_counts.failure_count += 1;
                continue;
            }
        };

        let canonical = match ExtractorRegistry::extract(&document, &payload.bytes, &[]) {
            Ok(canonical) => canonical,
            Err(ContractError::Unsupported(_)) => {
                coverage_counts.unsupported_count += 1;
                continue;
            }
            Err(_) => {
                coverage_counts.failure_count += 1;
                continue;
            }
        };

        for chunk in canonical.chunks {
            if !matcher.is_match(&chunk.text) {
                continue;
            }

            matches.push(SearchMatch {
                document_id: document.id.clone(),
                anchor: chunk.anchor,
                snippet: chunk.text,
                verified: true,
            });

            if let Some(limit) = request.limit
                && matches.len() >= usize::try_from(limit).unwrap_or(usize::MAX)
            {
                break;
            }
        }

        if let Some(limit) = request.limit
            && matches.len() >= usize::try_from(limit).unwrap_or(usize::MAX)
        {
            break;
        }
    }

    let coverage_status = if coverage_counts == CoverageCounts::default() {
        CoverageStatus::Complete
    } else {
        CoverageStatus::Partial
    };

    Ok(SearchResponse {
        request: request.clone(),
        matches: matches.clone(),
        summary: SearchSummary {
            corpus_id: request.corpus_id.clone(),
            query: request.query.clone(),
            total_candidates,
            indexed_candidates: 0,
            fetched_candidates,
            verified_matches: u64::try_from(matches.len()).unwrap_or_default(),
            coverage_status,
            coverage_counts,
        },
    })
}

fn ranking_key(document: &DocumentMeta, query_lower: &str) -> (u8, u8, u64, std::cmp::Reverse<u64>, String) {
    let path_lower = document.locator.path.to_ascii_lowercase();
    let path_matches_query = if path_lower.contains(query_lower) { 0 } else { 1 };
    let supported_text_rank = if is_extractable_candidate(document) { 0 } else { 1 };
    let size_rank = document.size_bytes;
    let modified_rank = std::cmp::Reverse(parse_modified_at(document.modified_at.as_deref()));
    (
        path_matches_query,
        supported_text_rank,
        size_rank,
        modified_rank,
        document.locator.path.clone(),
    )
}

fn build_glob_matcher(path_globs: &[String]) -> Result<Option<GlobSet>, SearchEngineError> {
    if path_globs.is_empty() {
        return Ok(None);
    }

    let mut builder = GlobSetBuilder::new();
    for path_glob in path_globs {
        let glob = Glob::new(path_glob).map_err(|error| SearchEngineError::InvalidGlob(error.to_string()))?;
        builder.add(glob);
    }

    builder
        .build()
        .map(Some)
        .map_err(|error| SearchEngineError::InvalidGlob(error.to_string()))
}

fn matches_path_globs(glob_matcher: &Option<GlobSet>, path: &str) -> bool {
    glob_matcher.as_ref().is_none_or(|matcher| matcher.is_match(path))
}

fn matches_extensions(request: &SearchRequest, document: &DocumentMeta) -> bool {
    if request.extensions.is_empty() {
        return true;
    }
    let extension = document.extension.as_deref().unwrap_or_default();
    request
        .extensions
        .iter()
        .any(|candidate| candidate.eq_ignore_ascii_case(extension))
}

fn matches_content_types(request: &SearchRequest, document: &DocumentMeta) -> bool {
    if request.content_types.is_empty() {
        return true;
    }
    let content_type = document.content_type.as_deref().unwrap_or_default();
    request
        .content_types
        .iter()
        .any(|candidate| candidate.eq_ignore_ascii_case(content_type))
}

fn matches_size(request: &SearchRequest, document: &DocumentMeta) -> bool {
    if let Some(size_min) = request.size_min
        && document.size_bytes < size_min
    {
        return false;
    }
    if let Some(size_max) = request.size_max
        && document.size_bytes > size_max
    {
        return false;
    }
    true
}

fn matches_modified_time(request: &SearchRequest, document: &DocumentMeta) -> bool {
    let Some(modified_at) = document.modified_at.as_deref() else {
        return request.modified_after.is_none() && request.modified_before.is_none();
    };
    let modified_value = Some(parse_modified_at(Some(modified_at)));
    let after_ok = match (request.modified_after.as_deref(), modified_value) {
        (Some(value), Some(modified)) => value.parse::<u64>().map(|after| modified >= after).unwrap_or(false),
        (Some(_), None) => false,
        (None, _) => true,
    };
    let before_ok = match (request.modified_before.as_deref(), modified_value) {
        (Some(value), Some(modified)) => value.parse::<u64>().map(|before| modified <= before).unwrap_or(false),
        (Some(_), None) => false,
        (None, _) => true,
    };
    after_ok && before_ok
}

fn is_extractable_candidate(document: &DocumentMeta) -> bool {
    if let Some(content_type) = document.content_type.as_deref() {
        if content_type.starts_with("text/") || content_type == "application/pdf" || content_type == "application/gzip" {
            return true;
        }
        if content_type.starts_with("application/vnd.openxmlformats-officedocument") {
            return true;
        }
    }

    matches!(
        document.extension.as_deref(),
        Some("csv" | "docx" | "gz" | "json" | "log" | "md" | "pdf" | "pptx" | "txt" | "xlsx" | "yaml" | "yml")
    )
}

fn parse_modified_at(modified_at: Option<&str>) -> u64 {
    modified_at
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or_default()
}

struct SearchMatcher {
    regex: Option<Regex>,
    query: String,
    fixed_strings: bool,
}

impl SearchMatcher {
    fn new(request: &SearchRequest) -> Result<Self, SearchEngineError> {
        let regex = if request.fixed_strings {
            None
        } else {
            Some(Regex::new(&request.query).map_err(|error| SearchEngineError::InvalidPattern(error.to_string()))?)
        };
        Ok(Self {
            regex,
            query: request.query.clone(),
            fixed_strings: request.fixed_strings,
        })
    }

    fn is_match(&self, line: &str) -> bool {
        if self.fixed_strings {
            line.contains(&self.query)
        } else {
            self.regex.as_ref().is_some_and(|regex| regex.is_match(line))
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::config::{CorpusConfig, ProviderConfig};
    use crate::contracts::{ByteRange, ContractError, Provider, ReadPayload, ResolvedDocument};
    use crate::domain::{
        CorpusId, DocumentId, DocumentLocator, SearchOutputFormat, SearchRequest,
    };

    use super::{execute_search_with_provider, filter_documents, rank_documents};

    fn request() -> SearchRequest {
        SearchRequest {
            corpus_id: CorpusId("local".to_owned()),
            query: "riverglass".to_owned(),
            fixed_strings: true,
            path_globs: vec!["docs/**".to_owned()],
            extensions: vec!["txt".to_owned()],
            content_types: vec!["text/plain".to_owned()],
            size_min: Some(10),
            size_max: Some(100),
            modified_after: Some("100".to_owned()),
            modified_before: Some("300".to_owned()),
            limit: None,
            output_format: SearchOutputFormat::Text,
        }
    }

    #[test]
    fn filter_documents_applies_path_and_metadata_predicates() {
        let filtered = filter_documents(
            vec![
                crate::domain::DocumentMeta {
                    id: DocumentId("docs/report.txt".to_owned()),
                    locator: DocumentLocator {
                        path: "docs/report.txt".to_owned(),
                    },
                    extension: Some("txt".to_owned()),
                    content_type: Some("text/plain".to_owned()),
                    version: Some("v1".to_owned()),
                    size_bytes: 42,
                    modified_at: Some("200".to_owned()),
                },
                crate::domain::DocumentMeta {
                    id: DocumentId("media/video.mp4".to_owned()),
                    locator: DocumentLocator {
                        path: "media/video.mp4".to_owned(),
                    },
                    extension: Some("mp4".to_owned()),
                    content_type: Some("video/mp4".to_owned()),
                    version: Some("v1".to_owned()),
                    size_bytes: 42,
                    modified_at: Some("200".to_owned()),
                },
            ],
            &request(),
        )
        .unwrap_or_else(|error| panic!("documents should filter: {error}"));

        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].locator.path, "docs/report.txt");
    }

    #[test]
    fn rank_documents_prefers_path_hits_text_and_smaller_files() {
        let request = request();
        let ranked = rank_documents(
            vec![
                crate::domain::DocumentMeta {
                    id: DocumentId("docs/large.txt".to_owned()),
                    locator: DocumentLocator {
                        path: "docs/large.txt".to_owned(),
                    },
                    extension: Some("txt".to_owned()),
                    content_type: Some("text/plain".to_owned()),
                    version: Some("v1".to_owned()),
                    size_bytes: 500,
                    modified_at: Some("100".to_owned()),
                },
                crate::domain::DocumentMeta {
                    id: DocumentId("docs/riverglass.txt".to_owned()),
                    locator: DocumentLocator {
                        path: "docs/riverglass.txt".to_owned(),
                    },
                    extension: Some("txt".to_owned()),
                    content_type: Some("text/plain".to_owned()),
                    version: Some("v1".to_owned()),
                    size_bytes: 50,
                    modified_at: Some("200".to_owned()),
                },
                crate::domain::DocumentMeta {
                    id: DocumentId("media/riverglass.mp4".to_owned()),
                    locator: DocumentLocator {
                        path: "media/riverglass.mp4".to_owned(),
                    },
                    extension: Some("mp4".to_owned()),
                    content_type: Some("video/mp4".to_owned()),
                    version: Some("v1".to_owned()),
                    size_bytes: 10,
                    modified_at: Some("300".to_owned()),
                },
            ],
            &request,
        );

        assert_eq!(ranked[0].locator.path, "docs/riverglass.txt");
        assert_eq!(ranked[1].locator.path, "media/riverglass.mp4");
    }

    struct TestProvider;

    impl Provider for TestProvider {
        fn list(&self, _prefix: &str, _cursor: Option<&str>) -> Result<crate::contracts::ListPage, ContractError> {
            Err(ContractError::Unsupported("list not used".to_owned()))
        }

        fn stat(&self, _document_id: &DocumentId) -> Result<crate::domain::DocumentMeta, ContractError> {
            Err(ContractError::Unsupported("stat not used".to_owned()))
        }

        fn read(&self, document_id: &DocumentId, _range: Option<ByteRange>) -> Result<ReadPayload, ContractError> {
            match document_id.0.as_str() {
                "docs/report.txt" => Ok(ReadPayload {
                    bytes: b"riverglass appears here\nanother line".to_vec(),
                }),
                "docs/report.gz" => {
                    use flate2::Compression;
                    use flate2::write::GzEncoder;
                    use std::io::Write;

                    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
                    encoder
                        .write_all(b"riverglass archived here\nanother line")
                        .unwrap_or_else(|error| panic!("gzip fixture should write: {error}"));
                    Ok(ReadPayload {
                        bytes: encoder
                            .finish()
                            .unwrap_or_else(|error| panic!("gzip fixture should finish: {error}")),
                    })
                }
                "docs/missing.txt" => Err(ContractError::NotFound("gone".to_owned())),
                "media/video.mp4" => Ok(ReadPayload { bytes: Vec::new() }),
                _ => Err(ContractError::Unsupported("unknown document".to_owned())),
            }
        }

        fn resolve(&self, _locator: &DocumentLocator) -> Result<ResolvedDocument, ContractError> {
            Err(ContractError::Unsupported("resolve not used".to_owned()))
        }
    }

    #[test]
    fn execute_search_fetches_text_candidates_and_verifies_matches() {
        let request = request();
        let response = execute_search_with_provider(
            &request,
            &CorpusConfig {
                id: "local".to_owned(),
                display_name: Some("Local".to_owned()),
                provider: ProviderConfig::LocalFs {
                    root: std::path::PathBuf::from("/tmp/docs"),
                },
                include_globs: Vec::new(),
                exclude_globs: Vec::new(),
                backend: None,
            },
            &TestProvider,
            vec![
                crate::domain::DocumentMeta {
                    id: DocumentId("docs/report.txt".to_owned()),
                    locator: DocumentLocator {
                        path: "docs/report.txt".to_owned(),
                    },
                    extension: Some("txt".to_owned()),
                    content_type: Some("text/plain".to_owned()),
                    version: Some("v1".to_owned()),
                    size_bytes: 42,
                    modified_at: Some("200".to_owned()),
                },
                crate::domain::DocumentMeta {
                    id: DocumentId("docs/report.gz".to_owned()),
                    locator: DocumentLocator {
                        path: "docs/report.gz".to_owned(),
                    },
                    extension: Some("gz".to_owned()),
                    content_type: Some("application/gzip".to_owned()),
                    version: Some("v1".to_owned()),
                    size_bytes: 42,
                    modified_at: Some("200".to_owned()),
                },
                crate::domain::DocumentMeta {
                    id: DocumentId("docs/missing.txt".to_owned()),
                    locator: DocumentLocator {
                        path: "docs/missing.txt".to_owned(),
                    },
                    extension: Some("txt".to_owned()),
                    content_type: Some("text/plain".to_owned()),
                    version: Some("v1".to_owned()),
                    size_bytes: 42,
                    modified_at: Some("200".to_owned()),
                },
                crate::domain::DocumentMeta {
                    id: DocumentId("media/video.mp4".to_owned()),
                    locator: DocumentLocator {
                        path: "media/video.mp4".to_owned(),
                    },
                    extension: Some("mp4".to_owned()),
                    content_type: Some("video/mp4".to_owned()),
                    version: Some("v1".to_owned()),
                    size_bytes: 42,
                    modified_at: Some("200".to_owned()),
                },
            ],
        )
        .unwrap_or_else(|error| panic!("search should execute: {error}"));

        assert_eq!(response.matches.len(), 2);
        assert_eq!(response.matches[0].document_id.0, "docs/report.txt");
        assert_eq!(response.matches[1].document_id.0, "docs/report.gz");
        assert_eq!(response.summary.fetched_candidates, 3);
        assert_eq!(response.summary.coverage_counts.deleted_count, 1);
        assert_eq!(response.summary.coverage_counts.unsupported_count, 1);
    }
}
