use std::io::{Cursor, Read};

use quick_xml::Reader;
use quick_xml::events::Event;
use zip::ZipArchive;

use crate::contracts::{ContractError, ExtractedChunk, ExtractedDocument, Extractor};
use crate::domain::{Anchor, AnchorKind, AnchorLocator, DocumentMeta};

pub struct PdfExtractor;
pub struct OfficeExtractor;

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

impl Extractor for OfficeExtractor {
    fn can_handle(&self, meta: &DocumentMeta, sniff: &[u8]) -> bool {
        sniff.starts_with(b"PK\x03\x04")
            && matches!(meta.extension.as_deref(), Some("docx" | "pptx" | "xlsx"))
    }

    fn extract(&self, bytes: &[u8], meta: &DocumentMeta) -> Result<ExtractedDocument, ContractError> {
        let mut archive = ZipArchive::new(Cursor::new(bytes))
            .map_err(|error| ContractError::Unsupported(format!("office archive open failed: {error}")))?;

        match meta.extension.as_deref() {
            Some("docx") => extract_docx(&mut archive, meta),
            Some("pptx") => extract_pptx(&mut archive, meta),
            Some("xlsx") => extract_xlsx(&mut archive, meta),
            _ => Err(ContractError::Unsupported("office extractor does not support this extension".to_owned())),
        }
    }
}

pub fn default_extractors() -> Vec<Box<dyn Extractor>> {
    vec![Box::new(PdfExtractor), Box::new(OfficeExtractor)]
}

