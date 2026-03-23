use crate::contracts::{ContractError, ExtractedChunk, ExtractedDocument, Extractor};
use crate::domain::{Anchor, AnchorKind, AnchorLocator, DocumentMeta};

pub struct PdfExtractor;

impl Extractor for PdfExtractor {
    fn can_handle(&self, meta: &DocumentMeta, sniff: &[u8]) -> bool {
        sniff.starts_with(b"%PDF-") || meta.extension.as_deref() == Some("pdf")
    }

    fn extract(&self, bytes: &[u8], meta: &DocumentMeta) -> Result<ExtractedDocument, ContractError> {
        let pages = pdf_extract::extract_text_from_mem_by_pages(bytes)
            .map_err(|error| ContractError::Unsupported(format!("pdf extraction failed: {error}")))?;

        let mut text = String::new();
        let mut chunks = Vec::new();

        for (page_index, page_text) in pages.into_iter().enumerate() {
            let normalized = normalize_whitespace(&page_text);
            if normalized.is_empty() {
                continue;
            }

            if !text.is_empty() {
                text.push_str("\n\n");
            }
            text.push_str(&normalized);

            for line in normalized.lines().filter(|line| !line.trim().is_empty()) {
                let anchor = Anchor {
                    kind: AnchorKind::PageSpan,
                    locator: AnchorLocator {
                        path: Some(meta.locator.path.clone()),
                        page: Some(u32::try_from(page_index + 1).unwrap_or(u32::MAX)),
                        ..AnchorLocator::default()
                    },
                };
                chunks.push(ExtractedChunk {
                    anchor,
                    text: line.to_owned(),
                });
            }
        }

        Ok(ExtractedDocument {
            text,
            chunks,
            warnings: Vec::new(),
            unsupported_reason: None,
        })
    }
}

pub fn default_extractors() -> Vec<Box<dyn Extractor>> {
    vec![Box::new(PdfExtractor)]
}

fn normalize_whitespace(value: &str) -> String {
    value
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use crate::contracts::Extractor;
    use crate::domain::{DocumentId, DocumentLocator, DocumentMeta};

    use super::PdfExtractor;

    #[test]
    fn pdf_extractor_emits_page_anchors() {
        let meta = DocumentMeta {
            id: DocumentId("docs/report.pdf".to_owned()),
            locator: DocumentLocator {
                path: "docs/report.pdf".to_owned(),
            },
            extension: Some("pdf".to_owned()),
            content_type: Some("application/pdf".to_owned()),
            version: Some("v1".to_owned()),
            size_bytes: 0,
            modified_at: Some("100".to_owned()),
        };

        let document = PdfExtractor
            .extract(&build_test_pdf(&["Riverglass PDF page one", "Page two archive"]), &meta)
            .unwrap_or_else(|error| panic!("pdf extraction should succeed: {error}"));

        assert_eq!(document.chunks.len(), 2);
        assert_eq!(document.chunks[0].anchor.locator.page, Some(1));
        assert_eq!(document.chunks[1].anchor.locator.page, Some(2));
        assert!(document.text.contains("Riverglass PDF page one"));
    }

    fn build_test_pdf(pages: &[&str]) -> Vec<u8> {
        let mut objects = Vec::new();
        let mut page_ids = Vec::new();
        let font_id = u32::try_from(3 + (pages.len() * 2)).unwrap_or(u32::MAX);

        objects.push((1_u32, "<< /Type /Catalog /Pages 2 0 R >>".to_owned()));

        for (index, page_text) in pages.iter().enumerate() {
            let page_id = u32::try_from(3 + (index * 2)).unwrap_or(u32::MAX);
            let content_id = page_id + 1;
            page_ids.push(format!("{page_id} 0 R"));

            let page_object = format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents {content_id} 0 R /Resources << /Font << /F1 {font_id} 0 R >> >> >>"
            );
            objects.push((page_id, page_object));

            let stream = format!(
                "BT\n/F1 18 Tf\n72 720 Td\n({}) Tj\nET",
                escape_pdf_text(page_text)
            );
            let content_object = format!(
                "<< /Length {} >>\nstream\n{stream}\nendstream",
                stream.len()
            );
            objects.push((content_id, content_object));
        }

        objects.insert(
            1,
            (
                2_u32,
                format!("<< /Type /Pages /Kids [{}] /Count {} >>", page_ids.join(" "), pages.len()),
            ),
        );
        objects.push((font_id, "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_owned()));

        let mut buffer = Vec::new();
        buffer.extend_from_slice(b"%PDF-1.4\n");
        let mut offsets = vec![0_usize];
        for (id, body) in &objects {
            offsets.push(buffer.len());
            buffer.extend_from_slice(format!("{id} 0 obj\n{body}\nendobj\n").as_bytes());
        }

        let xref_offset = buffer.len();
        buffer.extend_from_slice(format!("xref\n0 {}\n", objects.len() + 1).as_bytes());
        buffer.extend_from_slice(b"0000000000 65535 f \n");
        for offset in offsets.iter().skip(1) {
            buffer.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        buffer.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref_offset}\n%%EOF\n",
                objects.len() + 1
            )
            .as_bytes(),
        );
        buffer
    }

    fn escape_pdf_text(value: &str) -> String {
        value
            .replace('\\', "\\\\")
            .replace('(', "\\(")
            .replace(')', "\\)")
    }
}
