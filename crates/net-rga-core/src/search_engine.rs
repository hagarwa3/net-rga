use globset::{Glob, GlobSet, GlobSetBuilder};
use thiserror::Error;

use crate::domain::{DocumentMeta, SearchRequest};
use crate::runtime::RuntimePaths;
use crate::state::{ManifestDb, ManifestError};

#[derive(Debug, Error)]
pub enum SearchEngineError {
    #[error("{0}")]
    Manifest(#[from] ManifestError),
    #[error("invalid glob pattern: {0}")]
    InvalidGlob(String),
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

fn ranking_key(document: &DocumentMeta, query_lower: &str) -> (u8, u8, u64, std::cmp::Reverse<u64>, String) {
    let path_lower = document.locator.path.to_ascii_lowercase();
    let path_matches_query = if path_lower.contains(query_lower) { 0 } else { 1 };
    let supported_text_rank = if is_text_likely(document) { 0 } else { 1 };
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

fn is_text_likely(document: &DocumentMeta) -> bool {
    if let Some(content_type) = document.content_type.as_deref()
        && content_type.starts_with("text/")
    {
        return true;
    }

    matches!(
        document.extension.as_deref(),
        Some("csv" | "json" | "log" | "md" | "txt")
    )
}

fn parse_modified_at(modified_at: Option<&str>) -> u64 {
    modified_at
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use crate::domain::{CorpusId, DocumentId, DocumentLocator, SearchOutputFormat, SearchRequest};

    use super::{filter_documents, rank_documents};

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
}