fn normalize_whitespace(value: &str) -> String {
    value
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn extract_docx<R: std::io::Read + std::io::Seek>(
    archive: &mut ZipArchive<R>,
    meta: &DocumentMeta,
) -> Result<ExtractedDocument, ContractError> {
    let xml = read_zip_text(archive, "word/document.xml")?;
    let paragraphs = extract_docx_paragraphs(&xml)?;
    let mut text = String::new();
    let mut chunks = Vec::new();

    for (index, paragraph) in paragraphs.into_iter().enumerate() {
        if paragraph.is_empty() {
            continue;
        }
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(&paragraph);
        chunks.push(ExtractedChunk {
            anchor: Anchor {
                kind: AnchorKind::ChunkSpan,
                locator: AnchorLocator {
                    path: Some(meta.locator.path.clone()),
                    chunk_id: Some(format!("paragraph-{}", index + 1)),
                    ..AnchorLocator::default()
                },
            },
            text: paragraph,
        });
    }

    Ok(ExtractedDocument {
        text,
        chunks,
        warnings: Vec::new(),
        unsupported_reason: None,
    })
}

fn extract_pptx<R: std::io::Read + std::io::Seek>(
    archive: &mut ZipArchive<R>,
    meta: &DocumentMeta,
) -> Result<ExtractedDocument, ContractError> {
    let mut slide_names = archive
        .file_names()
        .filter(|name| name.starts_with("ppt/slides/slide") && name.ends_with(".xml"))
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    slide_names.sort();

    let mut text = String::new();
    let mut chunks = Vec::new();
    for (index, slide_name) in slide_names.into_iter().enumerate() {
        let xml = read_zip_text(archive, &slide_name)?;
        let slide_text = normalize_whitespace(&extract_text_from_tags(&xml, &["t"], &["p"])?);
        if slide_text.is_empty() {
            continue;
        }
        if !text.is_empty() {
            text.push_str("\n\n");
        }
        text.push_str(&slide_text);
        chunks.push(ExtractedChunk {
            anchor: Anchor {
                kind: AnchorKind::SlideRegion,
                locator: AnchorLocator {
                    path: Some(meta.locator.path.clone()),
                    slide: Some(u32::try_from(index + 1).unwrap_or(u32::MAX)),
                    ..AnchorLocator::default()
                },
            },
            text: slide_text,
        });
    }

    Ok(ExtractedDocument {
        text,
        chunks,
        warnings: Vec::new(),
        unsupported_reason: None,
    })
}

fn extract_xlsx<R: std::io::Read + std::io::Seek>(
    archive: &mut ZipArchive<R>,
    meta: &DocumentMeta,
) -> Result<ExtractedDocument, ContractError> {
    let workbook_xml = read_zip_text(archive, "xl/workbook.xml")?;
    let sheet_names = extract_sheet_names(&workbook_xml)?;
    let shared_strings = match read_zip_text_optional(archive, "xl/sharedStrings.xml")? {
        Some(xml) => extract_shared_strings(&xml)?,
        None => Vec::new(),
    };

    let mut text = String::new();
    let mut chunks = Vec::new();

    for (index, sheet_name) in sheet_names.iter().enumerate() {
        let path = format!("xl/worksheets/sheet{}.xml", index + 1);
        let Some(sheet_xml) = read_zip_text_optional(archive, &path)? else {
            continue;
        };

        let cells = extract_sheet_cells(&sheet_xml, &shared_strings)?;
        for (cell_ref, value) in cells {
            if value.trim().is_empty() {
                continue;
            }
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str(&value);
            chunks.push(ExtractedChunk {
                anchor: Anchor {
                    kind: AnchorKind::SheetRange,
                    locator: AnchorLocator {
                        path: Some(meta.locator.path.clone()),
                        sheet: Some(sheet_name.clone()),
                        cell_range: Some(cell_ref),
                        ..AnchorLocator::default()
                    },
                },
                text: value,
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

fn read_zip_text<R: std::io::Read + std::io::Seek>(
    archive: &mut ZipArchive<R>,
    path: &str,
) -> Result<String, ContractError> {
    read_zip_text_optional(archive, path)?
        .ok_or_else(|| ContractError::Unsupported(format!("missing archive entry: {path}")))
}

fn read_zip_text_optional<R: std::io::Read + std::io::Seek>(
    archive: &mut ZipArchive<R>,
    path: &str,
) -> Result<Option<String>, ContractError> {
    let mut file = match archive.by_name(path) {
        Ok(file) => file,
        Err(zip::result::ZipError::FileNotFound) => return Ok(None),
        Err(error) => {
            return Err(ContractError::Unsupported(format!(
                "archive entry open failed for {path}: {error}"
            )))
        }
    };
    let mut xml = String::new();
    file.read_to_string(&mut xml)
        .map_err(|error| ContractError::Io(error.to_string()))?;
    Ok(Some(xml))
}

fn extract_docx_paragraphs(xml: &str) -> Result<Vec<String>, ContractError> {
    let mut reader = xml_reader(xml);
    let mut buffer = Vec::new();
    let mut paragraphs = Vec::new();
    let mut current = String::new();
    let mut in_text = false;

    loop {
        match reader.read_event_into(&mut buffer) {
            Ok(Event::Start(event)) => {
                if name_eq(event.name().as_ref(), b"t") {
                    in_text = true;
                } else if name_eq(event.name().as_ref(), b"tab") {
                    current.push('\t');
                }
            }
            Ok(Event::Empty(event)) => {
                if name_eq(event.name().as_ref(), b"br") {
                    current.push('\n');
                } else if name_eq(event.name().as_ref(), b"tab") {
                    current.push('\t');
                }
            }
            Ok(Event::Text(text_event)) => {
                if in_text {
                    let decoded = text_event
                        .decode()
                        .map_err(|error| ContractError::Unsupported(format!("xml decode failed: {error}")))?;
                    current.push_str(&decoded);
                }
            }
            Ok(Event::End(event)) => {
                if name_eq(event.name().as_ref(), b"t") {
                    in_text = false;
                } else if name_eq(event.name().as_ref(), b"p") {
                    paragraphs.push(normalize_whitespace(&current));
                    current.clear();
                }
            }
            Ok(Event::Eof) => break,
            Err(error) => {
                return Err(ContractError::Unsupported(format!(
                    "docx xml parse failed: {error}"
                )))
            }
            _ => {}
        }
        buffer.clear();
    }

    Ok(paragraphs)
}

fn extract_text_from_tags(xml: &str, text_tags: &[&str], paragraph_tags: &[&str]) -> Result<String, ContractError> {
    let mut reader = xml_reader(xml);
    let mut buffer = Vec::new();
    let mut output = String::new();
    let mut in_text = false;

    loop {
        match reader.read_event_into(&mut buffer) {
            Ok(Event::Start(event)) => {
                if name_in(event.name().as_ref(), text_tags) {
                    in_text = true;
                }
            }
            Ok(Event::Text(text_event)) => {
                if in_text {
                    let decoded = text_event
                        .decode()
                        .map_err(|error| ContractError::Unsupported(format!("xml decode failed: {error}")))?;
                    output.push_str(&decoded);
                }
            }
            Ok(Event::End(event)) => {
                if name_in(event.name().as_ref(), text_tags) {
                    in_text = false;
                } else if name_in(event.name().as_ref(), paragraph_tags) {
                    output.push('\n');
                }
            }
            Ok(Event::Eof) => break,
            Err(error) => {
                return Err(ContractError::Unsupported(format!(
                    "pptx xml parse failed: {error}"
                )))
            }
            _ => {}
        }
        buffer.clear();
    }

    Ok(output)
}

fn extract_sheet_names(xml: &str) -> Result<Vec<String>, ContractError> {
    let mut reader = xml_reader(xml);
    let mut buffer = Vec::new();
    let mut names = Vec::new();

    loop {
        match reader.read_event_into(&mut buffer) {
            Ok(Event::Start(event)) | Ok(Event::Empty(event)) => {
                if name_eq(event.name().as_ref(), b"sheet") {
                    for attribute in event.attributes().with_checks(false) {
                        let attribute = attribute.map_err(|error| {
                            ContractError::Unsupported(format!("sheet attribute parse failed: {error}"))
                        })?;
                        if local_name(attribute.key.as_ref()) == b"name" {
                            let value = attribute
                                .decode_and_unescape_value(reader.decoder())
                                .map_err(|error| {
                                    ContractError::Unsupported(format!("sheet name decode failed: {error}"))
                                })?;
                            names.push(value.into_owned());
                        }
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(error) => {
                return Err(ContractError::Unsupported(format!(
                    "workbook xml parse failed: {error}"
                )))
            }
            _ => {}
        }
        buffer.clear();
    }

    Ok(names)
}

fn extract_shared_strings(xml: &str) -> Result<Vec<String>, ContractError> {
    let mut reader = xml_reader(xml);
    let mut buffer = Vec::new();
    let mut strings = Vec::new();
    let mut current = String::new();
    let mut in_shared_string = false;
    let mut in_text = false;

    loop {
        match reader.read_event_into(&mut buffer) {
            Ok(Event::Start(event)) => {
                if name_eq(event.name().as_ref(), b"si") {
                    in_shared_string = true;
                    current.clear();
                } else if name_eq(event.name().as_ref(), b"t") {
                    in_text = true;
                }
            }
            Ok(Event::Text(text_event)) => {
                if in_shared_string && in_text {
                    let decoded = text_event
                        .decode()
                        .map_err(|error| ContractError::Unsupported(format!("shared string decode failed: {error}")))?;
                    current.push_str(&decoded);
                }
            }
            Ok(Event::End(event)) => {
                if name_eq(event.name().as_ref(), b"t") {
                    in_text = false;
                } else if name_eq(event.name().as_ref(), b"si") {
                    strings.push(current.clone());
                    current.clear();
                    in_shared_string = false;
                }
            }
            Ok(Event::Eof) => break,
            Err(error) => {
                return Err(ContractError::Unsupported(format!(
                    "shared strings parse failed: {error}"
                )))
            }
            _ => {}
        }
        buffer.clear();
    }

    Ok(strings)
}

fn extract_sheet_cells(
    xml: &str,
    shared_strings: &[String],
) -> Result<Vec<(String, String)>, ContractError> {
    let mut reader = xml_reader(xml);
    let mut buffer = Vec::new();
    let mut cells = Vec::new();
    let mut current_ref: Option<String> = None;
    let mut current_type: Option<String> = None;
    let mut current_value = String::new();
    let mut in_value = false;
    let mut in_inline_text = false;

    loop {
        match reader.read_event_into(&mut buffer) {
            Ok(Event::Start(event)) => {
                if name_eq(event.name().as_ref(), b"c") {
                    current_ref = None;
                    current_type = None;
                    current_value.clear();
                    for attribute in event.attributes().with_checks(false) {
                        let attribute = attribute.map_err(|error| {
                            ContractError::Unsupported(format!("cell attribute parse failed: {error}"))
                        })?;
                        let key = local_name(attribute.key.as_ref());
                        let value = attribute
                            .decode_and_unescape_value(reader.decoder())
                            .map_err(|error| {
                                ContractError::Unsupported(format!("cell attribute decode failed: {error}"))
                            })?
                            .into_owned();
                        if key == b"r" {
                            current_ref = Some(value);
                        } else if key == b"t" {
                            current_type = Some(value);
                        }
                    }
                } else if name_eq(event.name().as_ref(), b"v") {
                    in_value = true;
                } else if name_eq(event.name().as_ref(), b"t") && current_type.as_deref() == Some("inlineStr") {
                    in_inline_text = true;
                }
            }
            Ok(Event::Text(text_event)) => {
                if in_value || in_inline_text {
                    let decoded = text_event
                        .decode()
                        .map_err(|error| ContractError::Unsupported(format!("cell text decode failed: {error}")))?;
                    current_value.push_str(&decoded);
                }
            }
            Ok(Event::End(event)) => {
                if name_eq(event.name().as_ref(), b"v") {
                    in_value = false;
                } else if name_eq(event.name().as_ref(), b"t") {
                    in_inline_text = false;
                } else if name_eq(event.name().as_ref(), b"c") {
                    let Some(cell_ref) = current_ref.clone() else {
                        buffer.clear();
                        continue;
                    };
                    let value = match current_type.as_deref() {
                        Some("s") => current_value
                            .parse::<usize>()
                            .ok()
                            .and_then(|index| shared_strings.get(index))
                            .cloned()
                            .unwrap_or_default(),
                        _ => current_value.clone(),
                    };
                    cells.push((cell_ref, value));
                    current_ref = None;
                    current_type = None;
                    current_value.clear();
                }
            }
            Ok(Event::Eof) => break,
            Err(error) => {
                return Err(ContractError::Unsupported(format!(
                    "sheet xml parse failed: {error}"
                )))
            }
            _ => {}
        }
        buffer.clear();
    }

    Ok(cells)
}

fn xml_reader(xml: &str) -> Reader<&[u8]> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    reader
}

fn local_name(name: &[u8]) -> &[u8] {
    name.rsplit(|byte| *byte == b':').next().unwrap_or(name)
}

fn name_eq(name: &[u8], expected: &[u8]) -> bool {
    local_name(name) == expected
}

fn name_in(name: &[u8], expected: &[&str]) -> bool {
    expected
        .iter()
        .any(|candidate| local_name(name) == candidate.as_bytes())
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;
    use std::io::Write;

    use zip::CompressionMethod;
    use zip::write::SimpleFileOptions;

    use crate::contracts::Extractor;
    use crate::domain::{DocumentId, DocumentLocator, DocumentMeta};

    use super::{OfficeExtractor, PdfExtractor};

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

    #[test]
    fn office_extractor_reads_docx_paragraphs() {
        let meta = office_meta("docs/report.docx", "docx");
        let document = OfficeExtractor
            .extract(
                &build_zip_archive(&[(
                    "word/document.xml",
                    r#"<?xml version="1.0" encoding="UTF-8"?>
                    <w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
                      <w:body>
                        <w:p><w:r><w:t>Riverglass docx paragraph</w:t></w:r></w:p>
                        <w:p><w:r><w:t>Second paragraph</w:t></w:r></w:p>
                      </w:body>
                    </w:document>"#,
                )]),
                &meta,
            )
            .unwrap_or_else(|error| panic!("docx extraction should succeed: {error}"));

        assert_eq!(document.chunks.len(), 2);
        assert_eq!(document.chunks[0].anchor.locator.chunk_id.as_deref(), Some("paragraph-1"));
        assert!(document.text.contains("Riverglass docx paragraph"));
    }

    #[test]
    fn office_extractor_reads_pptx_slides() {
        let meta = office_meta("slides/deck.pptx", "pptx");
        let document = OfficeExtractor
            .extract(
                &build_zip_archive(&[
                    (
                        "ppt/slides/slide1.xml",
                        r#"<?xml version="1.0" encoding="UTF-8"?>
                        <p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"
                               xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
                          <p:cSld><p:spTree><p:sp><p:txBody><a:p><a:r><a:t>Riverglass slide one</a:t></a:r></a:p></p:txBody></p:sp></p:spTree></p:cSld>
                        </p:sld>"#,
                    ),
                    (
                        "ppt/slides/slide2.xml",
                        r#"<?xml version="1.0" encoding="UTF-8"?>
                        <p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"
                               xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
                          <p:cSld><p:spTree><p:sp><p:txBody><a:p><a:r><a:t>Second slide</a:t></a:r></a:p></p:txBody></p:sp></p:spTree></p:cSld>
                        </p:sld>"#,
                    ),
                ]),
                &meta,
            )
            .unwrap_or_else(|error| panic!("pptx extraction should succeed: {error}"));

        assert_eq!(document.chunks.len(), 2);
        assert_eq!(document.chunks[0].anchor.locator.slide, Some(1));
        assert_eq!(document.chunks[1].anchor.locator.slide, Some(2));
    }

    #[test]
    fn office_extractor_reads_xlsx_cells() {
        let meta = office_meta("tables/data.xlsx", "xlsx");
        let document = OfficeExtractor
            .extract(
                &build_zip_archive(&[
                    (
                        "xl/workbook.xml",
                        r#"<?xml version="1.0" encoding="UTF-8"?>
                        <workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
                          <sheets>
                            <sheet name="Summary" sheetId="1" r:id="rId1" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"/>
                          </sheets>
                        </workbook>"#,
                    ),
                    (
                        "xl/sharedStrings.xml",
                        r#"<?xml version="1.0" encoding="UTF-8"?>
                        <sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
                          <si><t>Riverglass cell</t></si>
                        </sst>"#,
                    ),
                    (
                        "xl/worksheets/sheet1.xml",
                        r#"<?xml version="1.0" encoding="UTF-8"?>
                        <worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
                          <sheetData>
                            <row r="1">
                              <c r="A1" t="s"><v>0</v></c>
                              <c r="B1"><v>42</v></c>
                            </row>
                          </sheetData>
                        </worksheet>"#,
                    ),
                ]),
                &meta,
            )
            .unwrap_or_else(|error| panic!("xlsx extraction should succeed: {error}"));

        assert_eq!(document.chunks.len(), 2);
        assert_eq!(document.chunks[0].anchor.locator.sheet.as_deref(), Some("Summary"));
        assert_eq!(document.chunks[0].anchor.locator.cell_range.as_deref(), Some("A1"));
        assert_eq!(document.chunks[0].text, "Riverglass cell");
    }

    fn office_meta(path: &str, extension: &str) -> DocumentMeta {
        DocumentMeta {
            id: DocumentId(path.to_owned()),
            locator: DocumentLocator {
                path: path.to_owned(),
            },
            extension: Some(extension.to_owned()),
            content_type: None,
            version: Some("v1".to_owned()),
            size_bytes: 0,
            modified_at: Some("100".to_owned()),
        }
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

    fn build_zip_archive(entries: &[(&str, &str)]) -> Vec<u8> {
        let mut cursor = Cursor::new(Vec::new());
        {
            let mut writer = zip::ZipWriter::new(&mut cursor);
            let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
            for (path, contents) in entries {
                writer
                    .start_file(path, options)
                    .unwrap_or_else(|error| panic!("zip entry should start: {error}"));
                writer
                    .write_all(contents.as_bytes())
                    .unwrap_or_else(|error| panic!("zip entry should write: {error}"));
            }
            writer
                .finish()
                .unwrap_or_else(|error| panic!("zip archive should finish: {error}"));
        }
        cursor.into_inner()
    }
}
