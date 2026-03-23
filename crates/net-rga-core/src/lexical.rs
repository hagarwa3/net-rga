use grep_regex::{RegexMatcher, RegexMatcherBuilder};
use grep_searcher::{Searcher, SearcherBuilder, sinks::UTF8};
use thiserror::Error;

use crate::domain::SearchRequest;

#[derive(Debug, Error)]
pub enum LexicalError {
    #[error("invalid search pattern: {0}")]
    InvalidPattern(String),
    #[error("search runtime error: {0}")]
    SearchRuntime(String),
}

pub struct LexicalVerifier {
    matcher: RegexMatcher,
    searcher: Searcher,
}

impl LexicalVerifier {
    pub fn new(request: &SearchRequest) -> Result<Self, LexicalError> {
        let matcher = RegexMatcherBuilder::new()
            .fixed_strings(request.fixed_strings)
            .build(&request.query)
            .map_err(|error| LexicalError::InvalidPattern(error.to_string()))?;
        let searcher = SearcherBuilder::new().line_number(true).build();
        Ok(Self { matcher, searcher })
    }

    pub fn first_matching_snippet(&mut self, text: &str) -> Result<Option<String>, LexicalError> {
        let mut snippet = None;
        self.searcher
            .search_slice(&self.matcher, text.as_bytes(), UTF8(|_line_number, line| {
                snippet = Some(trim_line_terminator(line).to_owned());
                Ok(false)
            }))
            .map_err(|error| LexicalError::SearchRuntime(error.to_string()))?;
        Ok(snippet)
    }
}

fn trim_line_terminator(value: &str) -> &str {
    value.trim_end_matches(['\n', '\r'])
}

#[cfg(test)]
mod tests {
    use crate::domain::{CorpusId, SearchOutputFormat, SearchRequest};

    use super::{LexicalError, LexicalVerifier};

    fn request(query: &str, fixed_strings: bool) -> SearchRequest {
        SearchRequest {
            corpus_id: CorpusId("local".to_owned()),
            query: query.to_owned(),
            fixed_strings,
            path_globs: Vec::new(),
            extensions: Vec::new(),
            content_types: Vec::new(),
            size_min: None,
            size_max: None,
            modified_after: None,
            modified_before: None,
            limit: None,
            output_format: SearchOutputFormat::Text,
        }
    }

    #[test]
    fn fixed_string_verifier_returns_matching_line() {
        let mut verifier = LexicalVerifier::new(&request("riverglass", true))
            .unwrap_or_else(|error| panic!("matcher should build: {error}"));

        let snippet = verifier
            .first_matching_snippet("first line\nriverglass appears here\nthird line")
            .unwrap_or_else(|error| panic!("search should succeed: {error}"));

        assert_eq!(snippet.as_deref(), Some("riverglass appears here"));
    }

    #[test]
    fn regex_verifier_returns_first_matching_line() {
        let mut verifier = LexicalVerifier::new(&request(r"river\w+\s+appears", false))
            .unwrap_or_else(|error| panic!("regex should build: {error}"));

        let snippet = verifier
            .first_matching_snippet("alpha\nriverglass appears here\nriverbank appears elsewhere")
            .unwrap_or_else(|error| panic!("search should succeed: {error}"));

        assert_eq!(snippet.as_deref(), Some("riverglass appears here"));
    }

    #[test]
    fn invalid_pattern_is_reported() {
        let error = LexicalVerifier::new(&request("(", false))
            .err()
            .unwrap_or_else(|| panic!("invalid regex should fail"));

        assert!(matches!(error, LexicalError::InvalidPattern(_)));
    }
}
