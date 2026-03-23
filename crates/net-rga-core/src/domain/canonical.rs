use serde::{Deserialize, Serialize};

use crate::domain::{Anchor, DocumentId, DocumentLocator};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CanonicalContentKind {
    Text,
    Pdf,
    Document,
    Presentation,
    Spreadsheet,
    ArchiveText,
    Other,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CanonicalChunk {
    pub anchor: Anchor,
    pub anchor_ref: String,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CanonicalDocument {
    pub document_id: DocumentId,
    pub locator: DocumentLocator,
    pub content_kind: CanonicalContentKind,
    pub text: String,
    pub chunks: Vec<CanonicalChunk>,
    pub warnings: Vec<String>,
}

#[cfg(test)]
mod tests {
    use crate::domain::{AnchorKind, AnchorLocator};

    use super::{CanonicalChunk, CanonicalContentKind, CanonicalDocument};

    #[test]
    fn canonical_document_retains_chunk_anchor_refs() {
        let anchor = crate::domain::Anchor {
            kind: AnchorKind::PageSpan,
            locator: AnchorLocator {
                path: Some("docs/report.pdf".to_owned()),
                page: Some(2),
                ..AnchorLocator::default()
            },
        };
        let anchor_ref = anchor.stable_ref();
        let document = CanonicalDocument {
            document_id: crate::domain::DocumentId("docs/report.pdf".to_owned()),
            locator: crate::domain::DocumentLocator {
                path: "docs/report.pdf".to_owned(),
            },
            content_kind: CanonicalContentKind::Pdf,
            text: "Riverglass appears on page two.".to_owned(),
            chunks: vec![CanonicalChunk {
                anchor,
                anchor_ref: anchor_ref.clone(),
                text: "Riverglass appears on page two.".to_owned(),
            }],
            warnings: Vec::new(),
        };

        assert_eq!(document.content_kind, CanonicalContentKind::Pdf);
        assert_eq!(document.chunks[0].anchor_ref, anchor_ref);
        assert!(document.text.contains("Riverglass"));
    }
}
