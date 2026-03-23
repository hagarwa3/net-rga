use std::io::Read;

use flate2::read::GzDecoder;

use crate::contracts::{ContractError, ExtractedDocument, Extractor};
use crate::domain::{
    Anchor, AnchorKind, AnchorLocator, CanonicalChunk, CanonicalContentKind, CanonicalDocument,
    DocumentMeta,
};
use crate::extractor_adapters::default_extractors;

impl CanonicalDocument {
    pub fn from_extracted(
        meta: &DocumentMeta,
        content_kind: CanonicalContentKind,
        value: ExtractedDocument,
    ) -> Self {
        Self {
            document_id: meta.id.clone(),
            locator: meta.locator.clone(),
            content_kind,
            text: value.text,
            chunks: value
                .chunks
                .into_iter()
                .map(|chunk| CanonicalChunk {
                    anchor_ref: chunk.anchor.stable_ref(),
                    anchor: chunk.anchor,
                    text: chunk.text,
                })
                .collect(),
            warnings: value
                .warnings
                .into_iter()
                .map(|warning| warning.message)
                .collect(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExtractionPlan {
    PlainText,
    GzipText,
    Pdf,
    Docx,
    Pptx,
    Xlsx,
    Unsupported(String),
}

pub struct ExtractorRegistry;

impl ExtractorRegistry {
    pub fn sniff(meta: &DocumentMeta, bytes: &[u8]) -> ExtractionPlan {
        if bytes.starts_with(b"%PDF-") || meta.extension.as_deref() == Some("pdf") {
            return ExtractionPlan::Pdf;
        }
        if bytes.starts_with(&[0x1F, 0x8B]) || meta.extension.as_deref() == Some("gz") {
            return ExtractionPlan::GzipText;
        }
        if bytes.starts_with(b"PK\x03\x04") {
            return match meta.extension.as_deref() {
                Some("docx") => ExtractionPlan::Docx,
                Some("pptx") => ExtractionPlan::Pptx,
                Some("xlsx") => ExtractionPlan::Xlsx,
                _ => ExtractionPlan::Unsupported("zip container is not a supported document type".to_owned()),
            };
        }
        if is_plain_text(meta, bytes) {
            return ExtractionPlan::PlainText;
        }
        ExtractionPlan::Unsupported("no extractor available for document".to_owned())
    }

    pub fn dispatch<'a>(
        meta: &DocumentMeta,
        bytes: &'a [u8],
        extractors: &'a [Box<dyn Extractor>],
    ) -> Option<&'a dyn Extractor> {
        extractors
            .iter()
            .find(|extractor| extractor.can_handle(meta, bytes))
            .map(|extractor| extractor.as_ref())
    }

    pub fn extract(meta: &DocumentMeta, bytes: &[u8], extractors: &[Box<dyn Extractor>]) -> Result<CanonicalDocument, ContractError> {
        let fallback_extractors;
        let extractor_slice = if extractors.is_empty() {
            fallback_extractors = default_extractors();
            fallback_extractors.as_slice()
        } else {
            extractors
        };

        match Self::sniff(meta, bytes) {
            ExtractionPlan::PlainText => extract_plain_text(meta, bytes, CanonicalContentKind::Text),
            ExtractionPlan::GzipText => extract_gzip_text(meta, bytes),
            ExtractionPlan::Pdf => extract_with(meta, bytes, extractor_slice, CanonicalContentKind::Pdf),
            ExtractionPlan::Docx => extract_with(meta, bytes, extractor_slice, CanonicalContentKind::Document),
            ExtractionPlan::Pptx => extract_with(meta, bytes, extractor_slice, CanonicalContentKind::Presentation),
            ExtractionPlan::Xlsx => extract_with(meta, bytes, extractor_slice, CanonicalContentKind::Spreadsheet),
            ExtractionPlan::Unsupported(reason) => unsupported_document(reason),
        }
    }
}

fn is_plain_text(meta: &DocumentMeta, bytes: &[u8]) -> bool {
    if let Some(content_type) = meta.content_type.as_deref()
        && content_type.starts_with("text/")
    {
        return true;
    }

    if matches!(
        meta.extension.as_deref(),
        Some("csv" | "json" | "log" | "md" | "txt" | "yaml" | "yml")
    ) {
        return true;
    }

    !bytes.is_empty() && std::str::from_utf8(bytes).is_ok()
}

pub fn unsupported_document(reason: impl Into<String>) -> Result<CanonicalDocument, ContractError> {
    Err(ContractError::Unsupported(reason.into()))
}

fn extract_plain_text(
    meta: &DocumentMeta,
    bytes: &[u8],
    content_kind: CanonicalContentKind,
) -> Result<CanonicalDocument, ContractError> {
    let text = match String::from_utf8(bytes.to_vec()) {
        Ok(text) => text,
        Err(_) => String::from_utf8_lossy(bytes).into_owned(),
    };
    Ok(canonicalize_text(meta, &text, content_kind, Vec::new()))
}

fn extract_gzip_text(meta: &DocumentMeta, bytes: &[u8]) -> Result<CanonicalDocument, ContractError> {
    let mut decoder = GzDecoder::new(bytes);
    let mut decoded = Vec::new();
    decoder
        .read_to_end(&mut decoded)
        .map_err(|error| ContractError::Io(error.to_string()))?;
    extract_plain_text(meta, &decoded, CanonicalContentKind::ArchiveText)
}

fn extract_with(
    meta: &DocumentMeta,
    bytes: &[u8],
    extractors: &[Box<dyn Extractor>],
    content_kind: CanonicalContentKind,
) -> Result<CanonicalDocument, ContractError> {
    let extractor = ExtractorRegistry::dispatch(meta, bytes, extractors)
        .ok_or_else(|| ContractError::Unsupported("no matching extractor available".to_owned()))?;
    let extracted = extractor.extract(bytes, meta)?;
    Ok(CanonicalDocument::from_extracted(meta, content_kind, extracted))
}

fn canonicalize_text(
    meta: &DocumentMeta,
    text: &str,
    content_kind: CanonicalContentKind,
    warnings: Vec<String>,
) -> CanonicalDocument {
    let chunks = text
        .lines()
        .enumerate()
        .map(|(line_index, line)| {
            let anchor = Anchor {
                kind: AnchorKind::LineSpan,
                locator: AnchorLocator {
                    path: Some(meta.locator.path.clone()),
                    line_start: Some(u32::try_from(line_index + 1).unwrap_or(u32::MAX)),
                    line_end: Some(u32::try_from(line_index + 1).unwrap_or(u32::MAX)),
                    ..AnchorLocator::default()
                },
            };
            CanonicalChunk {
                anchor_ref: anchor.stable_ref(),
                anchor,
                text: line.to_owned(),
            }
        })
        .collect();

    CanonicalDocument {
        document_id: meta.id.clone(),
        locator: meta.locator.clone(),
        content_kind,
        text: text.to_owned(),
        chunks,
        warnings,
    }
}

#[cfg(test)]
mod tests {
    use flate2::Compression;
    use flate2::write::GzEncoder;

    use std::io::Write;

    use crate::domain::{CanonicalContentKind, DocumentId, DocumentLocator, DocumentMeta};

    use super::{ExtractionPlan, ExtractorRegistry};

    fn meta(path: &str, extension: Option<&str>, content_type: Option<&str>) -> DocumentMeta {
        DocumentMeta {
            id: DocumentId(path.to_owned()),
            locator: DocumentLocator {
                path: path.to_owned(),
            },
            extension: extension.map(ToOwned::to_owned),
            content_type: content_type.map(ToOwned::to_owned),
            version: Some("v1".to_owned()),
            size_bytes: 0,
            modified_at: Some("100".to_owned()),
        }
    }

    #[test]
    fn sniffing_identifies_supported_document_families() {
        assert_eq!(
            ExtractorRegistry::sniff(&meta("docs/report.txt", Some("txt"), Some("text/plain")), b"riverglass"),
            ExtractionPlan::PlainText
        );
        assert_eq!(
            ExtractorRegistry::sniff(&meta("docs/report.gz", Some("gz"), None), &[0x1F, 0x8B, 0x08]),
            ExtractionPlan::GzipText
        );
        assert_eq!(
            ExtractorRegistry::sniff(&meta("docs/report.pdf", Some("pdf"), None), b"%PDF-1.7"),
            ExtractionPlan::Pdf
        );
        assert_eq!(
            ExtractorRegistry::sniff(&meta("docs/report.docx", Some("docx"), None), b"PK\x03\x04"),
            ExtractionPlan::Docx
        );
    }

    #[test]
    fn extracts_plain_text_into_line_chunks() {
        let canonical = ExtractorRegistry::extract(
            &meta("docs/report.txt", Some("txt"), Some("text/plain")),
            b"riverglass appears here\nsecond line",
            &[],
        )
        .unwrap_or_else(|error| panic!("text extraction should succeed: {error}"));

        assert_eq!(canonical.document_id.0, "docs/report.txt");
        assert_eq!(canonical.chunks.len(), 2);
        assert_eq!(canonical.chunks[0].anchor.locator.line_start, Some(1));
        assert_eq!(canonical.chunks[1].text, "second line");
    }

    #[test]
    fn extracts_gzip_text_into_searchable_chunks() {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder
            .write_all(b"riverglass archive\nsecond line")
            .unwrap_or_else(|error| panic!("gzip fixture should write: {error}"));
        let compressed = encoder
            .finish()
            .unwrap_or_else(|error| panic!("gzip fixture should finish: {error}"));

        let canonical = ExtractorRegistry::extract(
            &meta("docs/report.gz", Some("gz"), Some("application/gzip")),
            &compressed,
            &[],
        )
        .unwrap_or_else(|error| panic!("gzip extraction should succeed: {error}"));

        assert_eq!(canonical.content_kind, CanonicalContentKind::ArchiveText);
        assert_eq!(canonical.chunks.len(), 2);
        assert!(canonical.text.contains("riverglass archive"));
    }
}
