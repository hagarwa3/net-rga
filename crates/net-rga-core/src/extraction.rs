use crate::contracts::{ContractError, ExtractedDocument, Extractor};
use crate::domain::{CanonicalChunk, CanonicalContentKind, CanonicalDocument, DocumentMeta};

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

    std::str::from_utf8(bytes).is_ok()
}

pub fn unsupported_document(reason: impl Into<String>) -> Result<CanonicalDocument, ContractError> {
    Err(ContractError::Unsupported(reason.into()))
}

#[cfg(test)]
mod tests {
    use crate::domain::{DocumentId, DocumentLocator, DocumentMeta};

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
}
